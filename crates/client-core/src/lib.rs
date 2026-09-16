//! UI-independent shared client control plane.

#[path = "../../sbtui/src/clash_api.rs"]
pub mod clash_api;
#[path = "../../sbtui/src/core.rs"]
pub mod core;
#[path = "../../sbtui/src/settings.rs"]
pub mod settings;
#[path = "../../sbtui/src/subscription.rs"]
pub mod subscription;
#[path = "../../sbtui/src/system_proxy.rs"]
pub mod system_proxy;

pub mod command;
pub mod controller;
pub mod event;
pub mod state;

pub use command::ClientCommand;
pub use controller::ClientController;
pub use event::{ClientError, ClientEvent};
pub use state::ClientSnapshot;
