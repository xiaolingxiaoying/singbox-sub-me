#!/usr/bin/env bash
# L3: build the two Linux artifacts in WSL (ext4), then run the Docker acceptance
# matrix from Git Bash, where the Docker CLI actually lives.
#
# Two different binaries on purpose: the production build's trust anchor rejects
# the development key, so the fixture/bootstrap suites need the test-signing
# build and the accepted artifact must not be it.
set -uo pipefail

root=/c/Users/ranly/Documents/singbox-sub-me
out=$root/.scratch/l3.log
: >"$out"

say() { printf '%s\n' "$*" | tee -a "$out" >/dev/null; }

cd "$root" || exit 1

if ! command -v docker >/dev/null 2>&1; then
  say "FAIL: docker is not on the Git Bash PATH (is Docker Desktop running?)"
  exit 3
fi
say "docker: $(docker --version 2>&1)"

say "== building the production Linux artifact in WSL (release, no test-signing)"
wsl -d Ubuntu-22.04 -- bash -lc \
  '. "$HOME/.cargo/env"; cd ~/src/singbox-sub-me && cargo build --release --target-dir /mnt/c/Users/ranly/Documents/singbox-sub-me/target-linux' \
  >>"$out" 2>&1
say "build_prod_exit=$?"

say "== building the sbtui client artifact in WSL (the acceptance image now needs it)"
wsl -d Ubuntu-22.04 -- bash -lc \
  '. "$HOME/.cargo/env"; cd ~/src/singbox-sub-me && cargo build --release -p sbtui --target-dir /mnt/c/Users/ranly/Documents/singbox-sub-me/target-linux' \
  >>"$out" 2>&1
say "build_sbtui_exit=$?"

say "== rsync (source may have moved) then building the test-signing fixture binary"
wsl -d Ubuntu-22.04 -- bash -lc \
  'rsync -a --delete --exclude target --exclude target-* --exclude .git --exclude node_modules /mnt/c/Users/ranly/Documents/singbox-sub-me/ ~/src/singbox-sub-me/ && . "$HOME/.cargo/env" && cd ~/src/singbox-sub-me && cargo build --release --features sbctl/test-signing --target-dir /mnt/c/Users/ranly/Documents/singbox-sub-me/target-fixtures' \
  >>"$out" 2>&1
say "build_fixture_exit=$?"

for pair in "target-linux/release/sbctl" "target-linux/release/sbtui" "target-fixtures/release/sbctl"; do
  if [ -f "$root/$pair" ]; then
    say "artifact ok: $pair ($(file "$root/$pair" 2>/dev/null | cut -c1-90 || echo unknown))"
  else
    say "FAIL: missing artifact $pair"
  fi
done

say "== running tests/acceptance/run.sh (debian:12-slim, ubuntu:22.04, ubuntu:24.04)"
SBCTL_ARTIFACT=./target-linux/release/sbctl \
  SBCTL_TEST_ARTIFACT=./target-fixtures/release/sbctl \
  SBCTUI_ARTIFACT=./target-linux/release/sbtui \
  sh tests/acceptance/run.sh >>"$out" 2>&1
say "run_sh_exit=$?"

say "== last lines of the run =="
cp "$out" "$out.snapshot"
grep -nE "FAIL|Error|error|not ok|exit code|ACCEPTANCE|verification" "$out.snapshot" | tail -40 >>"$out"
say "L3 complete"
