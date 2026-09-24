//! sbtui — a terminal UI sing-box proxy client.
//!
//! Tabs (Tab / number keys): dashboard, proxies, connections, logs, settings.
//! Everything runs on the keyboard: start/stop the core, switch nodes, run
//! latency tests, toggle the system proxy or TUN mode, and manage
//! subscription profiles.
//!
//! The terminal client is a pure renderer over `client_core::ClientController`
//! — the exact control plane the desktop client uses. A background engine owns
//! the sing-box core, the clash_api channel and the persisted settings; this
//! UI only draws [`ClientSnapshot`] and sends [`ClientCommand`]s, so the two
//! clients cannot drift apart.

pub use client_core::{ClientCommand, ClientController, ClientError, ClientEvent, ClientSnapshot};
/// The shared control plane lives in `client-core` so the desktop client can
/// reuse exactly the same clash_api client, core manager, settings store,
/// subscription handling and OS-proxy integration. Re-exported at the crate
/// root so existing `crate::clash_api::…` paths keep working.
pub use client_core::{
    clash_api, command, core, format, settings, state, subscription, system_proxy,
};

mod app;
mod input;
mod signals;
mod style;
mod view;

use std::path::PathBuf;
use std::time::Duration;

use anyhow::Result;
use clap::Parser;
use crossterm::event::{Event, EventStream, KeyCode, KeyEventKind, KeyModifiers};
use futures_util::StreamExt;

use crate::app::App;
use crate::input::handle_key;
use crate::view::draw;

const TICK_MS: u64 = 500;

#[derive(Parser)]
#[command(name = "sbtui", version, about = "终端 sing-box 代理客户端")]
struct Cli {
    /// Print the resolved data directory and exit (for debugging).
    #[arg(long)]
    print_dir: bool,
}

pub async fn run() -> Result<()> {
    let cli = Cli::parse();
    let dir = settings::data_dir()?;
    if cli.print_dir {
        println!("{}", dir.display());
        return Ok(());
    }
    // The mixed port and the OS proxy are machine-global, so a second instance
    // would fight the first over both. Held for the whole function.
    let _instance = match settings::acquire_instance_lock(&dir)? {
        Some(lock) => lock,
        None => {
            eprintln!("另一个 sbtui 实例正在使用 {}，请先退出它。", dir.display());
            return Ok(());
        }
    };
    let mut terminal = ratatui::init();
    // The engine starts here (including the `auto_start` bring-up), so the
    // terminal client and the desktop client boot the core identically.
    let controller = ClientController::start(dir.clone());
    let result = run_app(&mut terminal, controller, dir).await;
    ratatui::restore();
    result
}

async fn run_app(
    terminal: &mut ratatui::DefaultTerminal,
    controller: ClientController,
    dir: PathBuf,
) -> Result<()> {
    let mut app = App::new(controller, dir);
    let mut events = EventStream::new();
    let mut tick = tokio::time::interval(Duration::from_millis(TICK_MS));
    // Subscribed once, before the loop: a handler installed per iteration would
    // miss whatever arrives between iterations.
    let mut shutdown = std::pin::pin!(signals::shutdown_signal());
    // Every exit path (including a failed read or draw) has to reach the
    // teardown below, so the loop records the error and breaks instead.
    let mut loop_error: Option<anyhow::Error> = None;

    loop {
        tokio::select! {
            event = events.next() => {
                let Some(event) = event else { break };
                match event {
                    Ok(Event::Key(key)) if key.kind == KeyEventKind::Press => {
                        // The quit keys only quit outside the help and input
                        // overlays; inside them `q` is a plain character for
                        // the search box or the profile-name editor.
                        let quit_requested = !app.show_help
                            && app.input.is_none()
                            && ((key.code == KeyCode::Char('q') && key.modifiers.is_empty())
                                || (key.code == KeyCode::Char('c')
                                    && key.modifiers.contains(KeyModifiers::CONTROL)));
                        if quit_requested {
                            if app.snapshot.system_proxy_enabled && !app.confirm_quit {
                                app.confirm_quit = true;
                                app.status =
                                    "系统代理仍开启：再按 q 退出并保留代理设置，或按 p 关闭后退出"
                                        .to_owned();
                            } else {
                                break;
                            }
                        } else {
                            handle_key(&mut app, key);
                        }
                    }
                    Ok(_) => {}
                    Err(error) => {
                        loop_error = Some(error.into());
                        break;
                    }
                }
            }
            _ = tick.tick() => {
                app.refresh_snapshot();
            }
            // The OS is taking the process away — a closed terminal, `kill`, a
            // service stop, a logout. Leave through the same bottom of this
            // function that `q` reaches, because the teardown below is what puts
            // the machine-wide proxy back; breaking out here is the whole fix.
            () = &mut shutdown => {
                break;
            }
        }
        if let Err(error) = terminal.draw(|frame| draw(frame, &mut app)) {
            loop_error = Some(error.into());
            break;
        }
    }
    // The OS proxy is a machine-wide setting: clear it unless the user
    // explicitly chose to keep it. Then wait for the engine to reap the core —
    // `kill_on_drop` only fires if that task is scheduled again, and returning
    // from here ends the process.
    app.refresh_snapshot();
    if signals::should_clear_proxy(app.snapshot.system_proxy_enabled, app.confirm_quit) {
        let _ = system_proxy::disable(&app.dir);
    }
    app.controller.shutdown();
    match loop_error {
        Some(error) => Err(error),
        None => Ok(()),
    }
}
