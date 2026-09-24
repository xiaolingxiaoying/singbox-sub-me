//! UI-independent shared client control plane.
//!
//! Both the terminal client (`sbtui`) and the desktop client (`sbgui`) build on
//! this crate: it owns the clash_api control channel, the sing-box core
//! lifecycle, the persisted settings/profiles, subscription handling, and the
//! operating-system proxy integration. UIs only render [`ClientSnapshot`] state
//! and send [`ClientCommand`]s, so the two clients cannot drift apart.

pub mod clash_api;
pub mod command;
/// The per-profile "覆写配置文件内容" model: storage, merge, reserved fields.
/// (Named `config_override` because `override` is a reserved keyword.)
pub mod config_override;
pub mod controller;
pub mod core;
pub mod event;
pub mod event_code;
pub mod format;
pub mod settings;
pub mod state;
pub mod subscription;
pub mod system_proxy;

pub use command::ClientCommand;
pub use controller::ClientController;
pub use event::{ClientError, ClientEvent};
pub use state::ClientSnapshot;
