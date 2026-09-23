//! The subscription surface: every exported node format, the QR and
//! matrix routes, and how the serving path degrades without state.

use assert_cmd::Command;
use base64::Engine;
use predicates::prelude::*;
use std::fs;
use std::net::TcpListener;
use std::process::Command as ProcessCommand;
use tempfile::TempDir;

use crate::fixture::{
    free_high_tcp_port, http_get, http_request, initialize_ip_fallback_subscription,
    read_subscription_credential, sing_box_check_fixture, spawn_sbctl_serve, write_traffic_fixture,
};

#[test]
fn direct_domain_mode_generates_https_subscription_urls_without_an_http_port() {
    let fixture = TempDir::new().expect("temporary root is created");

    Command::cargo_bin("sbctl")
        .expect("sbctl binary is built")
        .args([
            "--root",
            fixture.path().to_str().expect("fixture path is UTF-8"),
            "config",
            "init",
            "--mode",
            "direct",
            "--subscription-host",
            "sub.example.test",
            "--interface",
            "ens3",
            "--protocol",
            "vless-reality",
            "--reality-decoy-sni",
            "www.cloudflare.com",
        ])
        .assert()
        .success();

    Command::cargo_bin("sbctl")
        .expect("sbctl binary is built")
        .args([
            "--root",
            fixture.path().to_str().expect("fixture path is UTF-8"),
            "sub",
            "--format",
            "uri",
        ])
        .assert()
        .success()
        .stdout(predicate::str::starts_with("https://sub.example.test/sub/"));

    assert!(
        fixture
            .path()
            .join("var/lib/sbctl/acme-webroot/.well-known/acme-challenge")
            .is_dir()
    );
}

#[test]
fn vless_reality_ip_fallback_exports_consistent_subscription_formats() {
    let fixture = TempDir::new().expect("temporary root is created");

    Command::cargo_bin("sbctl")
        .expect("sbctl binary is built")
        .args([
            "--root",
            fixture.path().to_str().expect("fixture path is UTF-8"),
            "config",
            "init",
            "--mode",
            "ip-fallback",
            "--subscription-host",
            "203.0.113.7",
            "--proxy-host",
            "198.51.100.9",
            "--http-port",
            "2080",
            "--interface",
            "ens3",
            "--protocol",
            "vless-reality",
            "--reality-decoy-sni",
            "www.cloudflare.com",
        ])
        .assert()
        .success();

    let config = fs::read_to_string(fixture.path().join("etc/sbctl/config.toml"))
        .expect("configuration is persisted");
    let credential = config
        .lines()
        .find_map(|line| {
            line.strip_prefix("subscription_credential = \"")
                .and_then(|value| value.strip_suffix('"'))
        })
        .expect("subscription credential is persisted");

    let artifacts = fixture.path().join("var/lib/sbctl/artifacts");
    let server = fs::read_to_string(artifacts.join("sing-box-server.json"))
        .expect("sing-box server configuration is cached");
    let sing_box = fs::read_to_string(artifacts.join("subscription-sing-box.json"))
        .expect("sing-box subscription is cached");
    let clash = fs::read_to_string(artifacts.join("subscription-clash.yaml"))
        .expect("Clash subscription is cached");
    let uri = fs::read_to_string(artifacts.join("subscription-uri.txt"))
        .expect("URI subscription is cached");
    let base64_uri = fs::read_to_string(artifacts.join("subscription-base64-uri.txt"))
        .expect("Base64 URI subscription is cached");
    assert!(sing_box.contains("\"type\": \"vless\""));
    assert!(server.contains("\"private_key\""));
    assert!(sing_box.contains("198.51.100.9"));
    assert!(clash.contains("type: vless"));
    assert!(clash.contains("198.51.100.9"));
    assert!(uri.starts_with("vless://"));
    assert!(uri.contains("198.51.100.9:"));
    for value in ["www.cloudflare.com", "security=reality", "xtls-rprx-vision"] {
        assert!(uri.contains(value), "URI contains {value}");
    }
    assert_eq!(
        base64::engine::general_purpose::STANDARD
            .decode(base64_uri.trim())
            .expect("Base64 URI subscription decodes"),
        uri.as_bytes(),
        "the Base64 URI subscription is an exact encoding of the canonical URI artifact"
    );

    Command::cargo_bin("sbctl")
        .expect("sbctl binary is built")
        .args([
            "--root",
            fixture.path().to_str().expect("fixture path is UTF-8"),
            "sub",
            "--format",
            "uri",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains(format!("/sub/{credential}/uri")));

    Command::cargo_bin("sbctl")
        .expect("sbctl binary is built")
        .args([
            "--root",
            fixture.path().to_str().expect("fixture path is UTF-8"),
            "sub",
            "--format",
            "base64-uri",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains(format!(
            "/sub/{credential}/uri.txt"
        )));
}

#[test]
fn domain_nodes_export_vmess_websocket_and_hysteria2_with_independent_tls_credentials() {
    let fixture = TempDir::new().expect("temporary root is created");
    let checker = sing_box_check_fixture(&fixture, true, &["vmess"]);
    let root = fixture.path().to_str().expect("fixture path is UTF-8");

    Command::cargo_bin("sbctl")
        .expect("sbctl binary is built")
        .args([
            "--root",
            root,
            "config",
            "init",
            "--mode",
            "direct",
            "--subscription-host",
            "sub.example.test",
            "--proxy-host",
            "proxy.example.test",
            "--interface",
            "ens3",
            "--protocol",
            "vless-reality",
            "--protocol",
            "vmess-websocket",
            "--protocol",
            "hysteria2",
            "--reality-decoy-sni",
            "www.cloudflare.com",
            "--sing-box-bin",
            checker.to_str().expect("checker path is UTF-8"),
        ])
        .assert()
        .success();

    let artifacts = fixture.path().join("var/lib/sbctl/artifacts");
    let server: serde_json::Value = serde_json::from_str(
        &fs::read_to_string(artifacts.join("sing-box-server.json"))
            .expect("sing-box server configuration is cached"),
    )
    .expect("server configuration is JSON");
    let subscription: serde_json::Value = serde_json::from_str(
        &fs::read_to_string(artifacts.join("subscription-sing-box.json"))
            .expect("sing-box subscription is cached"),
    )
    .expect("subscription is JSON");
    let inbounds = server["inbounds"].as_array().expect("inbounds are present");
    let outbounds = subscription["outbounds"]
        .as_array()
        .expect("outbounds are present");
    assert_eq!(inbounds.len(), 3);
    assert_eq!(outbounds.len(), 3);
    let vmess = outbounds
        .iter()
        .find(|node| node["type"] == "vmess")
        .expect("VMess WebSocket node is exported");
    let hysteria = outbounds
        .iter()
        .find(|node| node["type"] == "hysteria2")
        .expect("Hysteria2 node is exported");
    assert_eq!(vmess["server"], "proxy.example.test");
    assert_eq!(vmess["tls"]["server_name"], "sub.example.test");
    assert_eq!(hysteria["server"], "proxy.example.test");
    assert_eq!(hysteria["tls"]["server_name"], "sub.example.test");
    assert_ne!(vmess["server_port"], hysteria["server_port"]);
    assert_ne!(vmess["uuid"], hysteria["password"]);

    let clash = fs::read_to_string(artifacts.join("subscription-clash.yaml"))
        .expect("Clash subscription is cached");
    let _: serde_yaml::Value = serde_yaml::from_str(&clash).expect("Clash subscription is YAML");
    assert!(clash.contains("type: vmess"));
    assert!(clash.contains("type: hysteria2"));
    assert!(clash.contains("servername: sub.example.test"));
    assert!(clash.contains("sni: sub.example.test"));
    assert!(clash.contains(vmess["uuid"].as_str().expect("VMess UUID is text")));
    assert!(
        clash.contains(
            hysteria["password"]
                .as_str()
                .expect("Hysteria2 password is text")
        )
    );

    let uri = fs::read_to_string(artifacts.join("subscription-uri.txt"))
        .expect("URI subscription is cached");
    assert!(uri.contains("vmess://"));
    assert!(uri.contains("hysteria2://"));
    assert!(uri.contains("sni=sub.example.test"));
    let vmess_uri = uri
        .lines()
        .find(|line| line.starts_with("vmess://"))
        .expect("VMess URI is present");
    let vmess_payload: serde_json::Value = serde_json::from_slice(
        &base64::engine::general_purpose::STANDARD
            .decode(vmess_uri.trim_start_matches("vmess://"))
            .expect("VMess URI payload is base64"),
    )
    .expect("VMess URI payload is JSON");
    assert_eq!(vmess_payload["id"], vmess["uuid"]);
    assert_eq!(vmess_payload["port"], vmess["server_port"].to_string());
    assert!(
        uri.contains(
            hysteria["password"]
                .as_str()
                .expect("Hysteria2 password is text")
        )
    );
}

#[test]
fn domain_nodes_export_tuic_and_anytls_with_independent_tls_credentials() {
    let fixture = TempDir::new().expect("temporary root is created");
    let checker = sing_box_check_fixture(&fixture, true, &["tuic", "anytls"]);
    let root = fixture.path().to_str().expect("fixture path is UTF-8");

    Command::cargo_bin("sbctl")
        .expect("sbctl binary is built")
        .args([
            "--root",
            root,
            "config",
            "init",
            "--mode",
            "direct",
            "--subscription-host",
            "sub.example.test",
            "--proxy-host",
            "proxy.example.test",
            "--interface",
            "ens3",
            "--protocol",
            "tuic",
            "--protocol",
            "anytls",
            "--protocol",
            "vless-reality",
            "--protocol",
            "vmess-websocket",
            "--protocol",
            "hysteria2",
            "--reality-decoy-sni",
            "www.cloudflare.com",
            "--sing-box-bin",
            checker.to_str().expect("checker path is UTF-8"),
        ])
        .assert()
        .success();

    let artifacts = fixture.path().join("var/lib/sbctl/artifacts");
    let server: serde_json::Value = serde_json::from_str(
        &fs::read_to_string(artifacts.join("sing-box-server.json"))
            .expect("sing-box server configuration is cached"),
    )
    .expect("server configuration is JSON");
    let subscription: serde_json::Value = serde_json::from_str(
        &fs::read_to_string(artifacts.join("subscription-sing-box.json"))
            .expect("sing-box subscription is cached"),
    )
    .expect("subscription is JSON");
    let inbounds = server["inbounds"].as_array().expect("inbounds are present");
    let outbounds = subscription["outbounds"]
        .as_array()
        .expect("outbounds are present");
    assert_eq!(inbounds.len(), 5);
    assert_eq!(outbounds.len(), 5);
    let tuic = outbounds
        .iter()
        .find(|node| node["type"] == "tuic")
        .expect("TUIC node is exported");
    let anytls = outbounds
        .iter()
        .find(|node| node["type"] == "anytls")
        .expect("AnyTLS node is exported");
    assert_eq!(tuic["server"], "proxy.example.test");
    assert_eq!(tuic["tls"]["server_name"], "sub.example.test");
    assert_eq!(anytls["server"], "proxy.example.test");
    assert_eq!(anytls["tls"]["server_name"], "sub.example.test");
    assert_ne!(tuic["server_port"], anytls["server_port"]);
    assert_ne!(tuic["uuid"], anytls["password"]);
    assert_ne!(tuic["password"], anytls["password"]);

    let configuration = fs::read_to_string(fixture.path().join("etc/sbctl/config.toml"))
        .expect("configuration is persisted");
    let subscription_credential = configuration
        .lines()
        .find_map(|line| {
            line.strip_prefix("subscription_credential = \"")
                .and_then(|value| value.strip_suffix('\"'))
        })
        .expect("subscription credential is persisted");
    assert_ne!(subscription_credential, tuic["password"]);
    assert_ne!(subscription_credential, anytls["password"]);

    let mut baseline: sbctl::config::DeploymentConfig =
        toml::from_str(&configuration).expect("configuration is valid TOML");
    baseline.enabled_protocols.retain(|protocol| {
        !matches!(
            protocol,
            sbctl::config::ManagedProtocol::Tuic | sbctl::config::ManagedProtocol::Anytls
        )
    });
    baseline.tuic = None;
    baseline.anytls = None;
    let baseline_artifacts = sbctl::subscription::generated_artifacts(&baseline, fixture.path())
        .expect("existing protocol artifacts are generated");
    let baseline_subscription = baseline_artifacts
        .iter()
        .find(|(name, _)| *name == "subscription-sing-box.json")
        .map(|(_, contents)| serde_json::from_str::<serde_json::Value>(contents))
        .expect("baseline sing-box subscription is present")
        .expect("baseline sing-box subscription is JSON");
    let existing_types = ["vless", "vmess", "hysteria2"];
    let retained_outbounds = outbounds
        .iter()
        .filter(|node| existing_types.contains(&node["type"].as_str().unwrap_or_default()))
        .collect::<Vec<_>>();
    assert_eq!(
        retained_outbounds,
        baseline_subscription["outbounds"]
            .as_array()
            .expect("baseline outbounds are present")
            .iter()
            .collect::<Vec<_>>(),
        "adding TUIC and AnyTLS preserves generated existing protocol nodes"
    );

    let clash = fs::read_to_string(artifacts.join("subscription-clash.yaml"))
        .expect("Clash subscription is cached");
    let _: serde_yaml::Value = serde_yaml::from_str(&clash).expect("Clash subscription is YAML");
    assert!(clash.contains("type: tuic"));
    assert!(clash.contains("type: anytls"));
    assert!(clash.contains(tuic["uuid"].as_str().expect("TUIC UUID is text")));
    assert!(
        clash.contains(
            anytls["password"]
                .as_str()
                .expect("AnyTLS password is text")
        )
    );

    let uri = fs::read_to_string(artifacts.join("subscription-uri.txt"))
        .expect("URI subscription is cached");
    let tuic_uri = uri
        .lines()
        .find(|line| line.starts_with("tuic://"))
        .expect("TUIC URI is present");
    let anytls_uri = uri
        .lines()
        .find(|line| line.starts_with("anytls://"))
        .expect("AnyTLS URI is present");
    let parsed_tuic = url::Url::parse(tuic_uri).expect("TUIC URI is syntactically valid");
    assert_eq!(parsed_tuic.scheme(), "tuic");
    assert_eq!(parsed_tuic.host_str(), Some("proxy.example.test"));
    assert_eq!(
        parsed_tuic.port(),
        Some(tuic["server_port"].as_u64().expect("TUIC port") as u16)
    );
    let parsed_anytls = url::Url::parse(anytls_uri).expect("AnyTLS URI is syntactically valid");
    assert_eq!(parsed_anytls.scheme(), "anytls");
    assert_eq!(parsed_anytls.host_str(), Some("proxy.example.test"));
    assert_eq!(
        parsed_anytls.port(),
        Some(anytls["server_port"].as_u64().expect("AnyTLS port") as u16)
    );
    for credential in [
        tuic["uuid"].as_str().expect("TUIC UUID is text"),
        tuic["password"].as_str().expect("TUIC password is text"),
    ] {
        assert!(tuic_uri.contains(credential));
    }
    assert!(tuic_uri.contains("proxy.example.test"));
    assert!(tuic_uri.contains("sni=sub.example.test"));
    assert!(
        anytls_uri.contains(
            anytls["password"]
                .as_str()
                .expect("AnyTLS password is text")
        )
    );
    assert!(anytls_uri.contains("proxy.example.test"));
    assert!(anytls_uri.contains("sni=sub.example.test"));
}

#[test]
fn five_protocols_export_the_same_canonical_nodes_across_server_and_subscription_formats() {
    let fixture = TempDir::new().expect("temporary root is created");
    let checker = sing_box_check_fixture(
        &fixture,
        true,
        &["vless", "vmess", "hysteria2", "tuic", "anytls"],
    );
    let root = fixture.path().to_str().expect("fixture path is UTF-8");

    Command::cargo_bin("sbctl")
        .expect("sbctl binary is built")
        .args([
            "--root",
            root,
            "config",
            "init",
            "--mode",
            "direct",
            "--subscription-host",
            "sub.example.test",
            "--proxy-host",
            "proxy.example.test",
            "--interface",
            "ens3",
            "--protocol",
            "vless-reality",
            "--protocol",
            "vmess-websocket",
            "--protocol",
            "hysteria2",
            "--protocol",
            "tuic",
            "--protocol",
            "anytls",
            "--reality-decoy-sni",
            "www.cloudflare.com",
            "--sing-box-bin",
            checker.to_str().expect("checker path is UTF-8"),
        ])
        .assert()
        .success();

    let artifacts = fixture.path().join("var/lib/sbctl/artifacts");
    let server: serde_json::Value = serde_json::from_str(
        &fs::read_to_string(artifacts.join("sing-box-server.json"))
            .expect("server configuration is cached"),
    )
    .expect("server configuration is JSON");
    let subscription: serde_json::Value = serde_json::from_str(
        &fs::read_to_string(artifacts.join("subscription-sing-box.json"))
            .expect("sing-box subscription is cached"),
    )
    .expect("subscription is JSON");
    let clash = fs::read_to_string(artifacts.join("subscription-clash.yaml"))
        .expect("Clash subscription is cached");
    let uri = fs::read_to_string(artifacts.join("subscription-uri.txt"))
        .expect("URI subscription is cached");
    let inbounds = server["inbounds"].as_array().expect("inbounds are present");
    let outbounds = subscription["outbounds"]
        .as_array()
        .expect("outbounds are present");
    assert_eq!(inbounds.len(), 5);
    assert_eq!(outbounds.len(), 5);

    for outbound in outbounds {
        let kind = outbound["type"].as_str().expect("node type is present");
        let inbound = inbounds
            .iter()
            .find(|inbound| inbound["type"] == outbound["type"])
            .expect("every exported node has a server inbound");
        assert_eq!(
            outbound["server_port"], inbound["listen_port"],
            "{kind} keeps one port across the server and the client configuration"
        );
        assert_eq!(
            outbound["server"], "proxy.example.test",
            "{kind} uses the proxy host in every client format"
        );
        let expected_sni = if kind == "vless" {
            "www.cloudflare.com"
        } else {
            "sub.example.test"
        };
        assert_eq!(
            outbound["tls"]["server_name"], expected_sni,
            "{kind} uses the canonical TLS server name"
        );
        assert!(
            clash.contains(&outbound["server_port"].as_u64().expect("port").to_string()),
            "Clash carries the {kind} port"
        );
        let port_text = outbound["server_port"].as_u64().expect("port").to_string();
        if kind == "vmess" {
            let vmess_uri = uri
                .lines()
                .find(|line| line.starts_with("vmess://"))
                .expect("VMess URI is present");
            let payload: serde_json::Value = serde_json::from_slice(
                &base64::engine::general_purpose::STANDARD
                    .decode(vmess_uri.trim_start_matches("vmess://"))
                    .expect("VMess URI payload is base64"),
            )
            .expect("VMess URI payload is JSON");
            assert_eq!(payload["port"], port_text, "URI carries the vmess port");
        } else {
            assert!(uri.contains(&port_text), "URI carries the {kind} port");
        }
        assert!(uri.contains("proxy.example.test"));
        for secret in [outbound["uuid"].as_str(), outbound["password"].as_str()]
            .into_iter()
            .flatten()
        {
            assert!(
                clash.contains(secret),
                "Clash carries the same {kind} credential as the sing-box JSON"
            );
            if kind == "vmess" {
                let vmess_uri = uri
                    .lines()
                    .find(|line| line.starts_with("vmess://"))
                    .expect("VMess URI is present");
                let payload: serde_json::Value = serde_json::from_slice(
                    &base64::engine::general_purpose::STANDARD
                        .decode(vmess_uri.trim_start_matches("vmess://"))
                        .expect("VMess URI payload is base64"),
                )
                .expect("VMess URI payload is JSON");
                assert_eq!(
                    payload["id"], outbound["uuid"],
                    "URI carries the same VMess credential as the sing-box JSON"
                );
            } else {
                assert!(
                    uri.contains(secret),
                    "URI carries the same {kind} credential as the sing-box JSON"
                );
            }
        }
    }
    let _: serde_yaml::Value = serde_yaml::from_str(&clash).expect("Clash subscription is YAML");
}

/// G5: `sbctl node --uri` is the operator's own view of the same links the
/// `uri` artifact ships. It is opt-in because the lines carry node credentials,
/// and it must never carry the Subscription credential, which would let a
/// pasted screenshot both read and administer the deployment (ADR-0002).
#[test]
fn node_uri_flag_prints_the_native_share_links_and_plain_node_prints_none() {
    let fixture = TempDir::new().expect("temporary root is created");
    let checker = sing_box_check_fixture(
        &fixture,
        true,
        &["vless", "vmess", "hysteria2", "tuic", "anytls"],
    );
    let root = fixture.path().to_str().expect("fixture path is UTF-8");

    Command::cargo_bin("sbctl")
        .expect("sbctl binary is built")
        .args([
            "--root",
            root,
            "config",
            "init",
            "--mode",
            "direct",
            "--subscription-host",
            "sub.example.test",
            "--proxy-host",
            "proxy.example.test",
            "--interface",
            "ens3",
            "--protocol",
            "vless-reality",
            "--protocol",
            "vmess-websocket",
            "--protocol",
            "hysteria2",
            "--protocol",
            "tuic",
            "--protocol",
            "anytls",
            "--reality-decoy-sni",
            "www.cloudflare.com",
            "--sing-box-bin",
            checker.to_str().expect("checker path is UTF-8"),
        ])
        .assert()
        .success();

    let artifact = fs::read_to_string(
        fixture
            .path()
            .join("var/lib/sbctl/artifacts/subscription-uri.txt"),
    )
    .expect("URI subscription is cached");
    let credential = read_subscription_credential(&fixture);

    let plain = Command::cargo_bin("sbctl")
        .expect("sbctl binary is built")
        .args(["--root", root, "node"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let plain = String::from_utf8(plain).expect("plain node output is UTF-8");
    assert!(
        !plain.contains("://"),
        "share links stay opt-in; `sbctl node` is piped into logs and screenshots: {plain}"
    );

    let with_uris = Command::cargo_bin("sbctl")
        .expect("sbctl binary is built")
        .args(["--root", root, "node", "--uri"])
        .assert()
        .success()
        .stdout(predicate::str::contains("原生分享链接"))
        .get_output()
        .stdout
        .clone();
    let with_uris = String::from_utf8(with_uris).expect("--uri output is UTF-8");
    let links = with_uris
        .split_once("：\n")
        .or_else(|| with_uris.split_once(":\n"))
        .map(|(_, rest)| rest)
        .expect("the share-link header ends its own line");
    assert_eq!(
        links, artifact,
        "--uri must print the uri artifact byte-for-byte"
    );
    assert_eq!(
        links.lines().filter(|line| !line.is_empty()).count(),
        5,
        "every one of the five enabled nodes needs a line"
    );
    assert!(
        !with_uris.contains(&credential),
        "--uri must not expose the Subscription credential"
    );
}

/// G5: the index page sits behind the 256-bit path credential exactly like the
/// `uri` artifact does, so showing the same links there adds no boundary — it
/// only removes the need to download a credential'd file to read one's own
/// node parameters.
#[test]
fn the_index_page_shows_every_native_share_link_the_uri_route_serves() {
    let fixture = TempDir::new().expect("temporary root is created");
    let port = free_high_tcp_port();
    let checker = sing_box_check_fixture(&fixture, true, &["vless", "hysteria2", "tuic"]);
    write_traffic_fixture(&fixture, 100, 200, "boot-a");
    Command::cargo_bin("sbctl")
        .expect("sbctl binary is built")
        .args([
            "--root",
            fixture.path().to_str().expect("fixture path is UTF-8"),
            "config",
            "init",
            "--mode",
            "ip-fallback",
            "--subscription-host",
            "127.0.0.1",
            "--http-port",
            &port.to_string(),
            "--interface",
            "ens3",
            "--protocol",
            "vless-reality",
            "--protocol",
            "hysteria2",
            "--protocol",
            "tuic",
            "--reality-decoy-sni",
            "www.cloudflare.com",
            "--sing-box-bin",
            checker.to_str().expect("checker path is UTF-8"),
        ])
        .assert()
        .success();
    let credential = read_subscription_credential(&fixture);
    let stderr_log = fixture.path().join("serve.err");
    let mut server = spawn_sbctl_serve(&fixture, port, 2, &stderr_log);

    let served = http_get(port, &format!("/sub/{credential}/uri"));
    let (_, uris) = served
        .split_once("\r\n\r\n")
        .expect("the uri route has a body");
    let index = http_get(port, &format!("/sub/{credential}/index"));
    assert!(
        index.starts_with("HTTP/1.1 200 OK"),
        "index serves 200, got: {index}"
    );
    assert!(
        index.contains("节点与原生分享链接"),
        "the index page carries the native share-link section"
    );

    let lines: Vec<&str> = uris.lines().filter(|line| !line.is_empty()).collect();
    assert_eq!(
        lines.len(),
        3,
        "the three enabled nodes must each have a share link"
    );
    for line in &lines {
        assert!(
            index.contains(&html_escape(line)),
            "the index page must carry the {line} link"
        );
    }
    for tag in [
        "sbctl-vless-reality",
        "sbctl-hysteria2",
        "sbctl-tuic",
        "sbctl-anytls",
    ] {
        assert_eq!(
            index.contains(tag),
            tag != "sbctl-anytls",
            "only nodes this deployment enables are listed; {tag} is wrong"
        );
    }

    assert!(
        server
            .wait()
            .expect("server exits after the request limit")
            .success()
    );
}

/// Mirrors the private escaping `index_page::esc` applies, so a link that only
/// matches after escaping is caught.
fn html_escape(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#39;")
}

#[test]
fn proxy_credentials_cannot_read_the_subscription_and_the_subscription_credential_is_not_a_node_credential()
 {
    let fixture = TempDir::new().expect("temporary root is created");
    let checker = sing_box_check_fixture(
        &fixture,
        true,
        &["vless", "vmess", "hysteria2", "tuic", "anytls"],
    );
    let port = TcpListener::bind("127.0.0.1:0")
        .expect("an ephemeral port is available")
        .local_addr()
        .expect("address is available")
        .port();
    write_traffic_fixture(&fixture, 100, 200, "boot-a");
    let root = fixture.path().to_str().expect("fixture path is UTF-8");

    Command::cargo_bin("sbctl")
        .expect("sbctl binary is built")
        .args([
            "--root",
            root,
            "config",
            "init",
            "--mode",
            "external-proxy",
            "--subscription-host",
            "sub.example.test",
            "--listen-port",
            &port.to_string(),
            "--interface",
            "ens3",
            "--protocol",
            "vless-reality",
            "--protocol",
            "vmess-websocket",
            "--protocol",
            "hysteria2",
            "--protocol",
            "tuic",
            "--protocol",
            "anytls",
            "--reality-decoy-sni",
            "www.cloudflare.com",
            "--sing-box-bin",
            checker.to_str().expect("checker path is UTF-8"),
        ])
        .assert()
        .success();

    let configuration = fs::read_to_string(fixture.path().join("etc/sbctl/config.toml"))
        .expect("configuration is persisted");
    let deployment: sbctl::config::DeploymentConfig =
        toml::from_str(&configuration).expect("configuration is valid TOML");
    let subscription_credential = deployment.subscription_credential.clone();
    let node_secrets = sbctl::canonical::nodes(&deployment)
        .into_iter()
        .flat_map(|node| {
            node.secrets()
                .into_iter()
                .map(str::to_owned)
                .collect::<Vec<_>>()
        })
        .collect::<Vec<_>>();
    assert!(
        !node_secrets.contains(&subscription_credential),
        "the Subscription credential is independent from every Proxy credential"
    );
    for name in [
        "sing-box-server.json",
        "subscription-sing-box.json",
        "subscription-clash.yaml",
        "subscription-uri.txt",
        "subscription-base64-uri.txt",
    ] {
        let contents =
            fs::read_to_string(fixture.path().join("var/lib/sbctl/artifacts").join(name))
                .expect("artifact is readable");
        assert!(
            !contents.contains(&subscription_credential),
            "the Subscription credential never appears in {name}"
        );
    }

    Command::cargo_bin("sbctl")
        .expect("sbctl binary is built")
        .args(["--root", root, "accounting-reset"])
        .assert()
        .success();
    let stderr_log = fixture.path().join("serve.err");
    let mut server = spawn_sbctl_serve(&fixture, port, node_secrets.len() + 1, &stderr_log);

    let authorized = http_get(port, &format!("/sub/{subscription_credential}/uri"));
    assert!(authorized.starts_with("HTTP/1.1 200 OK"));
    for secret in &node_secrets {
        let rejected = http_get(port, &format!("/sub/{secret}/uri"));
        assert!(
            rejected.starts_with("HTTP/1.1 404 Not Found"),
            "a Proxy credential must not authorize subscription retrieval"
        );
    }
    assert!(server.wait().expect("server exits").success());
}

#[test]
fn ip_fallback_http_service_accepts_only_the_exact_credential_path_and_reports_vps_traffic() {
    let fixture = TempDir::new().expect("temporary root is created");
    let port = TcpListener::bind("127.0.0.1:0")
        .expect("an ephemeral port is available")
        .local_addr()
        .expect("address is available")
        .port();
    write_traffic_fixture(&fixture, 100, 200, "boot-a");
    Command::cargo_bin("sbctl")
        .expect("sbctl binary is built")
        .args([
            "--root",
            fixture.path().to_str().expect("fixture path is UTF-8"),
            "config",
            "init",
            "--mode",
            "ip-fallback",
            "--subscription-host",
            "127.0.0.1",
            "--http-port",
            &port.to_string(),
            "--interface",
            "ens3",
            "--protocol",
            "vless-reality",
            "--reality-decoy-sni",
            "www.cloudflare.com",
            "--monthly-traffic-limit",
            "1000",
        ])
        .assert()
        .success();
    let credential = fs::read_to_string(fixture.path().join("etc/sbctl/config.toml"))
        .expect("configuration is persisted")
        .lines()
        .find_map(|line| {
            line.strip_prefix("subscription_credential = \"")
                .and_then(|value| value.strip_suffix('"'))
        })
        .expect("credential is available")
        .to_owned();
    Command::cargo_bin("sbctl")
        .expect("sbctl binary is built")
        .args([
            "--root",
            fixture.path().to_str().expect("fixture path is UTF-8"),
            "accounting-reset",
        ])
        .assert()
        .success();
    let mut server = ProcessCommand::new(assert_cmd::cargo::cargo_bin!("sbctl"))
        .args([
            "--root",
            fixture.path().to_str().expect("fixture path is UTF-8"),
            "serve",
            "--bind",
            &format!("127.0.0.1:{port}"),
            "--max-requests",
            "6",
        ])
        .spawn()
        .expect("subscription service starts");

    let state_path = fixture.path().join("var/lib/sbctl/state.json");
    let before = fs::read(&state_path).expect("state is established before subscription reads");

    let response = http_get(port, &format!("/sub/{credential}/uri"));
    assert!(response.starts_with("HTTP/1.1 200 OK"));
    assert!(response.contains("subscription-userinfo: upload=0; download=0; total=1000; expire="));
    assert!(response.contains("cache-control: no-store"));
    let rejected = http_get(
        port,
        &format!("/sub/{credential}/uri?credential={credential}"),
    );
    assert!(rejected.starts_with("HTTP/1.1 404 Not Found"));
    assert!(
        http_get(port, &format!("/sub/{credential}/bogus")).starts_with("HTTP/1.1 404 Not Found"),
        "an unknown subscription format path is a uniform 404"
    );
    assert!(
        http_get(port, &format!("/sub/{credential}/uri/extra"))
            .starts_with("HTTP/1.1 404 Not Found"),
        "a trailing path segment is a uniform 404"
    );
    assert!(
        http_get(port, "/sub/wrong-credential/uri").starts_with("HTTP/1.1 404 Not Found"),
        "an invalid Subscription credential is a uniform 404"
    );
    assert!(
        http_get(port, "/sub/uri").starts_with("HTTP/1.1 404 Not Found"),
        "a missing credential path is a uniform 404"
    );
    assert_eq!(
        fs::read(&state_path).expect("state remains readable"),
        before,
        "subscription reads must not write accounting state"
    );
    assert!(
        server
            .wait()
            .expect("server exits after the request limit")
            .success()
    );
}

#[test]
fn subscription_matrix_routes_serve_content_types_and_reject_bad_paths() {
    let fixture = TempDir::new().expect("temporary root is created");
    let port = free_high_tcp_port();
    let credential = initialize_ip_fallback_subscription(&fixture, port);
    Command::cargo_bin("sbctl")
        .expect("sbctl binary is built")
        .args([
            "--root",
            fixture.path().to_str().expect("fixture path is UTF-8"),
            "accounting-reset",
        ])
        .assert()
        .success();
    let stderr_log = fixture.path().join("serve.err");
    let mut server = spawn_sbctl_serve(&fixture, port, 17, &stderr_log);

    for (path, content_type) in [
        ("sing-box.json", "application/json; charset=utf-8"),
        ("sing-box-full.json", "application/json; charset=utf-8"),
        ("sing-box-1.12.json", "application/json; charset=utf-8"),
        ("sing-box-1.14.json", "application/json; charset=utf-8"),
        ("clash.yaml", "application/yaml; charset=utf-8"),
        ("clash-1.18.yaml", "application/yaml; charset=utf-8"),
        ("uri", "text/plain; charset=utf-8"),
        ("uri.txt", "text/plain; charset=utf-8"),
        ("shadowrocket.txt", "text/plain; charset=utf-8"),
    ] {
        let response = http_get(port, &format!("/sub/{credential}/{path}"));
        assert!(
            response.starts_with("HTTP/1.1 200 OK"),
            "{path} must serve 200, got: {response}"
        );
        assert!(
            response.contains(&format!("content-type: {content_type}")),
            "{path} must carry {content_type}"
        );
        assert!(
            response.contains("subscription-userinfo:"),
            "{path} must carry traffic metadata"
        );
    }

    let qr = http_get(port, &format!("/sub/{credential}/qr/uri"));
    assert!(qr.starts_with("HTTP/1.1 200 OK"), "qr route serves 200");
    assert!(qr.contains("content-type: image/svg+xml"));
    assert!(qr.contains("<svg"), "qr route serves an SVG document");
    let index = http_get(port, &format!("/sub/{credential}/index"));
    assert!(index.starts_with("HTTP/1.1 200 OK"), "index serves 200");
    assert!(index.contains("content-type: text/html; charset=utf-8"));
    assert!(
        index.contains("AnyTLS 2.2.64"),
        "the index page documents the Shadowrocket protocol floors"
    );

    for path in [
        "bogus",
        "sing-box-1.09.json",
        "clash-1.17.yaml",
        "uri/extra",
    ] {
        assert!(
            http_get(port, &format!("/sub/{credential}/{path}"))
                .starts_with("HTTP/1.1 404 Not Found"),
            "{path} must be a uniform 404"
        );
    }
    assert!(
        http_get(port, &format!("/sub/{credential}/uri?x=1")).starts_with("HTTP/1.1 404 Not Found"),
        "a query parameter must be rejected"
    );
    assert!(
        http_get(port, "/sub/wrong-credential/uri").starts_with("HTTP/1.1 404 Not Found"),
        "an invalid credential must be a uniform 404"
    );
    assert!(
        server
            .wait()
            .expect("server exits after the request limit")
            .success()
    );
}

#[test]
fn qr_all_renders_every_matrix_format_and_a_positional_format_selects_one() {
    let fixture = TempDir::new().expect("temporary root is created");
    let port = free_high_tcp_port();
    let _credential = initialize_ip_fallback_subscription(&fixture, port);
    let root = fixture.path().to_str().expect("fixture path is UTF-8");

    let all = Command::cargo_bin("sbctl")
        .expect("sbctl binary is built")
        .args(["--root", root, "qr", "--all"])
        .assert()
        .success();
    let stdout = String::from_utf8(all.get_output().stdout.clone()).expect("stdout is UTF-8");
    assert_eq!(
        stdout.matches("\x1b[30;47m").count(),
        sbctl::subscription::subscription_matrix().len(),
        "qr --all renders one terminal code per matrix row"
    );
    assert!(
        stdout.contains("Shadowrocket"),
        "the matrix lists Shadowrocket"
    );

    let single = Command::cargo_bin("sbctl")
        .expect("sbctl binary is built")
        .args(["--root", root, "qr", "shadowrocket"])
        .assert()
        .success();
    let stdout = String::from_utf8(single.get_output().stdout.clone()).expect("stdout is UTF-8");
    assert_eq!(
        stdout.matches("\x1b[30;47m").count(),
        1,
        "a positional format renders exactly one terminal code"
    );
}

#[test]
fn subscription_degrades_to_artifact_only_for_a_missing_state_without_logging_the_credential() {
    let fixture = TempDir::new().expect("temporary root is created");
    let port = free_high_tcp_port();
    let credential = initialize_ip_fallback_subscription(&fixture, port);
    let stderr_log = fixture.path().join("serve.err");
    let mut server = spawn_sbctl_serve(&fixture, port, 2, &stderr_log);

    // A missing accounting state must not take the subscription offline: the
    // artifact still serves, only the traffic metadata is dropped.
    let available = http_get(port, &format!("/sub/{credential}/uri"));
    assert!(
        available.starts_with("HTTP/1.1 200 OK"),
        "the subscription survives a missing accounting state: {available}"
    );
    assert!(
        !available.contains("subscription-userinfo:"),
        "the degraded response must not carry traffic metadata"
    );
    assert!(
        available.contains("vless://"),
        "the artifact body still serves"
    );
    let rejected = http_get(port, "/sub/wrong-credential/uri");
    assert!(
        rejected.starts_with("HTTP/1.1 404 Not Found"),
        "an invalid credential stays 404 even when state is missing"
    );
    assert!(server.wait().expect("server exits").success());

    let log = fs::read_to_string(&stderr_log).expect("diagnostic log is readable");
    assert!(
        log.contains("subscription traffic metadata unavailable"),
        "a redacted diagnostic is written"
    );
    assert!(
        !log.contains(&credential),
        "the diagnostic must not contain the full Subscription credential"
    );
}

#[test]
fn subscription_degrades_to_artifact_only_for_a_corrupt_state_without_logging_the_credential() {
    let fixture = TempDir::new().expect("temporary root is created");
    let port = free_high_tcp_port();
    let credential = initialize_ip_fallback_subscription(&fixture, port);
    fs::create_dir_all(fixture.path().join("var/lib/sbctl")).expect("state directory is created");
    fs::write(fixture.path().join("var/lib/sbctl/state.json"), "not json")
        .expect("corrupt state is written");
    let stderr_log = fixture.path().join("serve.err");
    let mut server = spawn_sbctl_serve(&fixture, port, 2, &stderr_log);

    let available = http_get(port, &format!("/sub/{credential}/uri"));
    assert!(
        available.starts_with("HTTP/1.1 200 OK"),
        "the subscription survives a corrupt accounting state: {available}"
    );
    assert!(
        !available.contains("subscription-userinfo:"),
        "the degraded response must not carry traffic metadata"
    );
    http_get(port, "/sub/wrong-credential/uri");
    assert!(server.wait().expect("server exits").success());

    let log = fs::read_to_string(&stderr_log).expect("diagnostic log is readable");
    assert!(!log.contains(&credential));
}

#[test]
fn subscription_degrades_to_artifact_only_for_a_schema_mismatched_state() {
    let fixture = TempDir::new().expect("temporary root is created");
    let port = free_high_tcp_port();
    let credential = initialize_ip_fallback_subscription(&fixture, port);
    fs::create_dir_all(fixture.path().join("var/lib/sbctl")).expect("state directory is created");
    fs::write(
        fixture.path().join("var/lib/sbctl/state.json"),
        r#"{"schema_version":1,"cycle_key":"2024-02-01T00:00:00+00:00","interface":"ens3","baseline_rx":0,"baseline_tx":0,"accumulated_rx":0,"accumulated_tx":0,"boot_id":"boot-a","corrections":[]}"#,
    )
    .expect("schema-mismatched state is written");
    let stderr_log = fixture.path().join("serve.err");
    let mut server = spawn_sbctl_serve(&fixture, port, 2, &stderr_log);

    let available = http_get(port, &format!("/sub/{credential}/uri"));
    assert!(
        available.starts_with("HTTP/1.1 200 OK"),
        "the subscription survives a schema-mismatched state: {available}"
    );
    assert!(
        !available.contains("subscription-userinfo:"),
        "the degraded response must not carry traffic metadata"
    );
    http_get(port, "/sub/wrong-credential/uri");
    assert!(server.wait().expect("server exits").success());

    let log = fs::read_to_string(&stderr_log).expect("diagnostic log is readable");
    assert!(!log.contains(&credential));
}

#[test]
fn subscription_returns_a_redacted_503_for_a_missing_artifact() {
    let fixture = TempDir::new().expect("temporary root is created");
    let port = free_high_tcp_port();
    let credential = initialize_ip_fallback_subscription(&fixture, port);
    fs::remove_file(
        fixture
            .path()
            .join("var/lib/sbctl/artifacts/subscription-uri.txt"),
    )
    .expect("URI artifact is removed");
    let stderr_log = fixture.path().join("serve.err");
    let mut server = spawn_sbctl_serve(&fixture, port, 2, &stderr_log);

    assert!(
        http_get(port, &format!("/sub/{credential}/uri"))
            .starts_with("HTTP/1.1 503 Service Unavailable")
    );
    http_get(port, "/sub/wrong-credential/uri");
    assert!(server.wait().expect("server exits").success());

    let log = fs::read_to_string(&stderr_log).expect("diagnostic log is readable");
    assert!(!log.contains(&credential));
}

#[test]
fn subscription_userinfo_total_reflects_a_total_only_correction() {
    let fixture = TempDir::new().expect("temporary root is created");
    let port = free_high_tcp_port();
    let credential = initialize_ip_fallback_subscription(&fixture, port);
    Command::cargo_bin("sbctl")
        .expect("sbctl binary is built")
        .args([
            "--root",
            fixture.path().to_str().expect("fixture path is UTF-8"),
            "accounting-reset",
        ])
        .assert()
        .success();
    write_traffic_fixture(&fixture, 130, 260, "boot-a");
    Command::cargo_bin("sbctl")
        .expect("sbctl binary is built")
        .args([
            "--root",
            fixture.path().to_str().expect("fixture path is UTF-8"),
            "accounting-reset",
        ])
        .assert()
        .success();
    Command::cargo_bin("sbctl")
        .expect("sbctl binary is built")
        .args([
            "--root",
            fixture.path().to_str().expect("fixture path is UTF-8"),
            "traffic",
            "set-used",
            "--bytes",
            "5000",
        ])
        .assert()
        .success();

    let stderr_log = fixture.path().join("serve.err");
    let mut server = spawn_sbctl_serve(&fixture, port, 1, &stderr_log);
    let response = http_get(port, &format!("/sub/{credential}/uri"));
    assert!(response.starts_with("HTTP/1.1 200 OK"));
    assert!(
        response.contains("subscription-userinfo: upload=60; download=30; total=5000; expire=")
    );
    assert!(server.wait().expect("server exits").success());
}

#[test]
fn a_non_get_subscription_request_is_asked_for_get_rather_than_a_missing_route() {
    let fixture = TempDir::new().expect("temporary root is created");
    let port = free_high_tcp_port();
    let credential = initialize_ip_fallback_subscription(&fixture, port);
    let stderr_log = fixture.path().join("serve.err");
    let mut server = spawn_sbctl_serve(&fixture, port, 2, &stderr_log);

    let response = http_request("POST", port, &format!("/sub/{credential}/uri"));
    assert!(
        response.starts_with("HTTP/1.1 405 Method Not Allowed"),
        "a probe should learn the method is wrong, not that the subscription vanished: {response}"
    );
    assert!(response.contains("allow: GET"), "{response}");
    assert!(
        http_get(port, &format!("/sub/{credential}/uri")).starts_with("HTTP/1.1 200 OK"),
        "the route itself must keep serving GET"
    );
    assert!(server.wait().expect("server exits").success());
}
