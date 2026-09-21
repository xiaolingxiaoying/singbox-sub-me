use std::fmt;

use crate::state::ClientSnapshot;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ClientError {
    pub operation: String,
    pub message: String,
}

impl fmt::Display for ClientError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", self.operation, self.message)
    }
}

impl std::error::Error for ClientError {}

#[derive(Clone, Debug)]
pub enum ClientEvent {
    /// The full state changed. Boxed because a snapshot is much larger than the
    /// other variants.
    SnapshotChanged(Box<ClientSnapshot>),
    OperationStarted(String),
    OperationFinished(String),
    Error(ClientError),
}
