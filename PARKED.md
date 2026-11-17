# WP-356 parked (budget cap): cuts done, evidence remaining

Branch `wp/356-parse-and-die-costs`, base `422cfcb3`. The base binary is
at `/Volumes/AgentWork/hod-helper/.build/target-wp-356/bin-base/hod`.
Bench inputs are already generated under
`/Volumes/AgentWork/hod-helper/.build/target-wp-356/bench/inputs/{clang22,clang22-x86_64}`.

## Counting method

`/Volumes/AgentWork/hod-helper/.build/target-wp-356/count.sh <a64|x64> <dirA> <dirB> <inputs...>`
counts both binaries under callgrind, whole program, in one script:
`HOD_THREADS=1 hod --strict -O0 -filetype=obj -relocation-model=pic
[-mtriple=x86_64-unknown-linux-gnu]`. Directories are relative to the
build directory and have names of the same length (`bin-base`,
`bin-cut1`, ...). It prints the input, the target, count A, count B,
the difference, and `same` or `DIFFERENT` (`cmp` of the objects).
`own.py <callgrind file>` ranks functions by their own cost.

## Profile of the base, `stl-heavy-g` x86-64 (963.05 M), own cost, M

node_body 87.68, skip_group 48.06, next_token 41.50, word 36.11,
Scanner::next 24.94, quick_location 20.41, build_debug_fed 18.94,
Made::node 16.48, DebugInfo::node 14.57, memcpy 14.06, parse_type 13.59,
exclaim 12.64, number 11.80, bcmp 11.69, Fx::call 10.68, after_blank 10.61,
compile_function 10.35, AttrMemo::keyword 9.73, md_ref 9.73, at_end 9.25,
Writer::die 9.20, _int_malloc 8.65, Fx::set 8.23, quick_flags 8.13,
Lexer::global 8.12. Then `type_die` 6.22.

## Cuts kept (each counted against the commit before it, one script)

1. `56026792`: `node_body` checks a node's required fields with one
   mask per kind (`required_mask`, built once; the check by name is still
   used to find the first missing field, so the message is the same), and
   keeps the fields outside the record in a `Vec` that is only set up
   when one appears (it was an array of 48 slices set up for every node).
   `stl-heavy-g` -4.80 M (x86-64) / -4.95 M (AArch64); `templates-g`
   -2.30 / -2.31 M. Objects the same.
2. `cfac7ede`: `Made::define` builds the node in place
   (`Made::fill`) instead of getting it back from `Made::node`, so the
   returned node is not copied. `stl-heavy-g` -1.10 M (x86-64) / -1.00 M
   (AArch64; 0.996 M, at the threshold); `templates-g` -0.46 / -0.46 M.
   Objects the same. Kept: it is at least 1 M on x86-64.

## Measured and reverted

3. The fed node loop moves the lexer (`seek`) only for nodes that are
   read through the lexer. `stl-heavy-g` -0.03 M (x86-64) / +0.01 M
   (AArch64): with `HOD_THREADS=1` the node loop is not fed. Dropped
   (never on the branch).
4. `quick_location` matches a field name by its length first, then its
   bytes. `stl-heavy-g` -0.49 M (x86-64) / -0.68 M (AArch64);
   `templates-g` -0.27 M. Under 1 M: dropped (never on the branch).

## Candidates not built

- Recording cap (`Recording::MAX`): `huge-function` has one body of
  about 94,000 lines. Its peak memory with the base is 11.3 MiB (14.2 MiB
  with `-g`). Recording the whole body at 16 bytes a token would cost
  several MiB, more than the 2% / 3 MiB that peak memory may grow by.
  A scratch build with a larger cap to count the gain was not built.
- `define` / `declare` headers lexed twice: keeping the index's tokens
  for every header until the parse costs 16 bytes a token for the whole
  module. Not built.
- `Writer::die`'s full decode per DIE: WP-349's doubling put it at
  3.32 M. Keeping decoded nodes from the planning costs memory per DIE.
  `constructing_node` copies the decoded `DiNode` out of the
  `constructing` stack at every construction step (types.rs:784). Making
  `construct` borrow the node is a larger refactor. Neither was built.

## Remaining, in order (no edits needed; build the head first)

0. `cd /Volumes/AgentWork/hod-helper/hod/.worktrees/wp-356 && source scripts/env.sh && cargo build --release -q -p hod_cli && mkdir -p /Volumes/AgentWork/hod-helper/.build/target-wp-356/bin-head && cp /Volumes/AgentWork/hod-helper/.build/target-wp-356/release/hod /Volumes/AgentWork/hod-helper/.build/target-wp-356/bin-head/hod`
1. Counts on all 20 inputs, both targets (in the background):
   `cd /Volumes/AgentWork/hod-helper/.build/target-wp-356 && for t in a64 x64; do ./count.sh $t bin-base bin-head containers containers-g exceptions exceptions-g huge-function huge-function-g interp interp-g small-c small-c-g small-cxx small-cxx-g stl-heavy stl-heavy-g tables tables-g templates templates-g tiny-functions tiny-functions-g; done`
   (the last column is the `cmp` of every object). Peak memory per
   input: `/usr/bin/time -f %M` on both binaries, same arguments.
2. `cargo xtask compare-binaries` (see `--help` for its arguments),
   the head against `bin-base/hod`, `-j 4`, `--out` an absolute path
   under the build directory, over `tests/ir`,
   `/Volumes/AgentWork/hod-helper/.build/corpus/kiln` and
   `/Volumes/AgentWork/hod-helper/.build/corpus-x64/kiln`: 0 different,
   0 changed outcomes.
3. `llvm-dwarfdump --verify` on `work/stl-heavy-g.x64.bin-head.o` and
   `work/templates-g.a64.bin-head.o` (left by step 1).
4. `cargo fmt --all --check`, `cargo clippy --workspace --all-targets -- -D warnings`,
   `HOD_REQUIRE_TOOLS=1 cargo test -q -p hod_ll -p hod_dwarf -p hod_ir`
   (`hod_ll` passed with cut 3 in the tree; it has not been run on the head).
5. Write the record `spec/landed/dwarf/WP-356.md` (in the form of
   WP-349's) from this file and steps 1 to 3. Then delete, each by its
   literal path: `bin-base`, `bin-cut1`, `bin-cut2`, `bin-cut3`,
   `bin-cut4`, `bin-head` and `work` under
   `/Volumes/AgentWork/hod-helper/.build/target-wp-356/`. Remove this
   file and commit.
