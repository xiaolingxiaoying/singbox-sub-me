use base64::Engine;
use serde_json::json;

use super::client_skip_cert_verify;
use crate::canonical::CanonicalNode;
use crate::config::DeploymentConfig;
use crate::subscription::artifacts::SubscriptionError;
use crate::subscription::base64_uri;

/// The Shadowrocket-adapted Base64 URI list (research:
/// `docs/research/sing-box-client-version-differences.md` §6). Differences
/// from the plain `uri` rendering: passwords and SNI values are always
/// percent-encoded (Shadowrocket 2.2.44 fixed URI password decoding, which
/// implies special characters must arrive encoded), TUIC carries
/// `udp_relay_mode`, and AnyTLS follows the official anytls-go scheme with
/// the path slash and without the non-standard `security` parameter.
pub(crate) fn shadowrocket(
    config: &DeploymentConfig,
    nodes: &[CanonicalNode],
) -> Result<String, SubscriptionError> {
    let insecure = if client_skip_cert_verify(config) {
        1
    } else {
        0
    };
    let mut uris = String::new();
    // A URI authority needs IPv6 hosts bracketed; the server and Clash
    // renderers deliberately keep the bare address.
    let nodes = nodes
        .iter()
        .map(CanonicalNode::with_bracketed_host)
        .collect::<Vec<_>>();
    for node in &nodes {
        match &node {
            CanonicalNode::VlessReality {
                host,
                port,
                uuid,
                public_key,
                short_id,
                decoy_sni,
                ..
            } => uris.push_str(&format!("vless://{uuid}@{host}:{port}?encryption=none&flow=xtls-rprx-vision&security=reality&sni={}&fp=chrome&pbk={public_key}&sid={short_id}&type=tcp#{}\n", percent_encode(decoy_sni), node.tag())),
            CanonicalNode::VmessWebsocket {
                host,
                port,
                tls_server_name,
                uuid,
                path,
            } => {
                let payload = json!({"v": "2", "ps": node.tag(), "add": host, "port": port.to_string(), "id": uuid, "aid": "0", "scy": "auto", "net": "ws", "type": "none", "host": tls_server_name, "path": path, "tls": "tls", "sni": tls_server_name});
                let encoded = base64::engine::general_purpose::STANDARD
                    .encode(serde_json::to_vec(&payload).expect("JSON values serialize"));
                uris.push_str(&format!("vmess://{encoded}\n"));
            }
            CanonicalNode::Hysteria2 {
                host,
                port,
                tls_server_name,
                password,
            } => uris.push_str(&format!(
                "hysteria2://{}@{}:{}?insecure={}&sni={}#{}\n",
                percent_encode(password),
                host,
                port,
                insecure,
                percent_encode(tls_server_name),
                node.tag()
            )),
            CanonicalNode::Tuic {
                host,
                port,
                tls_server_name,
                uuid,
                password,
            } => uris.push_str(&format!(
                "tuic://{}:{}@{}:{}?congestion_control=bbr&udp_relay_mode=native&alpn=h3&insecure={}&sni={}#{}\n",
                percent_encode(uuid),
                percent_encode(password),
                host,
                port,
                insecure,
                percent_encode(tls_server_name),
                node.tag()
            )),
            CanonicalNode::Anytls {
                host,
                port,
                tls_server_name,
                password,
            } => uris.push_str(&format!(
                "anytls://{}@{}:{}/?insecure={}&sni={}#{}\n",
                percent_encode(password),
                host,
                port,
                insecure,
                percent_encode(tls_server_name),
                node.tag()
            )),
        }
    }
    Ok(base64_uri(&uris))
}

/// Percent-encodes everything outside the RFC 3986 unreserved set so secrets
/// with special characters survive URI parsing in every client.
fn percent_encode(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for byte in value.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' => {
                out.push(byte as char)
            }
            _ => out.push_str(&format!("%{byte:02X}")),
        }
    }
    out
}

pub(crate) fn uri(
    config: &DeploymentConfig,
    nodes: &[CanonicalNode],
) -> Result<String, SubscriptionError> {
    let insecure = if client_skip_cert_verify(config) {
        1
    } else {
        0
    };
    let mut uris = String::new();
    // A URI authority needs IPv6 hosts bracketed; the server and Clash
    // renderers deliberately keep the bare address.
    let nodes = nodes
        .iter()
        .map(CanonicalNode::with_bracketed_host)
        .collect::<Vec<_>>();
    for node in &nodes {
        match &node {
            CanonicalNode::VlessReality {
                host,
                port,
                uuid,
                public_key,
                short_id,
                decoy_sni,
                ..
            } => uris.push_str(&format!("vless://{uuid}@{host}:{port}?encryption=none&flow=xtls-rprx-vision&security=reality&sni={decoy_sni}&fp=chrome&pbk={public_key}&sid={short_id}&type=tcp#{}\n", node.tag())),
            CanonicalNode::VmessWebsocket {
                host,
                port,
                tls_server_name,
                uuid,
                path,
            } => {
                let payload = json!({"v": "2", "ps": node.tag(), "add": host, "port": port.to_string(), "id": uuid, "aid": "0", "scy": "auto", "net": "ws", "type": "none", "host": tls_server_name, "path": path, "tls": "tls", "sni": tls_server_name});
                let encoded = base64::engine::general_purpose::STANDARD
                    .encode(serde_json::to_vec(&payload).expect("JSON values serialize"));
                uris.push_str(&format!("vmess://{encoded}\n"));
            }
            CanonicalNode::Hysteria2 {
                host,
                port,
                tls_server_name,
                password,
            } => uris.push_str(&format!(
                "hysteria2://{password}@{host}:{port}?insecure={insecure}&sni={tls_server_name}#{}\n",
                node.tag()
            )),
            CanonicalNode::Tuic {
                host,
                port,
                tls_server_name,
                uuid,
                password,
            } => uris.push_str(&format!(
                "tuic://{uuid}:{password}@{host}:{port}?congestion_control=bbr&alpn=h3&insecure={insecure}&sni={tls_server_name}#{}\n",
                node.tag()
            )),
            CanonicalNode::Anytls {
                host,
                port,
                tls_server_name,
                password,
            } => uris.push_str(&format!(
                "anytls://{password}@{host}:{port}?security=tls&insecure={insecure}&sni={tls_server_name}#{}\n",
                node.tag()
            )),
        }
    }
    Ok(uris)
}

#[cfg(test)]
mod tests {
    use super::uri;
    use crate::config::{DeploymentConfig, ManagedProtocol, ProtocolPorts, SubscriptionMode};
    use crate::subscription::render::{clash, sing_box};
    use crate::subscription::{SubscriptionFormat, SubscriptionRoute, route_url};

    #[test]
    fn an_ipv6_host_is_bracketed_only_where_a_uri_authority_requires_it() {
        let config = DeploymentConfig::new_with_ports(
            SubscriptionMode::IpFallback,
            "2001:db8::1".into(),
            None,
            Some(2080),
            "ens3".into(),
            vec![ManagedProtocol::Hysteria2],
            Some("www.cloudflare.com".into()),
            ProtocolPorts::default(),
        )
        .expect("an IPv6 no-domain deployment is valid");
        let nodes = crate::canonical::nodes(&config);

        let uri = uri(&config, &nodes).expect("uri artifacts generate");
        assert!(
            uri.contains("@[2001:db8::1]:"),
            "an unbracketed IPv6 authority is not a parseable URI: {uri}"
        );
        let url = route_url(&config, SubscriptionRoute::Format(SubscriptionFormat::Uri))
            .expect("the subscription URL builds");
        assert!(
            url.starts_with("http://[2001:db8::1]:2080/sub/"),
            "the share link must bracket the IPv6 host: {url}"
        );

        let clash = clash(&config, &nodes).expect("clash artifacts generate");
        assert!(
            !clash.contains("[2001:db8::1]"),
            "configuration fields take a bare address; brackets leak into server fields"
        );
        let sing_box = sing_box(&config, &nodes).expect("sing-box artifacts generate");
        assert!(
            !sing_box.contains("[2001:db8::1]"),
            "sing-box `server` must carry the bare address"
        );
    }
}
