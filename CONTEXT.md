# sbctl

sbctl is a single-administrator tool for operating sing-box and providing a private subscription from one VPS.

## Language

**Subscription credential**:
A high-entropy secret embedded in the sole administrator's subscription URL. It authorizes retrieval of that administrator's generated subscription; it is not a user account.
_Avoid_: User token, API key, account

**Subscription credential rotation**:
The security operation that replaces the current Subscription credential with a newly generated one, immediately invalidating URLs that contain the previous credential. It does not change the Subscription host or any Proxy credential.
_Avoid_: Domain change, password change, UUID change

**VPS traffic**:
The total inbound and outbound network bytes measured for the VPS during a monthly accounting period. It is not traffic attributable to a protocol, node, or individual user.
_Avoid_: Per-user traffic, proxy traffic

**Monthly traffic limit**:
The administrator-configured VPS traffic allowance for one monthly accounting period. In the first release it is reported in subscription metadata and status, rather than enforcing a connection cutoff.
_Avoid_: Per-user quota, bandwidth limit

**Accounting period**:
The monthly interval over which VPS traffic is accumulated and compared with the monthly traffic limit.
_Avoid_: Billing cycle

**Natural-month reset**:
An accounting-period policy that begins a new period on the first day of every calendar month at 00:00 in the selected accounting timezone.
_Avoid_: Rolling month, anchored reset

**Anchored-month reset**:
An accounting-period policy that begins its first period at a configured date and time, then begins a new period at the same day and time each month in the selected accounting timezone. If that day does not exist in a short month, the reset occurs on that month's last day.
_Avoid_: Natural-month reset, rolling month

**Accounting timezone**:
The named IANA timezone used to interpret accounting-period reset dates and times. It belongs to the sbctl deployment and does not change the VPS operating-system timezone.
_Avoid_: System-timezone accounting, browser-local time

**Client display timezone**:
The named IANA timezone selected for presenting the next accounting reset to the administrator or subscription consumer. It may differ from the Accounting timezone, but it represents the same reset instant after timezone conversion.
_Avoid_: Accounting timezone, VPS timezone

**VPS refresh timezone**:
The administrator-facing name for the Accounting timezone when distinguishing it from the Client display timezone. It determines when the VPS accounting period actually refreshes.
_Avoid_: Client display timezone, operating-system timezone

**Default timezone pair**:
The onboarding defaults of America/Los_Angeles for the VPS refresh timezone and Asia/Shanghai for the Client display timezone. The pair describes one shared reset instant in two local representations and is used only when the administrator has not chosen other timezones.
_Avoid_: System timezone pair, client device timezone

**First reset instant**:
The administrator-selected date and local time at which an Anchored-month reset schedule first becomes active. Before this instant, the deployment has no active anchored accounting period.
_Avoid_: Installation time, billing start

**Accounting reset**:
The transition from one Accounting period to the next, including establishing a new network-counter baseline. It is independent from rebuilding or serving a Subscription format.
_Avoid_: Subscription refresh, traffic deletion

**Pending first reset**:
The valid pre-period state of an Anchored-month reset before its First reset instant. It reports zero VPS traffic for the not-yet-started period and exposes the first reset instant as the next reset.
_Avoid_: Broken accounting, missing period

**Traffic correction**:
An administrator-authored adjustment to the current Accounting period's reported VPS traffic when the measured amount is known to be wrong. A total-only correction does not invent RX/TX direction values.
_Avoid_: Data-plane limit, per-user traffic

**Total traffic adjustment**:
A correction applied only to the reported total VPS traffic without changing the measured RX or TX direction values. It is distinct from a direction-aware Traffic correction.
_Avoid_: Fake RX/TX, bandwidth limit

**Accounting state writer**:
One of the explicitly authorized operations that may persist accounting state: the periodic Accounting reset task or an administrator's Traffic correction command.
_Avoid_: Subscription request, status read

**Subscription format**:
One of the generated client-consumable representations of the same node set: sing-box JSON, Clash/Mihomo YAML, or URI text for Shadowrocket-compatible clients. The first release keeps all three formats and must cover the sing-box JSON and Clash Meta YAML compatibility expected by `vps-sub-meter`.
_Avoid_: Subscription protocol, node configuration

**Configuration topic**:
A coherent administrator concern that can be reviewed and changed independently, such as subscription delivery, protocol listeners, or traffic accounting. It is narrower than the complete deployment configuration and does not imply a separate deployment.
_Avoid_: Single setting, wizard step

**Traffic input unit**:
The human-facing unit used when an administrator enters a traffic amount. sbctl interprets the default unit with the same binary conversion as `vps-sub-meter`—one GiB equals 1024³ bytes—while persisted accounting values remain exact byte counts.
_Avoid_: Raw byte input, decimal GB

**Public fallback port**:
The high TCP port exposed by an IP fallback subscription when no domain is available. It is public-facing and is distinct from the loopback listener used behind an external reverse proxy.
_Avoid_: Backend port, protocol listener port

**Proxy subscription listener**:
The loopback HTTP listener used by an external reverse proxy to forward subscription requests to sbctl. It is not a public fallback port and does not belong to a Managed protocol.
_Avoid_: Public fallback port, protocol listener port

**Managed protocol**:
A proxy protocol whose server configuration, share representation, and subscription representation are owned by sbctl. The first release manages VLESS Reality, VMess WebSocket, Hysteria2, TUIC v5, and AnyTLS.
_Avoid_: Node type, transport

**Enabled protocol**:
A managed protocol selected for a particular VPS deployment. Each enabled protocol receives its own generated server configuration and listener port.
_Avoid_: Installed protocol, default node

**Direct subscription mode**:
A deployment mode in which sbctl owns public TCP ports 80 and 443 to serve the subscription endpoint and complete ACME validation. It is the default deployment mode.
_Avoid_: Embedded proxy mode, shared-port mode

**IP fallback subscription**:
An explicitly lower-security HTTP subscription URL served from the VPS public IP on a configured high port when no usable domain is available. It does not occupy TCP ports 80 or 443.
_Avoid_: IP HTTPS, domain subscription

**Proxy credential**:
A randomly generated protocol-specific secret that authenticates one generated proxy node. It is independent for every enabled protocol and is never used to authorize subscription retrieval.
_Avoid_: Subscription token, shared UUID

**Proxy host**:
The public IP address or hostname that a generated proxy client connects to. It defaults to the subscription host but may be configured separately.
_Avoid_: Reality SNI, subscription host

**Reality decoy SNI**:
The externally plausible server name used by VLESS Reality's camouflage handshake. It is neither the subscription host nor necessarily the proxy host.
_Avoid_: Proxy hostname, certificate hostname

**Protocol listener port**:
The administrator-selected or automatically allocated public port owned by one Enabled protocol. It is independent for every protocol, belongs to `10000–65535`, and is reserved across both TCP and UDP so two protocols cannot claim the same port number.
_Avoid_: Subscription port, shared port

**Upstream compatibility**:
Compatibility with the protocol configuration and client-consumable fields documented or generated by the selected upstream projects. It does not mean importing their files, sharing their credentials, or taking over their running deployment.
_Avoid_: Script compatibility, migration

**Existing deployment**:
Any sing-box binary, service, or configuration discovered before sbctl is installed. sbctl never silently replaces or adopts an existing deployment.
_Avoid_: Managed deployment, migration

**Development host**:
The WSL2 Ubuntu 22.04 environment used for compilation, unit tests, and simulated CLI checks. It is not evidence that a production systemd VPS installation is supported.
_Avoid_: Production host, deployment host

**Production host**:
A supported Debian or Ubuntu machine with a real systemd boot/runtime environment where service lifecycle, privileged port ownership, networking, and certificate automation are verified.
_Avoid_: WSL2 host, test fixture

**Direct HTTPS ownership**:
The deployment boundary in which sbctl owns public TCP ports 80 and 443, using a non-root service with only the narrowly required privileged-port capability or systemd socket activation.
_Avoid_: Root HTTPS daemon, shared public port

**Authenticated release manifest**:
A fixed-version manifest whose publisher authenticity is verified by a release signature before its artifact hashes and URLs are trusted. Its canonical signed payload and schema are part of the release contract.
_Avoid_: Unsigned manifest, latest manifest

**Socket-activated HTTPS service**:
A Direct HTTPS service whose TCP 80 and 443 listeners are opened and owned by systemd, then passed to a non-root sbctl process.
_Avoid_: Root-bound HTTPS service, capability-first binding

**Release gate**:
A condition that must pass before a version may be published, including real systemd installation, non-root service startup, authenticated manifest verification, and failure recovery.
_Avoid_: Best-effort test, unit-test-only release

**Subscription request failure**:
An externally observable response for an invalid route or credential that reveals no authorization detail, while internal storage, traffic, or certificate failures are separately diagnosed through redacted service logs.
_Avoid_: Credential error leak, silent 404 for every failure
