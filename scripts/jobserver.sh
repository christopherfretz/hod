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
# Run `start` when the container starts, BEFORE anything sources
# scripts/env.sh: env.sh exports MAKEFLAGS only if the fifo exists at that
# moment, and otherwise falls back to a per-cargo job cap without a word.
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
#   by name; compilers and the children of clients count as holders; while
#   clients are alive a deficit is refilled only if two consecutive passes
#   saw it; idle tokens above the pool size are drained. Over-counting
#   holders is safe. With no client alive every token must be idle, and
#   the pool is reset to exactly its size at once.
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

FIFO="${HOD_JOBSERVER_FIFO:-/run/hod/jobserver.fifo}"
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

# Every process that has the fifo open, except the holder: by descriptor,
# not by name (a test binary is `name-<hash>`).
client_pids() {
  local holder f
  holder=$(cat "$PIDFILE" 2>/dev/null || echo 0)
  for f in /proc/[0-9]*/fd/*; do
    [ "$(readlink "$f" 2>/dev/null)" = "$FIFO" ] && { f=${f#/proc/}; echo "${f%%/*}"; }
  done 2>/dev/null | sort -u | grep -vx "$holder" || true
}

# An upper bound on the tokens in use: compilers by name, and every child
# of a client (a harness holds one token per child process).
held_tokens() {
  local clients
  clients=$(client_pids | tr '\n' ' ')
  ps -eo pid=,ppid=,comm= | awk -v clients=" $clients" '
    { comm[$1] = $3; par[$1] = $2 }
    END {
      n = 0
      for (p in comm) {
        c = comm[p]
        if (c == "rustc") { n += 2; continue }
        if (c ~ /^(clang|clang\+\+|clang-[0-9]+|cc1|cc1plus|clippy-driver|rustdoc)$/) n += 1
        if (index(clients, " " par[p] " ")) n += 1
      }
      print n
    }'
}

clients_alive() {
  [ -n "$(client_pids)" ]
}

refill_once() {
  local idle rustc held want add
  idle=$(idle_tokens)
  rustc=$(pgrep -xc rustc || true)
  held=$(held_tokens)
  if ! clients_alive; then held=0; fi
  if [ "$idle" -gt "$JOBS" ]; then
    python3 - "$FIFO" "$(( idle - JOBS ))" <<'PY'
import os, sys
fd = os.open(sys.argv[1], os.O_RDWR | os.O_NONBLOCK); n = int(sys.argv[2]); got = 0
try:
    while got < n: got += len(os.read(fd, n - got))
except BlockingIOError: pass
PY
    echo "[jobserver] $(date -u +%FT%TZ) drained $(( idle - JOBS )) excess tokens (idle $idle, rustc $rustc, pool $JOBS)"
    return
  fi
  want=$(( JOBS - held ))
  add=$(( want - idle ))
  local prev=0 deficit_file="${FIFO%.fifo}.deficit"
  prev=$(cat "$deficit_file" 2>/dev/null || echo 0)
  echo "$(( add > 0 ? add : 0 ))" > "$deficit_file"
  if clients_alive && [ "$add" -gt 0 ]; then
    [ "$prev" -lt "$add" ] && add=$prev
    [ "$add" -gt 0 ] || { echo "[jobserver] $(date -u +%FT%TZ) deficit pending (idle $idle, held $held, rustc $rustc, pool $JOBS)"; return; }
  fi
  if [ "$add" -gt 0 ]; then
    python3 - "$FIFO" "$add" <<'PY'
import os, sys
fd = os.open(sys.argv[1], os.O_RDWR | os.O_NONBLOCK); os.write(fd, b"+" * int(sys.argv[2]))
PY
    echo "[jobserver] $(date -u +%FT%TZ) refilled $add tokens (idle $idle, held $held, rustc $rustc, pool $JOBS)"
  else
    echo "[jobserver] $(date -u +%FT%TZ) no refill (idle $idle, held $held, rustc $rustc, pool $JOBS)"
  fi
}

start_refill_loop() {
  if refill_alive; then return; fi
  mkdir -p "$(dirname "$REFILL_LOG")"
  nohup bash -c "while sleep 60; do '$SELF' refill >> '$REFILL_LOG' 2>&1; done" \
    >/dev/null 2>&1 &
  echo $! > "$REFILL_PIDFILE"
}

case "${1:-start}" in
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
    [ -n "${2:-}" ] || { echo "usage: $0 resize N"; exit 2; }
    echo "$2" > "$JOBSFILE"; JOBS="$2"
    echo "[jobserver] pool size now $JOBS (held tokens are drained as they return)"
    refill_once
    ;;
  stop)
    if [ -f "$PIDFILE" ]; then kill "$(cat "$PIDFILE")" 2>/dev/null || true; fi
    if [ -f "$REFILL_PIDFILE" ]; then kill "$(cat "$REFILL_PIDFILE")" 2>/dev/null || true; fi
    rm -f "$FIFO" "$PIDFILE" "$REFILL_PIDFILE"
    echo "[jobserver] stopped"
    ;;
  *) echo "usage: $0 start|status|refill|resize N|stop"; exit 2;;
esac
