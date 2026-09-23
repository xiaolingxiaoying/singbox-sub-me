#!/bin/bash
# L2 (WSL) leg: rsync the Windows tree into ext4, run the gate, and validate the
# clash artifacts against a real mihomo core. Every step reports its own exit
# code into a plain-text summary the Windows side reads.
set -u
scratch=/mnt/c/Users/ranly/Documents/singbox-sub-me
out=$scratch/.scratch/wsl-l2-summary.txt
: >"$out"
. "$HOME/.cargo/env" 2>/dev/null

rsync -a --delete --exclude target --exclude target-linux --exclude .git \
  --exclude node_modules "$scratch/" "$HOME/src/singbox-sub-me/"
echo "rsync_exit=$?" >>"$out"

cd "$HOME/src/singbox-sub-me" || exit 1

cargo fmt --all --check >/tmp/w-fmt.log 2>&1
echo "fmt_exit=$?" >>"$out"
grep -c 'Diff in' /tmp/w-fmt.log | sed 's/^/fmt_diffs=/' >>"$out"

cargo clippy --workspace --all-targets --features sbctl/test-signing -- -D warnings \
  >/tmp/w-clippy.log 2>&1
echo "clippy_exit=$?" >>"$out"
grep -c '^error' /tmp/w-clippy.log | sed 's/^/clippy_errors=/' >>"$out"

cargo test --workspace --features sbctl/test-signing >/tmp/w-test.log 2>&1
echo "test_exit=$?" >>"$out"
grep 'test result:' /tmp/w-test.log >>"$out"

echo "--- mihomo ---" >>"$out"
export MIHOMO_BIN="$HOME/bin/mihomo"
"$MIHOMO_BIN" -v >>"$out" 2>&1
cargo test --test clash_mihomo -- --ignored --nocapture >/tmp/w-mihomo.log 2>&1
echo "mihomo_exit=$?" >>"$out"
grep -E 'test result:|accepted|panicked' /tmp/w-mihomo.log >>"$out"
echo done >>"$out"
