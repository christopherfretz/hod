#!/usr/bin/env bash
# One pool of parallelism tokens for every hod checkout in a container: a
# GNU make jobserver (make 4.4's fifo style). `scripts/env.sh` points cargo
# and rustc at it through MAKEFLAGS while the fifo exists, so concurrent
# builds in several worktrees queue for tokens instead of each running its
# own -j. Ported from kiln's helper, which took several iterations to get
# right; the comments record what those iterations taught.
#
#   scripts/jobserver.sh start      create the pool (idempotent) and the refill loop
#   scripts/jobserver.sh status     idle tokens, pool size, refill loop
#   scripts/jobserver.sh refill     one refill pass (what the loop runs every 60 s)
#   scripts/jobserver.sh resize N   change the pool size, live
#   scripts/jobserver.sh stop       remove the pool
#
# `refill`, `resize` and `stop` refuse the machine's shared pool (the
# default fifo) unless the caller confirms with `--shared` (or
# HOD_JOBSERVER_SHARED=1): a test or a check meant for a private pool, run
# with HOD_JOBSERVER_FIFO lost on the way (an `export` in a chain sent to
# the background), once stopped the shared pool under every build of the
# machine. The refill loop the script starts needs no confirmation. Name
# a private pool inline: `HOD_JOBSERVER_FIFO=<path> scripts/jobserver.sh ...`.
#
# Run `start` when the container starts, BEFORE anything sources
# scripts/env.sh: env.sh exports MAKEFLAGS only if the fifo exists at that
# moment, and otherwise falls back to a per-cargo job cap (with one line on
# standard error).
#
# - The fifo lives on the container's tmpfs ($HOD_JOBSERVER_FIFO, default
#   /run/hod/jobserver.fifo), never on a workspace bind mount: fifos are
#   unreliable there. It vanishes with the container, so a stale fifo
#   without tokens cannot outlive a restart.
# - The holder process keeps a read-write end open, so writers never block
#   and the tokens persist (a fifo forgets its bytes when nobody has it
#   open).
# - Tokens get lost: a cargo or rustc killed while holding some (a timeout,
#   a cancelled command, the OOM killer) never writes them back, and any
#   blocking shell read of the fifo eats tokens. A pool drains to nothing
#   within hours and everything runs at -j1 with no error. Hence the refill
#   loop. Look into the fifo only with a non-blocking read that writes back
#   what it read (`idle_tokens`).
# - A refill must never under-count the tokens in use, or it mints tokens:
#   a pool has reached five times its size that way and run a machine out
#   of memory. So: clients are found by who has the fifo open (/proc), not
#   by name; compilers and the children of clients count as holders; a
#   deficit is refilled only if two consecutive passes saw it; idle tokens
#   above the pool size are drained. Over-counting holders is safe. A pass
#   looks in one process (`measure`), and counts the idle tokens before
#   and after it looks for clients: a pass whose shell scan of /proc took
#   seconds once saw the tokens of a client that ended meanwhile as
#   neither idle nor held, and minted them (hod WP-065). With no client
#   alive every token must be idle: a deficit then is a loss, refilled on
#   the second pass that sees it.
# - Tokens bound the number of compilers, not memory, and each cargo has
#   one implicit job of its own: N concurrent cargos run pool + N rustc.
#   Size the pool for the container's memory ($HOD_JOBS, default 12).
# - libtest is not a jobserver client: `cargo test` runs its threads
#   whatever the pool says. env.sh caps RUST_TEST_THREADS; the harness is
#   a client of its own (spec/harness.md "Jobserver").
# - ninja ignores the jobserver; a make in the chain must be GNU make 4.4
#   or later for the fifo style.
# - The refill log is in the build root (it survives restarts). "drained
#   N" with N of 2 or more means tokens were minted; a single-token drain
#   is noise.
set -euo pipefail

SELF="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)/$(basename "${BASH_SOURCE[0]}")"
_WS="$(cd "$(dirname "$SELF")/.." && pwd)"
_MAIN="$(cd "$(git -C "$_WS" rev-parse --path-format=absolute --git-common-dir 2>/dev/null || echo "$_WS/.git")/.." && pwd)"
case "$_MAIN" in */.git/modules*) _MAIN="${_MAIN%%/.git/modules*}";; esac

# The machine's pool. HOD_JOBSERVER_SHARED_FIFO moves it, for the tests of
# the guard below only.
SHARED_FIFO="${HOD_JOBSERVER_SHARED_FIFO:-/run/hod/jobserver.fifo}"
FIFO="${HOD_JOBSERVER_FIFO:-$SHARED_FIFO}"
PIDFILE="${FIFO%.fifo}.pid"
JOBSFILE="${FIFO%.fifo}.jobs"
JOBS="${HOD_JOBS:-$(cat "$JOBSFILE" 2>/dev/null || echo 12)}"

REFILL_PIDFILE="${FIFO%.fifo}.refill.pid"
REFILL_LOG="${HOD_BUILD_ROOT:-$_MAIN/.build}/jobserver-refill.log"

alive() { [ -f "$PIDFILE" ] && kill -0 "$(cat "$PIDFILE")" 2>/dev/null && [ -p "$FIFO" ]; }
refill_alive() { [ -f "$REFILL_PIDFILE" ] && kill -0 "$(cat "$REFILL_PIDFILE")" 2>/dev/null; }

# The idle tokens: taken out without blocking, counted, written back.
idle_tokens() {
  python3 - "$FIFO" <<'PY'
import os, sys
fd = os.open(sys.argv[1], os.O_RDWR | os.O_NONBLOCK); got = b""
try:
    while True:
        b = os.read(fd, 64)
        if not b: break
        got += b
except BlockingIOError: pass
os.write(fd, got); print(len(got))
PY
}

# One look at the pool, in one process and in a few milliseconds (a scan by
# a shell, one readlink per descriptor on the machine, took seconds under
# load, and every token that moved in that time was a chance to
# under-count): "<idle> <held> <rustc> <clients>".
# - idle: the idle tokens, counted before and after the scan; the larger
#   count. A client that ends during the scan has its tokens counted by
#   the second count, one that starts takes them after the first.
# - clients: the processes with the fifo open, except the holder: by
#   descriptor, not by name (a test binary is `name-<hash>`). A process
#   whose descriptors cannot be read counts as a client (over-counting
#   holders is safe); one that is gone does not.
# - held: an upper bound on the tokens in use: compilers by name, and every
#   child of a client (a harness holds one token per child process); 0
#   when no client is alive.
measure() {
  python3 - "$FIFO" "$(cat "$PIDFILE" 2>/dev/null || echo 0)" <<'PY'
import errno, os, sys
fifo, holder = sys.argv[1], sys.argv[2]
me = str(os.getpid())

def idle():
    fd = os.open(fifo, os.O_RDWR | os.O_NONBLOCK); got = b""
    try:
        while True:
            b = os.read(fd, 64)
            if not b: break
            got += b
    except BlockingIOError: pass
    os.write(fd, got); os.close(fd)
    return len(got)

GONE = (errno.ENOENT, errno.ESRCH)
before = idle()
comm, par, clients = {}, {}, set()
for pid in os.listdir("/proc"):
    if not pid.isdigit() or pid in (me, holder): continue
    try:
        with open(f"/proc/{pid}/stat") as f: st = f.read()
        r = st.rindex(")")
        comm[pid], par[pid] = st[st.index("(") + 1:r], st[r + 2:].split()[1]
    except (OSError, ValueError, IndexError):
        continue  # it ended
    try:
        fds = os.listdir(f"/proc/{pid}/fd")
    except OSError as e:
        if e.errno not in GONE: clients.add(pid)
        continue
    for fd in fds:
        try:
            if os.readlink(f"/proc/{pid}/fd/{fd}") == fifo:
                clients.add(pid); break
        except OSError:
            pass  # closed meanwhile
after = idle()
compilers = {"clang", "clang++", "cc1", "cc1plus", "clippy-driver", "rustdoc"}
held = rustc = 0
for pid, c in comm.items():
    if c == "rustc":
        rustc += 1; held += 2; continue
    if c in compilers or (c.startswith("clang-") and c[6:].isdigit()): held += 1
    if par[pid] in clients: held += 1
print(max(before, after), held if clients else 0, rustc, len(clients))
PY
}

refill_once() {
  local idle held rustc clients want add
  read -r idle held rustc clients <<<"$(measure)"
  local seen="idle $idle, held $held, rustc $rustc, clients $clients, pool $JOBS"
  if [ "$idle" -gt "$JOBS" ]; then
    python3 - "$FIFO" "$(( idle - JOBS ))" <<'PY'
import os, sys
fd = os.open(sys.argv[1], os.O_RDWR | os.O_NONBLOCK); n = int(sys.argv[2]); got = 0
try:
    while got < n: got += len(os.read(fd, n - got))
except BlockingIOError: pass
PY
    echo "[jobserver] $(date -u +%FT%TZ) drained $(( idle - JOBS )) excess tokens ($seen)"
    return
  fi
  want=$(( JOBS - held ))
  add=$(( want - idle ))
  # A deficit is refilled only if two consecutive passes saw it, and by the
  # smaller of the two: a single pass can still under-count (a token that
  # moves twice while it looks), with or without clients.
  local prev=0 deficit_file="${FIFO%.fifo}.deficit"
  prev=$(cat "$deficit_file" 2>/dev/null || echo 0)
  echo "$(( add > 0 ? add : 0 ))" > "$deficit_file"
  if [ "$add" -gt 0 ]; then
    [ "$prev" -lt "$add" ] && add=$prev
    [ "$add" -gt 0 ] || { echo "[jobserver] $(date -u +%FT%TZ) deficit pending ($seen)"; return; }
  fi
  if [ "$add" -gt 0 ]; then
    python3 - "$FIFO" "$add" <<'PY'
import os, sys
fd = os.open(sys.argv[1], os.O_RDWR | os.O_NONBLOCK); os.write(fd, b"+" * int(sys.argv[2]))
PY
    echo "[jobserver] $(date -u +%FT%TZ) refilled $add tokens ($seen)"
  else
    echo "[jobserver] $(date -u +%FT%TZ) no refill ($seen)"
  fi
}

start_refill_loop() {
  if refill_alive; then return; fi
  mkdir -p "$(dirname "$REFILL_LOG")"
  nohup bash -c "while sleep 60; do '$SELF' refill >> '$REFILL_LOG' 2>&1; done" \
    >/dev/null 2>&1 &
  echo $! > "$REFILL_PIDFILE"
}

VERB="${1:-start}"; [ $# -gt 0 ] && shift
SHARED_OK="${HOD_JOBSERVER_SHARED:-}"
ARGS=()
for a in "$@"; do
  if [ "$a" = --shared ]; then SHARED_OK=1; else ARGS+=("$a"); fi
done
set -- "${ARGS[@]+"${ARGS[@]}"}"

# The refill loop of this pool: the process that runs every pass.
from_refill_loop() {
  [ -f "$REFILL_PIDFILE" ] && [ "$(cat "$REFILL_PIDFILE" 2>/dev/null)" = "$PPID" ]
}

# `refill`, `resize` and `stop` act on the shared pool only when told so.
guard_shared() {
  [ "$(realpath -m -- "$FIFO")" = "$(realpath -m -- "$SHARED_FIFO")" ] || return 0
  [ "$SHARED_OK" = 1 ] && return 0
  from_refill_loop && return 0
  echo "[jobserver] refusing '$VERB' on the shared pool $FIFO: every build of the machine" \
    "draws on it. Pass --shared (or HOD_JOBSERVER_SHARED=1) if that is meant; for a private" \
    "pool name it on the command: HOD_JOBSERVER_FIFO=<path> $0 $VERB" >&2
  exit 3
}

case "$VERB" in refill|resize|stop) guard_shared;; esac

case "$VERB" in
  start)
    if alive; then
      start_refill_loop
      echo "[jobserver] running: $FIFO (pid $(cat "$PIDFILE"))"; exit 0
    fi
    mkdir -p "$(dirname "$FIFO")"
    rm -f "$FIFO" "$PIDFILE"
    mkfifo -m 0666 "$FIFO"
    nohup bash -c "exec 3<>'$FIFO'; printf '%.0s+' \$(seq $JOBS) >&3; exec sleep infinity" \
      >/dev/null 2>&1 &
    echo $! > "$PIDFILE"
    echo "$JOBS" > "$JOBSFILE"
    sleep 0.2
    alive || { echo "[jobserver] failed to start"; exit 1; }
    start_refill_loop
    echo "[jobserver] started: $FIFO with $JOBS tokens (pid $(cat "$PIDFILE"))"
    ;;
  status)
    alive || { echo "[jobserver] not running"; exit 1; }
    free=$(idle_tokens)
    echo "[jobserver] running: $FIFO (pid $(cat "$PIDFILE")); idle tokens $free of $JOBS; rustc $(pgrep -xc rustc || true); refill loop $(refill_alive && echo on || echo OFF)"
    ;;
  refill)
    alive || { echo "[jobserver] not running"; exit 1; }
    refill_once
    ;;
  resize)
    alive || { echo "[jobserver] not running"; exit 1; }
    [ -n "${1:-}" ] || { echo "usage: $0 resize N [--shared]"; exit 2; }
    echo "$1" > "$JOBSFILE"; JOBS="$1"
    echo "[jobserver] pool size now $JOBS (held tokens are drained as they return)"
    refill_once
    ;;
  stop)
    if [ -f "$PIDFILE" ]; then kill "$(cat "$PIDFILE")" 2>/dev/null || true; fi
    if [ -f "$REFILL_PIDFILE" ]; then kill "$(cat "$REFILL_PIDFILE")" 2>/dev/null || true; fi
    rm -f "$FIFO" "$PIDFILE" "$REFILL_PIDFILE"
    echo "[jobserver] stopped"
    ;;
  *) echo "usage: $0 start|status|refill|resize N|stop [--shared]"; exit 2;;
esac
