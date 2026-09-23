#!/bin/bash
# Ground truth for the orphan-guard test: does a backgrounded child survive its
# parent here, and does it survive the parent being SIGKILLed?
set -u

report() {
  if [ -d "/proc/$1" ]; then echo "$2: pid $1 ALIVE (cmdline: $(tr '\0' ' ' < /proc/$1/cmdline 2>/dev/null))"; else echo "$2: pid $1 gone"; fi
}

echo "--- case A: parent exits normally ---"
pid=$(bash -c 'sleep 120 & echo $!')
report "$pid" "right after parent exited"
sleep 1
report "$pid" "1s later"
kill -9 "$pid" 2>/dev/null

echo "--- case B: parent is SIGKILLed ---"
bash -c 'sleep 120 & echo $! > /tmp/orphanB.pid; exec sleep 300' &
parent=$!
sleep 1
pid=$(cat /tmp/orphanB.pid)
report "$pid" "child while parent alive"
echo "child PPid: $(awk '/^PPid:/{print $2}' /proc/$pid/status 2>/dev/null)"
kill -9 "$parent"
sleep 2
report "$pid" "child after parent SIGKILL"
kill -9 "$pid" 2>/dev/null

echo "--- case C: explicit prctl(PR_SET_PDEATHSIG) ---"
if command -v python3 >/dev/null; then
python3 - <<'PY'
import ctypes, os, subprocess, time, signal
libc = ctypes.CDLL("libc.so.6", use_errno=True)
def spawn():
    pid = os.fork()
    if pid == 0:
        libc.prctl(36, signal.SIGTERM, 0, 0, 0)  # PR_SET_PDEATHSIG
        os.execvp("sleep", ["sleep", "120"])
    return pid
p = spawn()
time.sleep(0.5)
print("  with prctl: child", p, "alive:", os.path.exists(f"/proc/{p}"))
os.kill(p, signal.SIGKILL)
PY
fi
