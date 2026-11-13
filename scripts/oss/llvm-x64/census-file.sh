#!/bin/bash
# Regenerate $HOD_OSS_FINDINGS/LLVM-X64-CENSUS.md (WP-180, from WP-178): the stage log of
# both trees, the census of the hod tree (census.py), the saved modules
# (save-modules.py) and the hand-written notes in
# $HOD_OSS_FINDINGS/llvm-x64/notes.md. Also saves up to three modules per new
# refusal name.
set -euo pipefail
source "$(dirname "$0")/env.sh"
out="$HOD_OSS_FINDINGS/LLVM-X64-CENSUS.md"
mkdir -p "$HOD_OSS_FINDINGS/llvm-x64"
hod_tree="$HOD_OSS_ROOT/${HOD_X64_PREFIX}hod"
python3 "$HOD_OSS_SCRIPTS/census.py" "$hod_tree" --json "$hod_tree/logs/census.json" > "$hod_tree/logs/census.md"
python3 "$HOD_OSS_SCRIPTS/save-modules.py" "$hod_tree/logs/census.json" "$HOD_OSS_FINDINGS/llvm-x64" \
  --hod "$hod_tree/bin/hod" >&2
{
  echo "# LLVM 19.1.7 for x86-64 (cross-built on AArch64) by kiln with hod: census (WP-180, from WP-178)"
  echo
  echo "Updated $(date -u +%Y-%m-%dT%H:%MZ). hod: $(cat "$hod_tree/logs/hod-version.txt" 2>/dev/null)."
  echo "Trees: \`$hod_tree\` (kiln + hod, fallback allowed, HOD_CENSUS=1) and"
  echo "\`$HOD_OSS_ROOT/${HOD_X64_PREFIX}llc\` (kiln + llc, the control). -O0, assertions, X86;AArch64, no -g, x86_64-unknown-linux-gnu under qemu-x86_64."
  echo
  echo "## Stages (name, start, end, ninja rc, targets)"
  echo
  for f in hod llc; do
    [ -f "$HOD_OSS_ROOT/$HOD_X64_PREFIX$f/logs/stages.txt" ] || continue
    while read -r st s e rc rest; do
      echo "- $f $st: $((e - s)) s, rc $rc: $rest"
    done < "$HOD_OSS_ROOT/$HOD_X64_PREFIX$f/logs/stages.txt"
  done
  echo
  cat "$hod_tree/logs/census.md"
  echo "## Saved modules (\`$HOD_OSS_FINDINGS/llvm-x64/<reason>/\`: .ll, .cmd.txt, .hod.txt)"
  echo
  for d in "$HOD_OSS_FINDINGS"/llvm-x64/*/; do
    [ -d "$d" ] || continue
    echo "- \`$(basename "$d")\`: $(find "$d" -name '*.ll' -printf '%f ' | sed 's/ $//')"
  done
  echo
  [ -f "$HOD_OSS_FINDINGS/llvm-x64/notes.md" ] && cat "$HOD_OSS_FINDINGS/llvm-x64/notes.md"
} > "$out.tmp"
mv "$out.tmp" "$out"
echo "wrote $out"
