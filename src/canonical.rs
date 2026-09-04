use base64::Engine;
use x25519_dalek::{X25519_BASEPOINT_BYTES, x25519};

use crate::config::{DeploymentConfig, ManagedProtocol};

/// The VLESS Reality client must present the public key that corresponds to the
/// server's private key. Deriving it from the stored private key at
/// artifact-generation time guarantees the pair cannot drift apart, even after a
/// messy historical regeneration left a stale public key behind (issue #1).
fn vless_public_key_from_private(private_key: &str, fallback: &str) -> String {
    let decoded = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .decode(private_key)
        .ok()
        .and_then(|bytes| <[u8; 32]>::try_from(bytes).ok());
    match decoded {
        Some(scalar) => {
            let public = x25519(scalar, X25519_BASEPOINT_BYTES);
            base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(public)
        }
        None => fallback.to_owned(),
    }
}

/// A single enabled Managed protocol rendered from the persisted deployment.
/// Every generated artifact derives its node set, host, port, credentials and
/// TLS fields from this one canonical model, so the sing-box server
/// configuration and the three Subscription formats cannot drift apart.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CanonicalNode {
    VlessReality {
        host: String,
        port: u16,
        tls_server_name: String,
        uuid: String,
        private_key: String,
        public_key: String,
        short_id: String,
        decoy_sni: String,
    },
    VmessWebsocket {
        host: String,
        port: u16,
        tls_server_name: String,
        uuid: String,
        path: String,
    },
    Hysteria2 {
        host: String,
        port: u16,
        tls_server_name: String,
        password: String,
    },
    Tuic {
        host: String,
        port: u16,
        tls_server_name: String,
        uuid: String,
        password: String,
    },
    Anytls {
        host: String,
        port: u16,
        tls_server_name: String,
        password: String,
    },
}

impl CanonicalNode {
    pub fn protocol(&self) -> ManagedProtocol {
        match self {
            Self::VlessReality { .. } => ManagedProtocol::VlessReality,
            Self::VmessWebsocket { .. } => ManagedProtocol::VmessWebsocket,
            Self::Hysteria2 { .. } => ManagedProtocol::Hysteria2,
            Self::Tuic { .. } => ManagedProtocol::Tuic,
            Self::Anytls { .. } => ManagedProtocol::Anytls,
        }
    }

    pub fn tag(&self) -> &'static str {
        match self {
            Self::VlessReality { .. } => "sbctl-vless-reality",
            Self::VmessWebsocket { .. } => "sbctl-vmess-websocket",
            Self::Hysteria2 { .. } => "sbctl-hysteria2",
            Self::Tuic { .. } => "sbctl-tuic",
            Self::Anytls { .. } => "sbctl-anytls",
        }
    }

    pub fn host(&self) -> &str {
        match self {
            Self::VlessReality { host, .. }
            | Self::VmessWebsocket { host, .. }
            | Self::Hysteria2 { host, .. }
            | Self::Tuic { host, .. }
            | Self::Anytls { host, .. } => host,
        }
    }

    pub fn port(&self) -> u16 {
        match self {
            Self::VlessReality { port, .. }
            | Self::VmessWebsocket { port, .. }
            | Self::Hysteria2 { port, .. }
            | Self::Tuic { port, .. }
            | Self::Anytls { port, .. } => *port,
        }
    }

    pub fn tls_server_name(&self) -> &str {
        match self {
            Self::VlessReality {
                tls_server_name, ..
            }
            | Self::VmessWebsocket {
                tls_server_name, ..
            }
            | Self::Hysteria2 {
                tls_server_name, ..
            }
            | Self::Tuic {
                tls_server_name, ..
            }
            | Self::Anytls {
                tls_server_name, ..
            } => tls_server_name,
        }
    }

    pub fn transport(&self) -> &'static str {
        match self {
            Self::VlessReality { .. } | Self::VmessWebsocket { .. } | Self::Anytls { .. } => "tcp",
            Self::Hysteria2 { .. } | Self::Tuic { .. } => "udp",
        }
    }

    /// Every protocol-specific secret that authenticates a proxy node. These
    /// must never authorize subscription retrieval, and the Subscription
    /// credential must never appear among them.
    pub fn secrets(&self) -> Vec<&str> {
        match self {
            Self::VlessReality {
                uuid,
                private_key,
                public_key,
                ..
            } => vec![uuid, private_key, public_key],
            Self::VmessWebsocket { uuid, .. } => vec![uuid],
            Self::Hysteria2 { password, .. } => vec![password],
            Self::Tuic { uuid, password, .. } => vec![uuid, password],
            Self::Anytls { password, .. } => vec![password],
        }
    }
}

/// Derives the canonical node set from the persisted deployment, in the same
/// order as `enabled_protocols`. Host and TLS server name come from the
/// deployment's proxy and subscription hosts; credentials and ports come from
/// the generated per-protocol node configuration.
pub fn nodes(config: &DeploymentConfig) -> Vec<CanonicalNode> {
    let host = config
        .proxy_host
        .as_deref()
        .unwrap_or(&config.subscription_host);
    let tls_server_name = &config.subscription_host;
    config
        .enabled_protocols
        .iter()
        .map(|protocol| match protocol {
            ManagedProtocol::VlessReality => {
                let node = config.vless_reality.as_ref().expect("validated");
                CanonicalNode::VlessReality {
                    host: host.to_owned(),
                    port: node.listen_port,
                    tls_server_name: tls_server_name.clone(),
                    uuid: node.uuid.clone(),
                    private_key: node.private_key.clone(),
                    public_key: vless_public_key_from_private(&node.private_key, &node.public_key),
                    short_id: node.short_id.clone(),
                    decoy_sni: config
                        .reality_decoy_sni
                        .as_deref()
                        .expect("validated")
                        .to_owned(),
                }
            }
            ManagedProtocol::VmessWebsocket => {
                let node = config.vmess_websocket.as_ref().expect("validated");
                CanonicalNode::VmessWebsocket {
                    host: host.to_owned(),
                    port: node.listen_port,
                    tls_server_name: tls_server_name.clone(),
                    uuid: node.uuid.clone(),
                    path: node.path.clone(),
                }
            }
            ManagedProtocol::Hysteria2 => {
                let node = config.hysteria2.as_ref().expect("validated");
                CanonicalNode::Hysteria2 {
                    host: host.to_owned(),
                    port: node.listen_port,
                    tls_server_name: tls_server_name.clone(),
                    password: node.password.clone(),
                }
            }
            ManagedProtocol::Tuic => {
                let node = config.tuic.as_ref().expect("validated");
                CanonicalNode::Tuic {
                    host: host.to_owned(),
                    port: node.listen_port,
                    tls_server_name: tls_server_name.clone(),
                    uuid: node.uuid.clone(),
                    password: node.password.clone(),
                }
            }
            ManagedProtocol::Anytls => {
                let node = config.anytls.as_ref().expect("validated");
                CanonicalNode::Anytls {
                    host: host.to_owned(),
                    port: node.listen_port,
                    tls_server_name: tls_server_name.clone(),
                    password: node.password.clone(),
                }
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::nodes;
    use crate::config::{DeploymentConfig, ManagedProtocol, ProtocolPorts, SubscriptionMode};

    #[test]
    fn canonical_nodes_derive_host_ports_credentials_and_tls_from_the_config() {
        let config = DeploymentConfig::new_with_ports(
            SubscriptionMode::Direct,
            "sub.example.test".into(),
            Some("proxy.example.test".into()),
            None,
            "ens3".into(),
            vec![
                ManagedProtocol::VlessReality,
                ManagedProtocol::VmessWebsocket,
                ManagedProtocol::Hysteria2,
                ManagedProtocol::Tuic,
                ManagedProtocol::Anytls,
            ],
            Some("www.cloudflare.com".into()),
            ProtocolPorts::default(),
        )
        .expect("a full canonical deployment is valid");

        let canonical = nodes(&config);
        assert_eq!(canonical.len(), 5);
        for node in &canonical {
            assert_eq!(node.host(), "proxy.example.test");
            assert_eq!(node.tls_server_name(), "sub.example.test");
        }
        let vless = &canonical[0];
        assert_eq!(
            vless.port(),
            config.vless_reality.as_ref().unwrap().listen_port
        );
        let vless_reality = config.vless_reality.clone().unwrap();
        assert!(vless.secrets().contains(&vless_reality.uuid.as_str()));
        let hysteria = &canonical[2];
        assert_eq!(hysteria.transport(), "udp");
        assert_eq!(canonical[4].transport(), "tcp");
    }

    #[test]
    fn canonical_nodes_default_to_the_subscription_host_as_the_proxy_host() {
        let config = DeploymentConfig::new(
            SubscriptionMode::IpFallback,
            "203.0.113.7".into(),
            None,
            Some(2080),
            "ens3".into(),
            vec![ManagedProtocol::VlessReality],
            Some("www.cloudflare.com".into()),
        )
        .expect("an IP fallback deployment is valid");

        let canonical = nodes(&config);
        assert_eq!(canonical[0].host(), "203.0.113.7");
        assert_eq!(canonical[0].tls_server_name(), config.subscription_host);
    }

    #[test]
    fn canonical_nodes_follow_the_enabled_protocol_order() {
        let config = DeploymentConfig::new(
            SubscriptionMode::Direct,
            "sub.example.test".into(),
            None,
            None,
            "ens3".into(),
            vec![
                ManagedProtocol::Anytls,
                ManagedProtocol::VlessReality,
                ManagedProtocol::Tuic,
            ],
            Some("www.cloudflare.com".into()),
        )
        .expect("a selective deployment is valid");

        let canonical = nodes(&config);
        let protocols = canonical
            .iter()
            .map(|node| node.protocol())
            .collect::<Vec<_>>();
        assert_eq!(
            protocols,
            vec![
                ManagedProtocol::Anytls,
                ManagedProtocol::VlessReality,
                ManagedProtocol::Tuic
            ]
        );
    }
}
