use crate::clash_api::OutboundMode;
use crate::system_proxy::TrafficMode;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ClientCommand {
    StartCore,
    StopCore,
    UpdateSubscription,
    SwitchProfile(String),
    SwitchNode(String),
    TestNode(String),
    ToggleSystemProxy,
    SetTrafficMode(TrafficMode),
    SetOutboundMode(OutboundMode),
    CloseConnection(String),
    CloseAllConnections,
}
