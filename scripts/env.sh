# source this before running cargo.
# Uses a workspace-local Rust toolchain if one exists (./.toolchain, or
# ../.toolchain when hod is a submodule of a wrapper repository), and keeps
# build output in ONE target directory PER CHECKOUT: a target directory
# shared between git worktrees is unsound (cargo names artifacts by path
# relative to the workspace root, so two checkouts overwrite each other).
_WS="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
_MAIN="$(cd "$(git -C "$_WS" rev-parse --path-format=absolute --git-common-dir 2>/dev/null || echo "$_WS/.git")/.." && pwd)"
case "$_MAIN" in */.git/modules*) _MAIN="${_MAIN%%/.git/modules*}";; esac
for _cand in "$_WS/.toolchain" "$_MAIN/.toolchain" "${HOD_TOOLCHAIN:-}"; do
  if [ -n "$_cand" ] && [ -d "$_cand/cargo/bin" ]; then
    export RUSTUP_HOME="$_cand/rustup" CARGO_HOME="$_cand/cargo"
    case ":$PATH:" in *":$CARGO_HOME/bin:"*) ;; *) export PATH="$CARGO_HOME/bin:$PATH";; esac
    break
  fi
done
export CARGO_TARGET_DIR="${CARGO_TARGET_DIR:-${HOD_BUILD_ROOT:-$_MAIN/.build}/target-$(basename "$_WS")}"
# Shared machines: one pool of parallelism tokens for every checkout in
# the container (`scripts/jobserver.sh start`, run when the container
# starts). While its fifo exists, cargo and rustc draw from it instead of
# each build having its own -j (cargo ignores CARGO_BUILD_JOBS when it
# inherits a jobserver), and a test binary runs a few tests at a time:
# libtest is not a jobserver client. Without a pool, cap the build's
# parallelism unless told otherwise.
_JSF="${HOD_JOBSERVER_FIFO:-/run/hod/jobserver.fifo}"
if [ -p "$_JSF" ] && [ -z "${HOD_NO_JOBSERVER:-}" ]; then
  export MAKEFLAGS="--jobserver-auth=fifo:$_JSF"
  unset CARGO_BUILD_JOBS
  export RUST_TEST_THREADS="${RUST_TEST_THREADS:-6}"
else
  export CARGO_BUILD_JOBS="${CARGO_BUILD_JOBS:-8}"
fi
unset _WS _MAIN _cand _JSF
# A workspace on a host bind mount has dropped rmeta/rlib writes under
# several concurrent pipelined builds (seen in kiln: "can't find crate" in
# the middle of a clean build). Without pipelining a dependent rustc never
# starts before the artifact is fully written.
export CARGO_BUILD_PIPELINING="${CARGO_BUILD_PIPELINING:-false}"
