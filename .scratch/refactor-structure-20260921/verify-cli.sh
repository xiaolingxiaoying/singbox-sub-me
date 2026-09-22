#!/usr/bin/env bash
# Rebuild in Linux, re-capture every --help, compare against the stored
# baseline, then run the test suites. GUI is excluded (verified in the VM).
set -uo pipefail
SRC=/mnt/c/Users/ranly/Documents/ChatGPT/singbox-sub-me
DST=$HOME/sbctl-refactor
export CARGO_TARGET_DIR=$HOME/refactor-target
B=$CARGO_TARGET_DIR/debug/sbctl

cd "$SRC"
tar --exclude=./target --exclude='./target-*' --exclude=./.git --exclude=./.qoder \
    --exclude=./.kilo --exclude=./node_modules --exclude=./dist \
    --exclude=./.tmp-sing-box-yg-research --exclude=./.reference-* \
    --exclude=./sbctl-linux-amd64 --exclude=./.scratch \
    -cf - . | tar -xf - -C "$DST"

cd "$DST" || exit 1
cargo build --features test-signing --bin sbctl 2>&1 | tail -3 || exit 1

rm -rf /tmp/help-after
mkdir -p /tmp/help-after
$B --help > /tmp/help-after/00-top.txt 2>&1
for s in install menu status traffic node restart uninstall update sing-box sub \
         qr credential serve certificate config; do
  $B "$s" --help > "/tmp/help-after/$s.txt" 2>&1
done
for s in "config init" "config show" "config validate" "traffic report" \
         "certificate status" "sub list" "config override show"; do
  n=$(echo "$s" | tr ' ' '-')
  $B $s --help > "/tmp/help-after/nested-$n.txt" 2>&1
done

echo "=== help file count: before=$(ls /tmp/help-before | wc -l) after=$(ls /tmp/help-after | wc -l) ==="
if diff -r /tmp/help-before /tmp/help-after > /tmp/help.diff 2>&1; then
  echo "HELP: ALL IDENTICAL"
else
  echo "HELP: DIFFERS"
  head -40 /tmp/help.diff
fi

echo "=== unknown-flag behaviour (flatten can change error text) ==="
$B install --mode bogus 2>&1 | head -3
$B install --nope 2>&1 | head -3

echo "=== tests ==="
cargo test --workspace --exclude sbgui --features sbctl/test-signing 2>&1 \
  | grep -E 'test result|^error|FAILED'
