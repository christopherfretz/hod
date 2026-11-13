# 0002: parsing bodies in parallel (the plan WP-029 left)

This is the park note of WP-029, kept as the plan for its first item,
which is now the backlog row WP-063: type and constant tables that
workers can read while they parse, so that bodies are parsed in parallel
without a copy of the module per worker. "Done" below is on `main`.

**Status: built by WP-063, with the plan refined** (`spec/ir.md` §5.1,
`spec/landed/ll/WP-063.md`). Items 2 to 5 were done another way:

- No interning trait in the parser (item 3). The overlay is in `hod_ir`'s
  tables themselves: `Module::freeze` puts each table a body adds to in a
  shared base, and `Module::fork` is a module whose tables are that base
  and a layer of its own. The parsers are unchanged and read against a
  fork as against the module, so the sequential path costs nothing new.
- Item 2's lookup-only probe is the fork's lookup, base first.
- Item 4 is `Module::delta` / `Module::adopt` / `Body::remap`, and the
  driver is generic, `hod_ir::read_bodies`, shared by `hod_ll`
  (`parse_bodies_on`) and `hod_bc` (`read_bodies_on`). The plan missed
  that debug expressions and locations are interned by bodies too, and
  that an error must be the first in file order: a body that fails in a
  fork is read again on the module in its turn.
- The clone in `tests/differential/bodies.rs` is left as it is; the
  determinism tests are `crates/hod_ll/tests/parallel.rs` and
  `crates/hod_bc/tests/parallel.rs`.

## Done (branch `wp/029-ir-gaps`, each with its tests and spec lines)

- Coordinator addition: `"sample-profile-suffix-elision-policy"` is dropped
  (`attrs.rs`, `spec/ir.md` §5.7).
- Item 2: `Body::empty`, `cast_is_valid`, `sext64` are public and the `hod_ll`
  copies are gone. `attr_info` scans only `PREFIXED_STRING_ROWS` on a miss.
  `llvm.experimental.noalias.scope.decl` is `NoCode::Drop`. Metadata operands
  are **not** done (see Decisions).
- Item 3: forwarded varargs, a missing label is an error, and `blockaddress`
  of an unlabelled block (`spec/ll.md` §5.5, §11.6).
- Item 4: symbols are numbered in LLVM's module order (`spec/ll.md` §5.7,
  `spec/ir.md` §2.2, `spec/bc.md` §5.5). `hod_bc`'s differential test checks
  the order.

## Remaining, in order

1. **Baseline measurements, before any change.**
   - Throughput: `HOD_LL_CORPUS=… cargo test --release -p hod_ll --test corpus`
     reports MiB/s, but it is noisy on the shared host (83–143 MiB/s across
     runs this session). Take the median of 5 single-threaded runs, either
     of that test or of `examples/ll_stat.rs` on the largest corpus module,
     against WP-008's 155 MiB/s.
   - Peak memory: `/usr/bin/time -v` on a small example that parses every
     body of the largest module on 4 workers. Today that means one
     `Module::clone` per worker, as
     `tests/differential/bodies.rs:150` does.
2. **`hod_ir`: lookup-only access.**
   - Add `TypeTable::find(&TypeKey) -> Option<TypeId>` and
     `ConstPool::find(&Const) -> Option<ConstId>` on `&self`, plus the same
     for fn-attr sets and inline asm.
   - The hash and the probe are already split in `intern.rs`
     (`InternTable`), so this is a read-only probe with no insert.
3. **`hod_ll`: an interning trait behind `Parser`.**
   - Body mode would intern through `trait Tables { fn ty(..); fn konst(..);
     fn attrs(..); fn asm(..) }`, with two implementations:
     - `&mut Module`: today's path, which must stay as fast;
     - `Overlay<'m> { base: &'m Module, local tables }`: it gives local ids
       above `base.len()`, in the body's own order.
4. **The merge, in file order, serially.**
   - For each body in function order, intern its overlay's local entries
     into the `Module`, then rewrite the body's ids through the
     local→global map.
   - This needs a `hod_ir` remap: `Body::remap_ids(&Remap)` over `InstData`
     operand words, result types, constant operands, attribute and asm ids.
   - The ids come out equal to the sequential parse for any number of
     workers and any batching, because every new entry is interned in the
     same order as in a serial parse.
   - Determinism test: 1, 2 and 7 workers against `parse_bodies`, comparing
     ids and table lengths.
   - Most bodies add nothing new after the first few, so the remap is usually
     the identity; skip it when the overlay is empty.
5. **Spec.** Write the design into `spec/ir.md` §5.1 (replace the "snapshot
   semantics, §10" sentence) and `spec/ll.md` §11.1 ("Parallel parsing").
   Then replace the clone in `tests/differential/bodies.rs:150` with the
   overlay.

## Decisions and reasons

- **Item 1 parked:**
  - It is a two-crate redesign: the overlay tables, the interning trait
    through `Parser`, and a body remap in `hod_ir`.
  - It has its own measurement protocol, which needs a quiet host.
  - It did not fit after items 2–4.
  - Production `parse_bodies` is sequential today, and the clone exists
    only in the test above, so nothing regresses while this is parked.
- **Deferral rejected:** a lookup-only parallel pass that re-parses the
  misses serially. Most bodies intern at least one constant or GEP type, so
  most would be parsed twice.
- **Metadata operands (item 2) deferred:**
  - A new `Value` tag would change exhaustive `ValueKind` matches in
    `hod_cg/src/call.rs` and `hod_interp/src/interp.rs`, which other
    packages have in flight.
  - No corpus body has a metadata argument other than the one
    `noalias.scope.decl` drops.
- **Symbol order:**
  - Rule R2: kind-major (variables, functions, aliases, IFUNCs), each kind
    in definition/declaration order.
  - It was established with `llvm-as | llvm-dis` and `llc` on LLVM 19 and
    22, using forward references across kinds.
  - It is the bitcode record order, so `hod_bc` needed only to create
    unnamed globals in place.
