//! The command handlers behind each `sbctl` subcommand, grouped by the area
//! of a deployment they touch. `main` only parses arguments and dispatches here.

pub(crate) mod certificate;
pub(crate) mod config;
pub(crate) mod install;
pub(crate) mod serve;
pub(crate) mod status;
pub(crate) mod system;
pub(crate) mod update;
