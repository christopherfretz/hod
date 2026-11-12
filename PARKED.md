# WP-008 (`hod_ll` function bodies) — PARKED

Parked on supervisor instruction (token quota). Branch `wp/008-ll-bodies`,
worktree `/Volumes/AgentWork/hod-helper/hod/.worktrees/wp-008`. Nothing was
pushed or merged. Delete this file when the package is finished.

Read the WP-008 brief first (it is the contract). This file only says where
the work stands and what was decided.

## State in one paragraph

The module-level parser was refactored so that the same `Parser` can parse a
function body against finished tables (done, builds). Two of the three new
source files of the body parser are written but are **not yet part of the
crate** (`lib.rs` has no `mod body; mod inst;`), so the tree compiles exactly
as after the refactor. They have **never been compiled**. The third file
(`call.rs`) is not written. No test, no spec text, no measurement exists yet.

## What last passed / failed

- `cargo build -p hod_ll` after the refactor: OK, one dead-code warning
  (`Parser::into_scratch`, `Parser::attr_group` — both are used by `body.rs`
  once it is in the crate).
- WP-007's tests (`cargo test -p hod_ll`) were **not re-run** after the
  refactor. Run them first; behaviour at module level must be unchanged.
- The gate (`HOD_REQUIRE_TOOLS=1 cargo xtask ci --quick`) was **never run**
  on this branch.
- Nothing in `body.rs` / `inst.rs` has been compiled or tested. Expect
  borrow-checker and typo fixes.

## Done (compiles)

Refactor in `crates/hod_ll/src/` (module-mode behaviour intended unchanged):

- `parser.rs`: `Parser<'a>` became `Parser<'a, 'm>` with `module: &'m mut
  Module` (borrowed, was owned) and `text: Option<&'m ModuleText<'a>>`
  (`Some` = body mode). New `Scratch` (reusable vectors), `Parser::new(src,
  module, index, text, scratch)`, `into_scratch()`, `attr_group(id)`,
  `has_md_node(id)`. `run()` builds the module, then the parser.
- `types.rs` `named_type`: body-mode branch resolves through
  `text.named_type` / `text.numbered_type`; helper `undefined_type`.
- `globals.rs` `global_ref`: body-mode branch resolves through
  `module.find_symbol` / `text.numbered_global`; missing = `Undefined`.
- `meta.rs` `md_ref`: uses `has_md_node`.
- `attrs.rs`, `consts.rs`: only the `impl Parser<'_, '_>` lifetime edit.

## Half-done (written, not in the crate, never compiled)

### `crates/hod_ll/src/body.rs` (complete as designed)

Public: `BodyParser<'t,'a>` (`new(&ModuleText)`, `parse(&mut Module,
&FunctionText) -> Result<Option<Body>, LlError>` where `None` = unsupported,
`unsupported()`, `debug() -> &BodyDebug`, `recycle(Body)`), `BodyDebug`,
`DebugRecordText`, `ParsedBody`, `parse_bodies(module, text, each)`.
Internal: `Local`, `LocalNames` (generation-stamped open addressing),
`FnState` (all per-function scratch, reused), `CalleeInfo`, `Fx` (the parser
of one function), `body()` / `build()` (one pass; a second "numbering" pass
when a numbered block without a label is referred to before it is reached),
`unlabelled_blocks()` (unused so far), label scan, `define`, `local_value`,
`block_ref`, `label`, `statement`, `attachments`, and the debug seam between
the two `SEAM(WP-005)` comment lines.

Needs, to compile:
- `names.rs`: make `fn hash` `pub(crate)`.
- `lib.rs`: `mod body; mod call; mod inst;` and `pub use body::{BodyDebug,
  BodyParser, DebugRecordText, ParsedBody, parse_bodies};`.
- `Fx::metadata_operand` (see below) and `Fx::call` must exist.
- `attachments()` calls `self.p.md_ref()` and checks the span; fine as is.

### `crates/hod_ll/src/inst.rs` (complete as designed, except calls)

`Fx::instruction()` dispatches on the opcode word and returns
`Done = (result type, value, ended the block)`. Every instruction except
call/invoke/callbr is written: binary ops, `fneg`, `freeze`, `icmp`, `fcmp`,
casts (own `cast_is_valid`, a copy of `hod_ir::verify`'s), `select`, `phi`,
`va_arg`, `load`, `store`, `alloca` (static = slot), `cmpxchg`, `atomicrmw`,
`fence`, `getelementptr` (-> `ptr_add`, constant indices folded),
`extractvalue`, `insertvalue`, `extractelement`, `insertelement`,
`shufflevector`, `landingpad`, the five funclet instructions (parsed, noted
`inst:<opcode>`, emitted with `b.funclet`), `ret`, `br`, `switch`,
`indirectbr`, `resume`, `unreachable`.

It calls two functions that do not exist yet: `self.call(at, op, tail)` and
`self.metadata_operand()`.

## Remaining, in order

1. **Write `crates/hod_ll/src/call.rs`** (`impl Fx`), design:
   - `call(at, op: Opcode::{Call,Invoke,CallBr}, tail: TailKind) -> PResult<Done>`.
     Grammar: `[fast-math flags, call only] [cc | "cc" N] [return attrs]
     [addrspace(N)] type callee ( args ) [fn attrs | #N]* [ bundles ]`, then
     `to label %n unwind label %u` (invoke) or `to label %f [label %a, …]`
     (callbr). `type` is a function type or just the return type; in the
     second case the function type is built from the argument types (use the
     callee symbol's type when it is the same, to skip interning).
   - cc: as `globals.rs` `function()` does (`call_conv_keyword`,
     `CallConv::from_llvm`).
   - Callee: `@g` -> `Value::sym`; `%x` -> `local_value(token, PTR)`;
     `asm [sideeffect] [alignstack] [inteldialect] [unwind] "t", "c"` ->
     `module.inline_asm` after the function type is known, `Value::asm`;
     anything else -> `parse_const(PTR)`, and `Const::Addr{sym, addend: 0}`
     becomes `Value::sym(sym)` (a direct call is a symbol callee).
   - Per-symbol cache `FnState::callees: Vec<CalleeInfo>` indexed by
     `SymId`: name starts with `llvm.` and is a function -> first
     `hod_ir::no_code_intrinsic(name)`, else `resolve_intrinsic(types, name,
     symbol.ty())`; a known member is stored as `Intrinsic<'static>` (find it
     in `Intrinsic::ALL` by `name()`) with its overloads; else `Other`.
     `resolve_intrinsic` is slow, hence the cache.
   - No-code rules (`spec/ir.md` §5.5): `Drop` = no instruction, result is a
     poison constant (or nothing for void); `FirstArg` = result is the first
     argument's value (its type must be the return type); `DebugRecord` =
     skip the argument group with `self.skip_group()` and call
     `self.debug_call(name_span, args_span)`. An `invoke`/`callbr` of a
     no-code intrinsic: note `invoke:no-code-intrinsic`, emit `b.br(normal)`
     as stand-in.
   - Known intrinsic: `CallDesc::intrinsic(intrinsic, overloads, fn_ty,
     args)`; other `llvm.*`: `Intrinsic::Other(name)` with no overloads. If
     the call's function type differs from the declaration's for an `llvm.*`
     callee: planned as a `Type` error ("intrinsic called with incompatible
     signature") — **check against `llvm-as` 19 and 22 before keeping it**.
   - Args: `type [attrs] value`, into `s.vals`, `s.arg_tys`, `s.arg_attrs`
     (pass an empty `arg_attrs` slice when all are `ParamAttrs::NONE`).
     `metadata` typed argument: parse with `metadata_operand()`, and if the
     call would be emitted note `metadata-arg:<callee name>` (or
     `metadata-arg:indirect`) and return a stand-in. `...` as last argument:
     only after `musttail`; note `call:forwarded-varargs`.
     With an explicit function type check count and types against it.
   - `metadata_operand()`: `!"string"` (Exclaim then Str; `md_ref` does not
     take this form — peek with `bump` and `p.goto(start)` back), `!N` /
     `!{…}` / `!DIKind(…)` via `p.md_ref()` (check the inline span ends
     before `self.end`), else `type value`.
   - Fn attrs after `)`: `#N` -> `self.p.text.unwrap().attr_group(id)`: OR in
     `module.fn_attrs(group.attrs).flags()`, and note each `group.rejected`
     name as `attr:<name>` (look in `p.pending` before formatting, to avoid
     an allocation per call). Inline attrs: `p.fa.clear()` once, then
     `p.fn_attr(false)` until it returns false; read `p.fa.flags()`. Keep
     only `FnAttrs::NORETURN | NOUNWIND | RETURNS_TWICE` (`spec/ir.md` §5.7).
     `hod_ir::attr_info` scans ~100 rows on a miss: before calling
     `param_attrs` / `fn_attr`, skip when the token cannot be an attribute
     (not `Word`/`Str`; or the word is `void`, `ptr`, `null`, `true`,
     `false`, `undef`, `poison`, `zeroinitializer`, or a statement keyword
     such as `ret`, `br`, `call`, `store`, `to`, `unreachable`, `tail`).
   - Bundles: `[ "tag"(type value, …), … ]` into `s.bundle_tags`,
     `s.bundle_vals`, `s.bundles`; build a `Vec<Bundle>` only when there is
     one (corpus: only `"align"` ×10 and `"separate_storage"` ×4).
2. Add the three `mod` lines and the `pub use` to `lib.rs`, make
   `names::hash` `pub(crate)`, build, fix. Delete the duplicate
   `is_terminator` (keep one of `body.rs` / `consts.rs`).
3. `consts.rs` `block_ordinal`: add a body-mode branch (when `self.text` is
   `Some`): find the function by `SymId` in `text.functions()` (build a
   sorted `(sym, index)` vector once, keep it in `Scratch`), then
   `block_labels(src, function.body)` cached in `self.labels` by function
   index. Without it every `blockaddress` inside a body fails with
   "blockaddress of a function this module does not define". Closing the
   `blockaddress:unlabelled-block` follow-up (numbered block without a
   label) is optional: `body::unlabelled_blocks` was written for it; it
   needs a spare `FnState` and a guard against recursion. Such text is
   hand-written only (LLVM prints `N:` labels), so it may stay a follow-up.
4. Run `cargo test -p hod_ll` (WP-007 unchanged), then an env-var-driven
   corpus example/test (`.build/corpus`, 5,760 modules, read-only): parse
   every body, `verify` each, table of unsupported reasons, errors must be 0.
   Drive the remaining bugs from it.
5. Tests the brief lists: syntax rows vs `llvm-as` 19/22
   (`crates/hod_ll/tests/syntax/rows.rs`; rows that used the `BODY` reason
   must be updated), interpreter round trip (add `hod_interp` as
   dev-dependency; printer via `#[path]` to
   `crates/hod_interp/tests/llvm_text/mod.rs`; gate batch + `#[ignore]`d
   batch), real programs (`tests/ir/*.ll`, corpus sample), structure vs
   `opt -passes=instcount`, robustness (prefixes, mutations, deep nesting,
   100k-way switch, 100k phi incomings, million-instruction block), a
   parallel test (below), perf rows in `tests/perf.rs`.
6. Docs: `spec/ll.md` body sections (§11.x are referred to from the code
   comments: §11.1 API and the module/clone rule, §11.3 block numbering and
   the numbering pass, §11.4–§11.6 instructions, §11.7 no-code calls, §11.9
   debug seam; extend the §3.4 table of unsupported names),
   `spec/landed/ll/WP-008.md`, the `ll.md` row of `spec/README.md`.
7. Gate in the background (log
   `/Volumes/AgentWork/hod-helper/.build/wp008-gate-impl.log`), commit,
   empty the scratch dir, handback in the brief's format.

## Decisions made, and why

- **`spec/ir.md` §5.1 deviation (parallel parsing against frozen `&Module`
  is not implementable).** `hod_ir` has no lookup-only API for types or
  constants, `Value` has no immediate integers, so nearly every body interns
  something. Smallest deviation chosen: one entry point taking `&mut
  Module`; for parallel parsing each worker owns a **clone** of the module
  (`Module: Clone`) and its bodies are valid against that clone (ids already
  present mean the same in every clone). Unsupported names are returned by
  the parser as well as recorded, so a consumer can merge them. Planned
  test: one sequential pass to warm a module, then clones on 3 threads must
  give bodies `==` to a second sequential pass. To be reported as an
  `hod_ir` gap (proposal: lookup-only `find_*`, or §10's snapshot tables).
- **Any unsupported note in a body => no body (`Ok(None)`).** The body is
  still built with stand-ins so the rest of the text is checked, then
  `abandon()`ed.
- **Blocks**: `BlockId` = ordinal in text = number of terminators before
  the block (same rule as WP-007's `block_labels`, so module-level
  `blockaddress` constants agree). Unknown block name -> one lazy label scan
  from the current statement. Unknown *numbered* block after the scan ->
  `need_numbering`, one extra full pass that records `(number, ordinal)` of
  blocks without labels, then the real pass.
- **Locals**: numbers in a dense `Vec<Local>` (gap up to 4096) with a
  `HashMap` only for outliers; names in `LocalNames`. Forward references
  are `b.reserve(ty)` placeholders; the type is checked at each use and at
  the definition; undefined at the end = `Undefined` at the first use.
  LLVM's numbering rules are in `take_number` / `define`.
- **Lowering**: flags (`nuw`, `nsw`, `exact`, `disjoint`, `samesign`,
  `nneg`, fast-math, `inbounds`…) are parsed and dropped (`spec/ir.md`
  §5.4). Static alloca (entry block, literal count, address space 0) =
  `add_slot(alloc size × count, align or preferred align)`; otherwise the
  `Alloca` instruction. Default alignments: load/store ABI, alloca
  preferred, cmpxchg/atomicrmw store size rounded up to a power of two.
  `inalloca`, `swifterror`, scalable alloca, custom `syncscope`, scalable or
  non-byte vector GEP steps, funclets, fields without fixed offset are
  notes (names in `inst.rs`), never errors.
- **Call-site attributes**: only noreturn/nounwind/returns_twice are kept;
  the callee declaration's attributes are not merged in (a consumer reads
  them from the symbol).
- **What is not checked** (to be listed in the spec): dominance, phi lists
  against predecessors, landing-pad placement, inline-asm constraints,
  `musttail` rules — `Body::verify` or LLVM's verifier territory. A body
  that parses need not verify.

## Ruled out

- A "frozen tables, hit-only lookup" parallel scheme: it would miss on
  almost every body, and `Body` has no id-remapping API.
- A token-only pre-pass to number unlabelled blocks: whether an unnamed
  instruction takes a number depends on its result type, which needs the
  real grammar; hence the full numbering pass.
- Editing `hod_ir` to speed up `attr_info` misses or to expose
  `cast_is_valid` / `sext64`: out of scope; copies and pre-checks instead.

## hod_ir notes collected so far (for the handback)

- No lookup-only access to `TypeTable` / `ConstPool` (blocks §5.1 as
  written). `Body::empty()` is `pub(crate)`, so the first body cannot reuse
  storage (minor).
- `hod_ir::attr_info` (`crates/hod_ir/src/attrs.rs`, about line 1180) does
  a linear scan of the keyword rows on every miss; a parser hits that once
  per call instruction. Proposal: only scan the rows with `prefix` set.
- `verify.rs` `cast_is_valid` and `wide::sext64` are private; a producer
  has to copy them.

## Scratch

Nothing of this package is under
`/Volumes/AgentWork/hod-helper/.build/target-wp-008/` except cargo's own
output. One log: `/Volumes/AgentWork/hod-helper/.build/wp008-build0.log`.
