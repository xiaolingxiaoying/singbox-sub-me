//! Subscription artefacts: the client version matrix, the generated artefacts and
//! configuration transactions, the HTTP/TLS/ACME server, and the per-format
//! renderers. Every public item of the crate-facing surface is re-exported here
//! so callers keep using `sbctl::subscription::...` unchanged.
mod artifacts;
mod profile;
mod render;
mod serve;
#[cfg(test)]
mod test_support;

use base64::Engine;

pub use artifacts::{
    DeploymentSnapshot, SubscriptionError, apply_config_transaction, check_sing_box_config,
    generated_artifacts, generated_artifacts_for_kernel, read_authorized, regenerate,
    resolve_full_profile, restore_config_transaction, route_url, subscription_url,
};

pub use profile::{
    CLASH_LEGACY_VERSION, ClientSubscriptionFormat, ClientSubscriptionRow, ClientVersion,
    SING_BOX_VERSION_PROFILES, SingBoxVersionProfile, SubscriptionFormat, SubscriptionLinkInfo,
    SubscriptionRoute, band_warning_for, client_subscription_matrix, installed_kernel_version,
    kernel_band_warning, latest_version_profile, parse_kernel_version, subscription_matrix,
};
pub use render::{
    AI_DOMAIN_SUFFIXES, AUTO_TAG, SELECTOR_TAG, ensure_external_proxy_listener_available,
};
pub use serve::{redact_secret, serve};

/// The native share link for one managed node (`vless://…`, `vmess://…`,
/// `hysteria2://…`, `tuic://…`, `anytls://…`).
///
/// This is the same string the `uri` artifact carries, exposed so the index page
/// and `sbctl status nodes --uri` can show a node's parameters without making the
/// operator download a credential'd file to see their own configuration. The
/// returned line keeps its trailing newline, matching the artifact byte-for-byte.
pub fn node_share_link(
    config: &crate::config::DeploymentConfig,
    node: &crate::canonical::CanonicalNode,
) -> String {
    render::node_uri(render::insecure_flag(config), &node.with_bracketed_host())
}

fn base64_uri(uri: &str) -> String {
    base64::engine::general_purpose::STANDARD.encode(uri.as_bytes())
}

fn constant_time_eq(left: &[u8], right: &[u8]) -> bool {
    let mut different = left.len() ^ right.len();
    for index in 0..left.len().max(right.len()) {
        different |= usize::from(*left.get(index).unwrap_or(&0) ^ *right.get(index).unwrap_or(&0));
    }
    different == 0
}
