# Acceptance fixture boundary

`verify.sh` is a black-box acceptance flow: it invokes only the administrator-visible
`sbctl` CLI and the subscription HTTP endpoint. `fixture.sh` supplies isolated host
state under `$work/<name>` for `/etc`, `/proc`, `/sys`, systemd command shims and
certificates. A fixture command is rooted below `usr/bin` and never falls back to the
container's real command.

The shell flow is suitable for local development and CI smoke checks. Production
support is established only by `run.sh` in a Debian/Ubuntu VM or equivalent real
systemd environment. WSL2 is a Development host for compilation, Rust tests and
simulated CLI checks; it is not evidence for the Production host release gate.

签名测试现在需要显式 `test-signing` 构建，通过 `SBCTL_TEST_ARTIFACT` 传入；生产工件仍通过 `SBCTL_ARTIFACT` 传入。两者不得混用，详见 [签名与验收隔离](../../docs/release-signing.md)。
