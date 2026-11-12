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
# Shared machines: cap build parallelism unless told otherwise.
export CARGO_BUILD_JOBS="${CARGO_BUILD_JOBS:-8}"
unset _WS _MAIN _cand
