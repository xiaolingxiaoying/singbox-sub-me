//! One module per sidebar page.
//!
//! A page renders the snapshot plus its own slice of view state and never
//! reaches into another page's state.

pub(crate) mod connections;
pub(crate) mod dashboard;
pub(crate) mod logs;
pub(crate) mod proxies;
pub(crate) mod rules;
pub(crate) mod settings;
pub(crate) mod subscriptions;
