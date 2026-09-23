#!/bin/bash
# Validate the generated clash artifacts against the pinned real mihomo core,
# after the sniffer change, and re-run the whole Linux gate.
set -u
scratch=/mnt/c/Users/ranly/Documents/singbox-sub-me
out=$scratch/.scratch/wsl-clash-gate.txt
: >"$out"
. "$HOME/.cargo/env" 2>/dev/null

rsync -a --delete --exclude target --exclude .git --exclude node_modules \
  "$scratch/" "$HOME/src/singbox-sub-me/"
echo "rsync_exit=$?" >>"$out"

cd "$HOME/src/singbox-sub-me" || exit 1
export MIHOMO_BIN="$HOME/bin/mihomo"
"$MIHOMO_BIN" -v >>"$out" 2>&1
cargo test --test clash_mihomo -- --ignored --nocapture >/tmp/w-mihomo.log 2>&1
echo "mihomo_exit=$?" >>"$out"
grep -E 'test result:|accepted|panicked|not find' /tmp/w-mihomo.log >>"$out"

cargo test --workspace --features sbctl/test-signing >/tmp/w-test3.log 2>&1
echo "workspace_test_exit=$?" >>"$out"
grep -cE '^test .* ok$' /tmp/w-test3.log | sed 's/^/linux_passed=/' >>"$out"
grep -E 'test result: FAILED|panicked at' /tmp/w-test3.log | head -10 >>"$out"
echo done >>"$out"
