//! The application half of the window: construction, the snapshot sync and
//! field commit paths, the close and exit decisions, and the `Render` impl.

use std::path::PathBuf;

use client_core::command::SettingsPatch;
use client_core::state::ProxyGroupSnapshot;
use client_core::{ClientCommand, ClientController, system_proxy};
use gpui::{
    Context, InteractiveElement, IntoElement, KeyDownEvent, ParentElement, Render, ScrollHandle,
    StatefulInteractiveElement, Styled, Window, div, px, rgb,
};

use crate::parse::{
    ImportRequest, file_import_request, link_edit_request, parse_count, parse_port,
    subscription_request, url_editor_target,
};
use crate::state::{
    ExitChoice, INPUT_FIELDS, InputField, LogLevelFilter, Page, Sbgui, TextField, Tone, env_locale,
    env_page, env_settings_section, env_show_clear_confirm, env_show_exit_confirm,
    env_show_import_panel, env_show_stop_confirm, env_show_url_editor, env_startup_notice,
};
use crate::theme::{
    BG, BORDER, CONTENT_MAX, CONTENT_PAD, DANGER, GAP_ITEM, GAP_SECTION, LABEL, TEXT, WINDOW_RADIUS,
};
use crate::tr;

impl Sbgui {
    pub(crate) fn new(
        controller: ClientController,
        data_dir: PathBuf,
        startup_notice: Option<String>,
        cx: &mut Context<Self>,
    ) -> Self {
        let snapshot = controller.snapshot();
        // The band this is only ever filled by a read that failed at launch,
        // which the screenshot harness never causes. The seam produces the
        // sentence the real outcome produces, so the layout has frame evidence.
        let startup_notice = startup_notice.or_else(|| {
            env_startup_notice()
                .then(|| {
                    crate::startup::Launch::DefaultsInstead(
                        "settings.toml (Permission denied (os error 13))".to_owned(),
                    )
                })?
                .notice(env_locale())
        });
        let mut view = Self {
            controller,
            data_dir,
            startup_notice,
            snapshot,
            page: env_page().unwrap_or(Page::Dashboard),
            group_index: 0,
            confirm_exit: env_show_exit_confirm(),
            confirm_stop_core: env_show_stop_confirm(),
            locale: env_locale(),
            exit_choice: None,
            inputs: INPUT_FIELDS.map(|_| TextField {
                focus: cx.focus_handle(),
                text: String::new(),
            }),
            log_level: LogLevelFilter::default(),
            settings_section: env_settings_section().unwrap_or_default(),
            show_subscription_import: env_show_import_panel(),
            subscription_error: None,
            import_file_error: None,
            editing_profile_url: None,
            profile_url_error: None,
            show_rule_sets: false,
            show_all_rules: false,
            show_all_connections: false,
            core_menu_open: false,
            advanced_open: true,
            node_card_view: false,
            paused_connections: None,
            selected_connection: None,
            log_scroll: ScrollHandle::default(),
            log_tail: std::cell::Cell::new(None),
            painted_minute: 0,
            log_wrap: true,
            log_follow: true,
            confirm_close_all: false,
            confirm_delete_profile: None,
            confirm_clear_override: env_show_clear_confirm(),
            confirm_traffic_restart: None,
        };
        // The seam replays the click instead of painting its result: the panel is
        // open, so pressing 「添加」 on the still-empty field goes through the very
        // path a user takes. A frame that shows no panel is the old bug returning.
        if env_show_import_panel() {
            view.submit_sub_url(cx);
        }
        // Same seam, same replay: 「编辑链接」 is a click the harness cannot make,
        // and arming through the handler is what makes the frame the real one —
        // including the link the field is prefilled from.
        if env_show_url_editor()
            && let Some(profile) = view.snapshot.profiles.first()
        {
            let name = profile.name.clone();
            view.arm_url_editor(&name, cx);
        }
        view
    }

    pub(crate) fn field(&self, field: InputField) -> &TextField {
        &self.inputs[field as usize]
    }

    fn field_mut(&mut self, field: InputField) -> &mut TextField {
        &mut self.inputs[field as usize]
    }

    pub(crate) fn send(&self, command: ClientCommand) {
        let _ = self.controller.send(command);
    }

    /// Fields that mirror persisted settings re-sync from the snapshot
    /// whenever they are not being edited, so the UI never shows a stale
    /// value after the engine applies (or rejects) a change.
    fn sync_fields(&mut self, window: &Window) {
        let settings = &self.snapshot.settings;
        let expected = [
            (InputField::Mirror, settings.mirror.clone()),
            (InputField::MixedPort, settings.mixed_port.to_string()),
            (InputField::TestUrl, settings.test_url.clone()),
            (
                InputField::AutoUpdateMinutes,
                settings.auto_update_minutes.to_string(),
            ),
            (InputField::CoreVersion, settings.core_version.clone()),
        ];
        for (field, value) in expected {
            let state = self.field_mut(field);
            if !state.focus.is_focused(window) && state.text != value {
                state.text = value;
            }
        }
    }

    pub(crate) fn handle_field_key(
        &mut self,
        field: InputField,
        event: &KeyDownEvent,
        cx: &mut Context<Self>,
    ) {
        let keystroke = &event.keystroke;
        let modifier_held = keystroke.modifiers.control
            || keystroke.modifiers.alt
            || keystroke.modifiers.platform
            || keystroke.modifiers.function;
        if keystroke.modifiers.control || keystroke.modifiers.platform {
            // These fields have no menu and no right-click, so Ctrl+V is the
            // only way to paste a subscription link copied elsewhere; every
            // other modified key stays ignored rather than inserting control
            // characters.
            if keystroke.key.as_str() == "v"
                && let Some(text) = cx.read_from_clipboard().and_then(|item| item.text())
            {
                self.field_mut(field)
                    .text
                    .push_str(&text.replace(['\r', '\n'], ""));
            }
            return;
        }
        if !modifier_held {
            if let Some(typed) = keystroke
                .key_char
                .as_deref()
                .filter(|typed| !typed.contains('\n') && !typed.contains('\r'))
            {
                self.field_mut(field).text.push_str(typed);
            } else {
                match keystroke.key.as_str() {
                    "backspace" => {
                        self.field_mut(field).text.pop();
                    }
                    "space" => self.field_mut(field).text.push(' '),
                    _ => {}
                }
            }
        }
        match keystroke.key.as_str() {
            "enter"
                if matches!(
                    field,
                    InputField::ConnFilter
                        | InputField::LogQuery
                        | InputField::ProxySearch
                        | InputField::RuleSearch
                        | InputField::SubUrl
                        | InputField::SubName
                        | InputField::SubFile
                        | InputField::SubEditUrl
                ) =>
            {
                self.commit_field(field, cx)
            }
            "escape" => self.reset_field(field),
            _ => {}
        }
        cx.notify();
    }

    /// Applies a committed field. The two filters already apply live on every
    /// keystroke; the settings fields send a partial update, and the engine
    /// reports the outcome (including rejection while the core runs) in the
    /// page header's status line.
    fn commit_field(&mut self, field: InputField, cx: &mut Context<Self>) {
        let text = self.field(field).text.trim().to_owned();
        match field {
            InputField::ConnFilter
            | InputField::LogQuery
            | InputField::ProxySearch
            | InputField::RuleSearch => {}
            InputField::SubUrl | InputField::SubName => {
                // Enter in the name field submits the form, the same as in the
                // link field: the name is optional, so it can never be the only
                // thing a submission has.
                self.submit_sub_url(cx);
            }
            InputField::SubFile => {
                self.submit_sub_file(cx);
            }
            InputField::SubEditUrl => {
                self.submit_profile_url(cx);
            }
            InputField::Mirror => self.send(ClientCommand::UpdateSettings(SettingsPatch {
                mirror: Some(text),
                ..Default::default()
            })),
            InputField::MixedPort => match parse_port(&text) {
                Some(port) => self.send(ClientCommand::UpdateSettings(SettingsPatch {
                    mixed_port: Some(port),
                    ..Default::default()
                })),
                None => self.reset_field(field),
            },
            InputField::TestUrl => {
                if !text.is_empty() {
                    self.send(ClientCommand::UpdateSettings(SettingsPatch {
                        test_url: Some(text),
                        ..Default::default()
                    }));
                }
            }
            InputField::AutoUpdateMinutes => match parse_count(&text) {
                Some(minutes) => self.send(ClientCommand::UpdateSettings(SettingsPatch {
                    auto_update_minutes: Some(minutes),
                    ..Default::default()
                })),
                None => self.reset_field(field),
            },
            InputField::CoreVersion => {
                // Empty means "track the latest release" again.
                self.send(ClientCommand::UpdateSettings(SettingsPatch {
                    core_version: Some(text.trim_start_matches('v').to_owned()),
                    ..Default::default()
                }));
            }
        }
    }

    /// Submits the import panel's link and its optional name, and reports
    /// whether the click was taken, which is the only answer that lets the panel
    /// close. An empty field used to be a silent early `return` while the click
    /// handler closed the panel anyway, so the entry vanished without a word
    /// (issue 02 item 4).
    ///
    /// Both branches repaint: `button()` and `send()` notify nobody, and the poll
    /// loop only notifies when the snapshot actually changed, so a branch that
    /// mutated view state without notifying could sit un-drawn for a minute.
    pub(crate) fn submit_sub_url(&mut self, cx: &mut Context<Self>) -> bool {
        let taken = match subscription_request(
            &self.field(InputField::SubName).text,
            &self.field(InputField::SubUrl).text,
        ) {
            ImportRequest::Send {
                command,
                keep_in_field,
            } => {
                self.subscription_error = None;
                self.send(command);
                // A non-HTTP line stays in the field so it can be fixed; the
                // engine's rejection shows up in the header status line either way.
                if !keep_in_field {
                    self.field_mut(InputField::SubUrl).text.clear();
                }
                true
            }
            ImportRequest::Reject(reason) => {
                self.subscription_error = Some(reason);
                false
            }
        };
        cx.notify();
        taken
    }

    /// 「导入本地 JSON」: the path field's half of the same panel, committed as
    /// [`ClientCommand::ImportProfileFile`]. The engine reads the file and either
    /// makes a profile out of it or says why it could not, so the close rule is
    /// the link's: the panel goes when the click was taken, and the path text
    /// survives to be fixed after a refusal.
    pub(crate) fn submit_sub_file(&mut self, cx: &mut Context<Self>) -> bool {
        let taken = match file_import_request(&self.field(InputField::SubFile).text) {
            ImportRequest::Send {
                command,
                keep_in_field,
            } => {
                self.import_file_error = None;
                self.send(command);
                if !keep_in_field {
                    self.field_mut(InputField::SubFile).text.clear();
                }
                true
            }
            ImportRequest::Reject(reason) => {
                self.import_file_error = Some(reason);
                false
            }
        };
        cx.notify();
        taken
    }

    /// 「保存链接」 on one row: the same command rule as an import, but rewriting
    /// the profile the editor was opened on. Reported as taken so the caller can
    /// close the editor; a refusal leaves it open with its reason and its text.
    pub(crate) fn submit_profile_url(&mut self, cx: &mut Context<Self>) -> bool {
        let Some(profile) = self.editing_profile_url.clone() else {
            return false;
        };
        let taken = match link_edit_request(&profile, &self.field(InputField::SubEditUrl).text) {
            ImportRequest::Send {
                command,
                keep_in_field,
            } => {
                self.profile_url_error = None;
                self.send(command);
                if !keep_in_field {
                    self.field_mut(InputField::SubEditUrl).text.clear();
                }
                true
            }
            ImportRequest::Reject(reason) => {
                self.profile_url_error = Some(reason);
                false
            }
        };
        cx.notify();
        taken
    }

    /// Closes the link editor and drops what was typed into it: 「取消」, Esc and
    /// the open row's own button all leave the editor in this state.
    pub(crate) fn close_url_editor(&mut self) {
        self.editing_profile_url = None;
        self.profile_url_error = None;
        self.field_mut(InputField::SubEditUrl).text.clear();
    }

    /// The row's 「编辑链接」 button, and the review seam: one editor at a time,
    /// prefilled with the link that profile holds — a file-only profile has no
    /// link, so its editor opens empty. Clicking the open row's button again
    /// closes it, so the button never offers an action it will not do.
    pub(crate) fn arm_url_editor(&mut self, profile: &str, cx: &mut Context<Self>) {
        let next = url_editor_target(self.editing_profile_url.as_deref(), profile);
        let text = next
            .as_deref()
            .and_then(|name| {
                self.snapshot
                    .profiles
                    .iter()
                    .find(|item| item.name == name)
                    .map(|item| item.url.clone())
            })
            .unwrap_or_default();
        self.field_mut(InputField::SubEditUrl).text = text;
        self.profile_url_error = None;
        self.editing_profile_url = next;
        cx.notify();
    }

    pub(crate) fn save_settings(&mut self, cx: &mut Context<Self>) {
        for field in [
            InputField::Mirror,
            InputField::MixedPort,
            InputField::TestUrl,
            InputField::AutoUpdateMinutes,
            InputField::CoreVersion,
        ] {
            self.commit_field(field, cx);
        }
        cx.notify();
    }

    /// Esc: filters and the subscription fields clear; settings fields restore the
    /// persisted value, so a half-typed edit never pretends to be saved.
    fn reset_field(&mut self, field: InputField) {
        let value = match field {
            InputField::ConnFilter
            | InputField::LogQuery
            | InputField::ProxySearch
            | InputField::RuleSearch
            | InputField::SubUrl
            | InputField::SubName
            | InputField::SubFile
            | InputField::SubEditUrl => String::new(),
            InputField::Mirror => self.snapshot.settings.mirror.clone(),
            InputField::MixedPort => self.snapshot.settings.mixed_port.to_string(),
            InputField::TestUrl => self.snapshot.settings.test_url.clone(),
            InputField::AutoUpdateMinutes => self.snapshot.settings.auto_update_minutes.to_string(),
            InputField::CoreVersion => self.snapshot.settings.core_version.clone(),
        };
        self.field_mut(field).text = value;
        // Disowning the link also disowns the complaint about it: the error
        // speaks about the text that was there, which Esc has just removed.
        match field {
            InputField::SubUrl => self.subscription_error = None,
            InputField::SubFile => self.import_file_error = None,
            // Nothing to edit with no link left in the field, and an editor that
            // outlived its own Esc would then save an empty one.
            InputField::SubEditUrl => self.close_url_editor(),
            _ => {}
        }
    }

    /// Sends a command, holding back the ones that must not happen by accident.
    ///
    /// Stopping the core drops every proxied connection and, with the system
    /// proxy on, takes other applications offline, so the kit requires an
    /// explicit confirmation first. Everything else goes straight through.
    pub(crate) fn request(&mut self, command: ClientCommand, cx: &mut Context<Self>) {
        if matches!(command, ClientCommand::StopCore) && !self.confirm_stop_core {
            self.confirm_stop_core = true;
            cx.notify();
            return;
        }
        self.send(command);
    }

    pub(crate) fn answer_stop_core(&mut self, confirm: bool, cx: &mut Context<Self>) {
        self.confirm_stop_core = false;
        if confirm {
            self.send(ClientCommand::StopCore);
        }
        cx.notify();
    }

    /// The veto GPUI consults for `WM_CLOSE` (Alt+F4, taskbar close). A `false`
    /// return keeps the window open so the exit decision can be asked for
    /// first; the custom close button routes through [`Self::request_close`]
    /// because a client-area click never reaches this hook.
    pub(crate) fn handle_close_request(&mut self, cx: &mut Context<Self>) -> bool {
        if self.exit_choice.is_none() && self.snapshot.system_proxy_enabled {
            if !self.confirm_exit {
                self.confirm_exit = true;
                cx.notify();
            }
            return false;
        }
        self.teardown();
        true
    }

    pub(crate) fn choose_exit(&mut self, choice: ExitChoice, window: &mut Window) {
        if choice == ExitChoice::Restore {
            // Synchronous and fast (registry + refresh broadcast); the engine
            // is about to die with the process, so no command round-trip.
            let _ = system_proxy::disable(&self.data_dir);
            self.snapshot.system_proxy_enabled = false;
        }
        self.exit_choice = Some(choice);
        self.teardown();
        window.remove_window();
    }

    /// Stops the core and waits for the engine to reap it, before the window
    /// disappears. The engine's runtime is leaked for the process lifetime, so
    /// nothing would ever drop the child handle and `kill_on_drop` would not
    /// run: sing-box would outlive the client holding the mixed port and, in
    /// TUN mode, the routes it took over.
    fn teardown(&self) {
        self.controller.shutdown();
    }

    pub(crate) fn selected_group(&self) -> Option<ProxyGroupSnapshot> {
        self.snapshot
            .proxy_groups
            .get(self.group_index)
            .or_else(|| self.snapshot.proxy_groups.first())
            .cloned()
    }
}

impl Render for Sbgui {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.sync_fields(window);
        let page = self.page;
        div()
            .size_full()
            .rounded(px(WINDOW_RADIUS))
            .overflow_hidden()
            .border_1()
            .border_color(rgb(BORDER))
            .font_family("Segoe UI Variable, Microsoft YaHei UI, Noto Sans SC")
            .text_color(rgb(TEXT))
            .bg(rgb(BG))
            .flex()
            .flex_col()
            .child(self.titlebar(window, cx))
            .child(
                div()
                    .flex_1()
                    .flex()
                    .overflow_hidden()
                    .child(self.sidebar(page, cx))
                    .child(
                        div()
                            .flex_1()
                            .h_full()
                            .min_h(px(0.0))
                            .min_w(px(0.0))
                            .bg(rgb(BG))
                            .flex()
                            .flex_col()
                            .child(self.toolbar(page, cx))
                            .children(self.startup_notice.clone().map(|message| {
                                // One band between the toolbar and the page, in
                                // the palette's own warning colour: it is the
                                // launch path speaking, not the engine, and the
                                // user decides when it has said enough.
                                div()
                                    .px(px(CONTENT_PAD))
                                    .pt(px(GAP_SECTION))
                                    .flex()
                                    .flex_wrap()
                                    .items_center()
                                    .justify_between()
                                    .gap(px(GAP_ITEM))
                                    .child(
                                        div()
                                            .flex_1()
                                            .min_w(px(240.0))
                                            .text_size(px(LABEL))
                                            .line_height(px(19.0))
                                            .text_color(rgb(DANGER))
                                            .child(message),
                                    )
                                    .child(self.button(
                                        "dismiss-startup-notice",
                                        tr!(self.locale, "知道了", "Got it"),
                                        Tone::Neutral,
                                        None,
                                        cx,
                                        |view, cx| {
                                            view.startup_notice = None;
                                            cx.notify();
                                        },
                                    ))
                            }))
                            .child(
                                div()
                                    .id("page-scroll")
                                    .flex_1()
                                    .overflow_y_scroll()
                                    .pt(px(GAP_SECTION))
                                    .px(px(CONTENT_PAD))
                                    .pb(px(32.0))
                                    .child(
                                        div()
                                            .w_full()
                                            .max_w(px(CONTENT_MAX))
                                            .mx_auto()
                                            .child(self.content(window, cx)),
                                    ),
                            ),
                    ),
            )
            .children(
                (self.confirm_exit && self.exit_choice.is_none()).then(|| self.exit_overlay(cx)),
            )
            .children(self.confirm_stop_core.then(|| self.stop_core_overlay(cx)))
    }
}
