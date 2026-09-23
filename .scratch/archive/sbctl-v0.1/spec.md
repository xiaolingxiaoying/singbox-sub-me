# sbctl V0.1：单 VPS sing-box 控制面与私密订阅

Status: ready-for-agent

## Problem Statement

单个管理员需要在自己的 Debian 或 Ubuntu VPS 上可靠地部署和运营 sing-box，并通过一个私密订阅链接把节点导入客户端。现有上游 Bash 项目将协议生成、订阅、流量、证书、定时任务和系统修改分散在多个脚本与进程中，难以验证、升级和恢复。管理员需要一个低常驻内存、可审计、可回滚的原生工具，而不是另一个包装上游脚本的菜单。

## Solution

提供一个由 Rust 实现的单一 `sbctl` 二进制。它作为控制面管理 sing-box、生成五种 Managed protocol 的统一节点集、缓存三种 Subscription format、提供私密订阅端点，并报告单 VPS 的 VPS traffic。sing-box 继续作为数据面。

默认 Direct subscription mode 下，sbctl 在公网 80/443 直接提供 HTTPS 订阅并为 Certbot 提供 ACME webroot；在已有 Web 入口的主机上，管理员可改用 loopback 外部反代模式。没有可用域名时，工具提供明确标示为低安全性的 IP fallback subscription。所有可变配置与状态均以验证、原子提交和回滚为前提。

## User Stories

1. As the sole administrator, I want to install one `sbctl` binary on a fresh Debian or Ubuntu VPS, so that I can operate sing-box without maintaining a collection of application scripts.
2. As the sole administrator, I want installation to refuse an Existing deployment, so that existing manual or sing-box-yg installations are never silently damaged.
3. As the sole administrator, I want the interactive installer to select all five Managed protocols by default, so that a fresh deployment immediately has the intended protocol coverage.
4. As the sole administrator, I want to deselect a Managed protocol during setup, so that I do not expose an unnecessary listener.
5. As the sole administrator, I want every Enabled protocol to receive its own listener port and Proxy credential, so that a leaked credential has a narrow compromise scope.
6. As the sole administrator, I want a Subscription credential separate from every Proxy credential, so that possession of a node credential does not authorize subscription retrieval.
7. As the sole administrator, I want to configure a subscription host and optionally a distinct Proxy host, so that the public address of my subscription and proxy services can be separated when necessary.
8. As the sole administrator, I want Reality decoy SNI configured independently of the public hostnames, so that Reality camouflage settings cannot be confused with certificate identity.
9. As the sole administrator, I want sbctl to obtain and renew domain certificates through the distribution-maintained Certbot package, so that HTTPS does not require Caddy or a resident ACME process.
10. As the sole administrator, I want Direct subscription mode to serve HTTPS itself, so that the normal deployment contains only sing-box and sbctl as long-running application services.
11. As the sole administrator, I want an external reverse-proxy mode that binds sbctl only to loopback, so that an existing service may retain public 80/443.
12. As the sole administrator, I want an IP fallback subscription on a configurable high HTTP port when no usable domain exists, so that I can still import nodes without taking 80/443.
13. As the sole administrator, I want sing-box JSON, Clash/Mihomo YAML, and URI text subscription representations, so that I can use the clients I already have.
14. As the sole administrator, I want each subscription representation to originate from the same generated node set, so that formats never disagree about ports, credentials, hosts, or TLS settings.
15. As the sole administrator, I want each subscription response to include `subscription-userinfo`, so that compatible clients display consumed, allowed, and next-reset traffic information.
16. As the sole administrator, I want VPS traffic to include the selected network interface's received and transmitted bytes, so that the displayed number matches my VPS-level traffic allowance.
17. As the sole administrator, I want the installer to discover the default-route interface and allow an override, so that different cloud interface names work without hard-coding `eth0`.
18. As the sole administrator, I want Natural-month reset and Anchored-month reset policies, so that traffic can follow either a calendar month or my selected reset day and time.
19. As the sole administrator, I want accounting to recover correctly after host reboot, state recreation, short months, and service downtime, so that a counter reset cannot corrupt the displayed total.
20. As the sole administrator, I want the accounting timezone to default to the VPS system timezone and offer common named timezones, so that monthly boundaries match my expected schedule without changing host time.
21. As the sole administrator, I want `sbctl status`, `traffic`, `node`, `sub`, and `restart` to report current state without manual file inspection, so that normal operation is straightforward.
22. As the sole administrator, I want generated sing-box configuration validated before it replaces the running configuration, so that a failed edit leaves the previous proxy service usable.
23. As the sole administrator, I want configuration changes, subscription cache generation, and state updates to be atomic and serialized, so that concurrent CLI and daemon work cannot expose partial artifacts.
24. As the sole administrator, I want the public subscription daemon to run under a dedicated non-root account, so that a remote HTTP parsing flaw has limited host privileges.
25. As the sole administrator, I want install output to list required TCP and UDP ports without automatically modifying my firewall, so that sbctl does not interfere with unrelated host policy.
26. As the sole administrator, I want `sbctl update --check` to show available versions without changing the host, so that I control when upgrades occur.
27. As the sole administrator, I want an explicit update to verify artifacts, preserve a rollback point, validate services, and restore the old version on failure, so that upgrades are recoverable.
28. As the sole administrator, I want uninstall to preserve a root-readable backup by default and require `--purge` for destructive removal, so that credentials and traffic state are not accidentally lost.
29. As the sole administrator, I want subscription credentials absent from normal logs and never accepted in query parameters, so that URL leakage risk is minimized.
30. As the sole administrator, I want a clear error when a required port, domain validation condition, release artifact, or existing deployment prevents an operation, so that I can fix the prerequisite without guessing.

## Implementation Decisions

- sbctl targets stable Rust edition 2024. It uses a single-thread Tokio runtime for its low-volume I/O service, Axum/Hyper for HTTP routing, Rustls for TLS, Serde for TOML, JSON, YAML-adjacent data models, Clap for the CLI, Reqwest for release retrieval, and Tracing for structured redacted logging.
- The system has two long-running processes: sing-box is the data plane; sbctl daemon is the subscription, traffic, and cache-serving control-plane process. CLI administration and the daemon are subcommands of the same binary.
- V0.1 supports VLESS Reality, VMess WebSocket, Hysteria2, TUIC v5, and AnyTLS. Interactive installation enables all five by default but permits explicit deselection. Each Enabled protocol owns an independently chosen, available high listener port and a protocol-appropriate, cryptographically random Proxy credential.
- A single canonical node model is the source for sing-box server configuration, URI text, Clash/Mihomo YAML, and sing-box JSON. The Subscription credential is at least 256 bits from the OS CSPRNG, differs from all Proxy credentials, is rendered only as a path segment, and is redacted in logs and user-facing diagnostics.
- `subscription_host`, optional `proxy_host`, and Reality decoy SNI are separate configuration fields. The proxy host defaults to the subscription host. Reality decoy SNI never defaults from either public host field.
- Direct subscription mode is the default. sbctl owns public TCP 80 and 443, terminates TLS with Rustls, serves the ACME challenge webroot, and loads certificates issued or renewed by the Debian/Ubuntu Certbot package. Certbot is invoked only for certificate operations.
- External reverse-proxy mode binds sbctl to loopback and delegates public TLS, routing, and certificate ownership to an administrator-managed proxy. sbctl does not generate, overwrite, or take over a Caddy or Nginx global configuration.
- IP fallback subscription is plain HTTP on an administrator-selected high port. It is emitted only when no usable domain mode is configured and clearly marked as lower security. Publicly trusted short-lived IP certificates are not a default V0.1 path.
- Subscription routes are explicit by Subscription format. Responses authenticate only an exact path credential, use restrictive cache and logging behavior, set the correct content type, and add a dynamically derived `subscription-userinfo` header. No User-Agent format guessing or query-parameter credential bypass is supported.
- Node artifacts are regenerated after an accepted node or configuration change and atomically replace the previous cache. Subscription requests read a complete cached artifact and compute only the current traffic header, avoiding periodic copy jobs.
- VPS traffic is measured from one configured Linux network interface. Interface RX represents client upload and interface TX represents client download; total is their sum. The UI and headers call this VPS traffic and never imply per-protocol or per-user attribution.
- The traffic state records a schema version, accounting policy, current accounting period identity, accumulated RX/TX, latest interface counters, and current boot ID. A periodic daemon task and relevant CLI reads reconcile counter deltas. Boot-ID changes and decreasing counters retain prior accumulation before accepting the new counter epoch.
- Natural-month reset occurs on the first day of a calendar month at 00:00 in the selected accounting timezone. Anchored-month reset uses a configured first reset date plus day and time; when the selected day is absent, the reset is the last day of that month. `expire` is the next reset instant.
- The installer detects the default-route interface, persists it after administrator confirmation, and permits an explicit configuration override. It displays required firewall openings but changes neither UFW nor nftables unless a future explicit opt-in operation is introduced.
- Configuration, generated artifacts, and state commits use an exclusive operation lock; write to a same-filesystem temporary file; set restrictive ownership and permissions; fsync file and directory where supported; then rename atomically. A configuration operation runs `sing-box check` before replacing the active configuration and reloads/restarts only after commit. A failed validation or health check restores the prior known-good version.
- The daemon runs as a dedicated non-root system account. Administrative subcommands that install packages, manage systemd, bind privileged ports, read protected certificates, or replace sing-box configuration require explicit privilege escalation. The systemd unit applies least-privilege hardening compatible with certificate access and required writable state.
- V0.1 supports Debian and Ubuntu on amd64 first, with arm64 as the next supported release artifact. It assumes systemd. It does not claim Alpine compatibility.
- Bootstrap scripts are limited to OS prerequisite installation, verified sbctl retrieval, and invoking sbctl installation. sbctl retrieves sing-box only from a pinned release manifest, verifies expected hashes and available signatures, and never silently follows `main` or an unpinned latest artifact.
- `update --check` is read-only. An explicit update downloads verified artifacts, keeps the prior binary/configuration/state rollback point, validates the candidate configuration and service health, then commits; no daily cron, timer-driven software upgrade, or unconditional service restart is allowed.
- Installation detects an Existing deployment and exits without modifying it. V0.1 has no automatic import or migration workflow.
- Default uninstall removes sbctl-owned services and binaries after producing a root-readable backup of configuration, credentials, and state. `uninstall --purge` is the only operation that deletes sbctl-owned persistent data. Neither uninstall form touches another application's services, proxy configuration, or firewall rules.

## Testing Decisions

- The primary and highest seam is black-box CLI acceptance in isolated Debian and Ubuntu environments. Tests invoke sbctl commands as an administrator, inspect only externally observable results, and exercise the produced systemd services and subscription HTTP endpoint rather than private Rust implementation details.
- A successful end-to-end test installs a fresh deployment, accepts all default Managed protocols, validates generated sing-box configuration, starts services, retrieves all three Subscription formats through the expected credential path, and verifies that each representation corresponds to the same public nodes.
- End-to-end tests cover direct domain mode with a locally controlled ACME-compatible test endpoint or equivalent certificate fixture, external reverse-proxy loopback mode, and the IP fallback subscription mode. They verify port ownership and that credentials in query strings are rejected.
- Acceptance tests cover an Existing deployment discovery failure, occupied listener port, unavailable interface, invalid host/certificate prerequisites, malformed configuration, failed sing-box validation, service restart failure, and failed update. Each must leave the preceding known-good deployment usable.
- Traffic behavior tests use controlled sysfs-reader seams or equivalent Linux fixture interfaces to verify RX/TX mapping, first observation, normal delta accumulation, boot-ID change, decreasing counters, missing state, restart recovery, Natural-month reset, Anchored-month reset, and short-month month-end handling. They assert external `traffic`, `status`, and `subscription-userinfo` values.
- Format contract tests parse the generated JSON, YAML, and URI text with representative client-compatible parsers and verify all fields originate from the same canonical node input. They test every enabled Managed protocol and credential separation.
- Security-oriented tests verify path-only credential acceptance, token redaction in logs and errors, restrictive persistent-file permissions, daemon non-root identity, atomic cache replacement under concurrent requests, and no unrequested firewall or unrelated configuration mutation.
- Update and uninstall tests verify artifact validation before replacement, rollback after induced failure, read-only update check behavior, default backup preservation, and `--purge` scope limited to sbctl-owned data.
- There is no existing application test suite. This spec establishes the CLI end-to-end seam as the required baseline; pure domain logic may additionally have focused tests, but they complement rather than replace that seam.

## Out of Scope

- Multiple administrators, subscriber accounts, per-user quotas, per-user traffic attribution, or enforcement that disconnects traffic at the monthly limit.
- Automatic migration or takeover of sing-box-yg, vps-sub-meter, manually managed sing-box, Caddy, Nginx, firewall, or other host services.
- Automatic background software updates, daily forced restarts, unpinned remote scripts, and automatic firewall modification.
- VMess CDN/Argo/Cloudflare tunnel automation, custom CDN flows, and general Web hosting.
- Native ACME protocol implementation, a permanent ACME daemon, and IP-address HTTPS as the normal no-domain workflow.
- Alpine, non-systemd Linux, Windows, macOS, containers, QR-code UI, databases, web administration panels, User-Agent subscription-format guessing, and query-string subscription authentication.
- Additional protocols or advanced sing-box runtime API control beyond the five Managed protocols.

## Further Notes

- This specification supersedes older exploratory statements in `singbox-sub-plan.md` that propose Caddy, a two-protocol V0.1, or a C++ control plane.
- It implements the boundaries established by ADR-0001 through ADR-0005: direct delivery with fallback, separated credentials and endpoints, verified/reversible lifecycle, refusal to take over existing deployments, and a Rust control plane.
- The local issue tracker convention places later implementation tickets in `.scratch/sbctl-v0.1/issues/` and uses the `ready-for-agent` status vocabulary.
