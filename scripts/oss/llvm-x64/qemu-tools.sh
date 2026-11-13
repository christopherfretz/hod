#!/bin/bash
# Make the x86-64 tools of a tree runnable by lit on the AArch64 host
# (WP-180): qemu-tools.sh hod|llc [--undo]
#
# The container has no binfmt_misc entry for x86-64, and lit runs the
# tools of build/bin by name. Each x86-64 ELF executable build/bin/<t> is
# moved to build/bin/.x64/<t> and replaced by a two-line launcher that
# execs `$HOD_X64_QEMU -0 "$0" -L $HOD_X64_SYSROOT build/bin/.x64/<t>`:
# the program's argv[0] is the name it was run by, so the tools'
# diagnostics keep their names ("llc: error: ...") and a symlink such as
# llvm-readelf -> llvm-readobj chooses its mode as natively. Every
# launcher (also those of tools moved by an earlier run) is rewritten. The launcher is newer than the tool's inputs,
# so ninja sees the target up to date; a relink overwrites the launcher,
# and running this again moves the new binary. --undo puts the binaries
# back.
set -euo pipefail
source "$(dirname "$0")/env.sh"
flavour="${1:?usage: qemu-tools.sh hod|llc [--undo]}"
bin="$HOD_OSS_ROOT/$HOD_X64_PREFIX$flavour/build/bin"
mkdir -p "$bin/.x64"
n=0
if [ "${2:-}" = --undo ]; then
  for f in "$bin"/.x64/*; do
    [ -f "$f" ] || continue
    mv -f "$f" "$bin/$(basename "$f")"
    n=$((n + 1))
  done
  echo "restored $n tools in $bin"
  exit 0
fi
for f in "$bin"/*; do
  [ -f "$f" ] && [ ! -L "$f" ] || continue
  # x86-64 ELF: "\x7fELF", class 2, machine 0x3e at offset 18.
  [ "$(head -c 4 "$f" | od -An -c | tr -d ' ')" = '177ELF' ] || continue
  [ "$(od -An -tx1 -j18 -N1 "$f" | tr -d ' ')" = 3e ] || continue
  mv -f "$f" "$bin/.x64/$(basename "$f")"
done
for f in "$bin"/.x64/*; do
  [ -f "$f" ] || continue
  t=$(basename "$f")
  printf '#!/bin/sh\nexec "%s" -0 "$0" -L "%s" "%s" "$@"\n' "$HOD_X64_QEMU" "$HOD_X64_SYSROOT" "$f" > "$bin/$t"
  chmod +x "$bin/$t"
  n=$((n + 1))
done
echo "wrapped $n tools in $bin"
