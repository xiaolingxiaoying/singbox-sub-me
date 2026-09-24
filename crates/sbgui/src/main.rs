//! sbgui — the desktop sing-box client.
//!
//! The window is a pure renderer over `client_core::ClientController`: a
//! background engine owns the core, the clash_api channel and the persisted
//! settings, publishes a [`ClientSnapshot`] roughly four times a second, and
//! the UI only draws that snapshot and sends [`ClientCommand`]s. This is the
//! same control plane the terminal client uses, so the two clients cannot drift
//! apart.
//!
//! THESIS: calm proxy control, informed by the Serein prototype and adapted to sing-box.
//! OWN-WORLD: cool neutral canvas, teal actions, pale active navigation, quiet line icons.
//! STORY: find a section in the sidebar, understand its state, and change it without visual noise.
//! FIRST VIEWPORT: 216px navigation sidebar plus one toolbar holding the page name and the
//! kernel, mode, node, system-proxy and TUN state.
//! FORM: a desktop control surface that separates blocks with whitespace, not with ink.
//! FINISH: unreviewed and undocumented is unfinished; this build ends with the finish review, the verdict, and DESIGN.md.

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod app;
mod chrome;
mod components;
mod lang;
mod overlay;
mod pages;
mod parse;
mod startup;
mod state;
mod theme;

use std::borrow::Cow;
use std::path::Path;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use anyhow::Result;
use client_core::settings::{Profiles, Settings};
use client_core::{ClientController, settings};
use gpui::{
    App, AppContext as _, AssetSource, Bounds, SharedString, TitlebarOptions, WindowBounds,
    WindowOptions, px, size,
};
// Only the Windows chrome takes a `&Window`; the title bar itself lives in
// chrome.rs, so an unconditional import would be unused on Linux.
#[cfg(windows)]
use gpui::Window;
use gpui_platform::application;

use crate::state::{Sbgui, env_window_size};
use crate::theme::{BRAND_ICON_PATH, DATA_DIR};

pub(crate) struct SereinAssets;

impl AssetSource for SereinAssets {
    fn load(&self, path: &str) -> Result<Option<Cow<'static, [u8]>>> {
        if path == BRAND_ICON_PATH {
            Ok(Some(Cow::Borrowed(include_bytes!("../assets/serein.ico"))))
        } else {
            Ok(None)
        }
    }

    fn list(&self, _path: &str) -> Result<Vec<SharedString>> {
        Ok(vec![BRAND_ICON_PATH.into()])
    }
}

/// Loads one settings file the same way the engine does, so the window can
/// open before the engine's first poll completes. The error is the caller's
/// business: falling back to defaults silently would show the user settings
/// they never chose.
fn load_settings(dir: &Path) -> Result<Settings> {
    Settings::load_or_create(dir)
}

/// Records one launch outcome in the trace file, and keeps its sentence for the
/// window when the process still gets to open one.
fn report(
    dir: &Path,
    outcome: &startup::Launch,
    locale: crate::lang::Locale,
    notices: &mut Vec<String>,
) {
    if let Some(trace) = outcome.trace() {
        startup::record(dir, &trace);
    }
    notices.extend(outcome.notice(locale));
}

fn main() {
    let locale = state::env_locale();
    // The engine (controller task, tokio::fs, reqwest) runs on this runtime for
    // the whole process lifetime. It is leaked on purpose: moved into the GPUI
    // launch closure it would be dropped the moment that closure returns, the
    // runtime would shut down, and every engine await — starting with the
    // auto-start subscription read — would fail with "background task failed".
    let runtime: &'static tokio::runtime::Runtime = Box::leak(Box::new(
        tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .expect("tokio runtime"),
    ));
    let dir = match settings::data_dir_for(DATA_DIR) {
        Ok(dir) => dir,
        Err(error) => {
            // The data directory is where the trace file lives, so this is the
            // one outcome with nowhere to write: the console and a non-zero exit
            // code are all that a start this early can leave behind.
            let outcome = startup::Launch::NoDataDir(error.to_string());
            eprintln!(
                "sbgui: {}",
                outcome.trace().expect("a failed start always has a reason")
            );
            std::process::exit(1);
        }
    };
    startup::record(
        &dir,
        &format!("sbgui {} 启动 / starting", env!("CARGO_PKG_VERSION")),
    );
    // From here on, a panic is also a start that failed with no console to say
    // so; the hook is what keeps GPUI's own abort from being silent.
    startup::install_panic_hook(&dir);
    let mut notices: Vec<String> = Vec::new();
    // The mixed port, the OS proxy and the runtime configuration are all
    // per-directory or machine-global, so a second instance would fight the
    // first over all three. Held for as long as the event loop runs.
    let lock = settings::acquire_instance_lock(&dir);
    let outcome = match &lock {
        Ok(Some(_)) => startup::Launch::Proceed,
        Ok(None) => startup::Launch::AlreadyRunning,
        Err(error) => startup::Launch::LockFailed(error.to_string()),
    };
    report(&dir, &outcome, locale, &mut notices);
    if outcome.fatal() {
        // Leaving is right; leaving without a word was the bug. The trace file
        // now carries the reason, in the language the user reads and the one the
        // support channel does, next to the process id that said it.
        return;
    }
    // Past the fatal outcomes, so the lock is held.
    let _instance = lock.expect("only an uncontended lock reaches this point");
    // The files the engine is about to read: opening them here is what lets the
    // window show up before the first poll, and failing to open one used to be
    // something the client kept to itself.
    if let Err(error) = load_settings(&dir) {
        report(
            &dir,
            &startup::Launch::DefaultsInstead(format!("settings.toml ({error})")),
            locale,
            &mut notices,
        );
    }
    if let Err(error) = Profiles::load_or_create(&dir) {
        report(
            &dir,
            &startup::Launch::DefaultsInstead(format!("profiles.toml ({error})")),
            locale,
            &mut notices,
        );
    }
    let ui_startup_notice = (!notices.is_empty()).then(|| notices.join(" · "));
    let ui_data_dir = dir.clone();
    let controller = {
        let _guard = runtime.enter();
        ClientController::start(dir)
    };

    application()
        .with_assets(SereinAssets)
        .run(move |cx: &mut App| {
            let (win_w, win_h) = env_window_size();
            let bounds = Bounds::centered(None, size(px(win_w), px(win_h)), cx);
            cx.open_window(
                WindowOptions {
                    // The operating-system titlebar is intentionally transparent/hidden.
                    // The complete titlebar, including branding and window controls, is
                    // rendered by `Sbgui::titlebar` through GPUI.
                    titlebar: Some(TitlebarOptions {
                        title: Some("Serein".into()),
                        appears_transparent: true,
                        ..Default::default()
                    }),
                    is_movable: true,
                    window_bounds: Some(WindowBounds::Windowed(bounds)),
                    window_min_size: Some(size(px(860.0), px(640.0))),
                    ..Default::default()
                },
                move |window, cx| {
                    #[cfg(windows)]
                    apply_windows_window_chrome(window);

                    let view = cx.new(|cx| {
                        let view = Sbgui::new(controller, ui_data_dir, ui_startup_notice, cx);
                        let refresh = cx.spawn(async move |this, cx| {
                            loop {
                                cx.background_executor()
                                    .timer(Duration::from_millis(400))
                                    .await;
                                let Some(entity) = this.upgrade() else {
                                    break;
                                };
                                entity.update(cx, |view: &mut Sbgui, cx| {
                                    let snapshot = view.controller.snapshot();
                                    if std::env::var_os("CLIENT_POLL_TRACE").is_some() {
                                        eprintln!(
                                            "gui-trace: core_running={} conns={} down={}",
                                            snapshot.core_running,
                                            snapshot.connections.connections.len(),
                                            snapshot.download_speed,
                                        );
                                    }
                                    // Repainting unconditionally kept a window
                                    // redrawing, decoding icons and resampling
                                    // the graph four times a second while nothing
                                    // had changed — and while it was hidden.
                                    let changed = snapshot != view.snapshot;
                                    if changed {
                                        view.snapshot = snapshot;
                                    }
                                    let minute = SystemTime::now()
                                        .duration_since(UNIX_EPOCH)
                                        .map(|elapsed| elapsed.as_secs() / 60)
                                        .unwrap_or(view.painted_minute);
                                    if changed || minute != view.painted_minute {
                                        view.painted_minute = minute;
                                        cx.notify();
                                    }
                                });
                            }
                        });
                        refresh.detach();
                        view
                    });
                    // Alt+F4 and the taskbar close arrive as WM_CLOSE and are
                    // vetoed here; the custom close button is a client-area
                    // click that bypasses this hook and calls
                    // `Sbgui::request_close` instead.
                    let close_view = view.downgrade();
                    window.on_window_should_close(cx, move |_, cx| {
                        close_view
                            .update(cx, |view, cx| view.handle_close_request(cx))
                            .unwrap_or(true)
                    });
                    view
                },
            )
            .unwrap();
        });
}

#[cfg(windows)]
fn apply_windows_window_chrome(window: &Window) {
    use raw_window_handle::{HasWindowHandle, RawWindowHandle};
    use windows::Win32::Foundation::HWND;
    use windows::Win32::Graphics::Dwm::{
        DWMWA_WINDOW_CORNER_PREFERENCE, DWMWCP_ROUND, DwmSetWindowAttribute,
    };

    let Ok(handle) = HasWindowHandle::window_handle(window) else {
        return;
    };
    let RawWindowHandle::Win32(handle) = handle.as_raw() else {
        return;
    };

    let hwnd = HWND(handle.hwnd.get() as *mut std::ffi::c_void);
    let preference = DWMWCP_ROUND;
    let _ = unsafe {
        DwmSetWindowAttribute(
            hwnd,
            DWMWA_WINDOW_CORNER_PREFERENCE,
            &preference as *const _ as *const std::ffi::c_void,
            std::mem::size_of_val(&preference) as u32,
        )
    };
}
