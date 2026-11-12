#!/usr/bin/env bash
# One pool of parallelism tokens for every hod checkout on a machine: a GNU
# make jobserver (make 4.4's fifo style). `scripts/env.sh` points cargo and
# rustc at it through MAKEFLAGS when it is live, so concurrent builds in
# several worktrees queue for tokens instead of each running its own -j.
#
#   scripts/jobserver.sh start [N]    create the pool with N tokens (idempotent)
#   scripts/jobserver.sh status       idle tokens / pool size
#   scripts/jobserver.sh refill [N]   top the idle tokens up to N
#   scripts/jobserver.sh stop         remove the pool
#
# N defaults to $HOD_JOBS, else to the size the pool was started with, else
# 12. The fifo is $HOD_JOBSERVER_FIFO, else <build root>/jobserver.fifo with
# the build root of scripts/env.sh. Beside it: `<fifo>.pid` (the process
# that keeps the fifo open; a fifo forgets its bytes when nobody has it
# open) and `<fifo>.jobs` (the pool size).
#
# A client killed with SIGKILL takes its tokens with it. `status` shows the
# loss once nothing is building; `refill` restores it. `refill` is only
# right while no client holds tokens: tokens in use come back on top.
set -u

_WS="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
_MAIN="$(cd "$(git -C "$_WS" rev-parse --path-format=absolute --git-common-dir 2>/dev/null || echo "$_WS/.git")/.." && pwd)"
case "$_MAIN" in */.git/modules*) _MAIN="${_MAIN%%/.git/modules*}";; esac
FIFO="${HOD_JOBSERVER_FIFO:-${HOD_BUILD_ROOT:-$_MAIN/.build}/jobserver.fifo}"

live() {
  [ -p "$FIFO" ] && [ -r "$FIFO.pid" ] && [ "/proc/$(cat "$FIFO.pid")/fd/3" -ef "$FIFO" ]
}

size() {
  local n="${1:-${HOD_JOBS:-}}"
  [ -z "$n" ] && [ -r "$FIFO.jobs" ] && n="$(cat "$FIFO.jobs")"
  n="${n:-12}"
  case "$n" in ''|*[!0-9]*|0) echo "jobserver: not a job count: $n" >&2; exit 2;; esac
  echo "$n"
}

# The idle tokens: taken out without blocking, counted, put back.
idle() {
  local taken
  exec 4<>"$FIFO"
  taken="$(dd iflag=nonblock bs=65536 count=1 <&4 2>/dev/null)"
  printf '%s' "$taken" >&4
  exec 4>&-
  echo "${#taken}"
}

fill() {
  local n="$1" tokens
  [ "$n" -gt 0 ] || return 0
  tokens="$(printf '%*s' "$n" '' | tr ' ' '+')"
  printf '%s' "$tokens" > "$FIFO"
}

case "${1:-}" in
  start)
    n="$(size "${2:-}")" || exit 2
    if live; then
      echo "jobserver: already live: $FIFO ($(cat "$FIFO.jobs" 2>/dev/null || echo '?') tokens)"
      exit 0
    fi
    mkdir -p "$(dirname "$FIFO")"
    rm -f "$FIFO" "$FIFO.pid" "$FIFO.jobs"
    mkfifo "$FIFO" || exit 1
    # The holder: keeps descriptor 3 open on the fifo and does nothing.
    setsid sh -c 'exec 3<>"$0"; exec sleep infinity' "$FIFO" </dev/null >/dev/null 2>&1 &
    echo "$!" > "$FIFO.pid"
    for _ in 1 2 3 4 5 6 7 8 9 10; do live && break; sleep 0.1; done
    live || { echo "jobserver: the holder did not start" >&2; exit 1; }
    echo "$n" > "$FIFO.jobs"
    fill "$n"
    echo "jobserver: started: $FIFO ($n tokens)"
    ;;
  status)
    live || { echo "jobserver: not running ($FIFO)"; exit 1; }
    echo "jobserver: $(idle) idle of $(size) tokens: $FIFO"
    ;;
  refill)
    live || { echo "jobserver: not running ($FIFO)" >&2; exit 1; }
    n="$(size "${2:-}")" || exit 2
    have="$(idle)"
    if [ "$have" -ge "$n" ]; then
      echo "jobserver: $have idle tokens, nothing to add (pool size $n)"
    else
      fill "$((n - have))"
      echo "jobserver: added $((n - have)) tokens ($n idle)"
    fi
    echo "$n" > "$FIFO.jobs"
    ;;
  stop)
    if live; then kill "$(cat "$FIFO.pid")" 2>/dev/null; fi
    rm -f "$FIFO" "$FIFO.pid" "$FIFO.jobs"
    echo "jobserver: stopped"
    ;;
  *)
    echo "usage: scripts/jobserver.sh start [N] | status | refill [N] | stop" >&2
    exit 2
    ;;
esac
