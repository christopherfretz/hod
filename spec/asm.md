# `hod_asm`: assembly text in IR

LLVM IR carries assembly text in two places: the template of an
inline-assembly callee (`call`, `invoke`, `callbr` of `asm "..."`) and
module-level `module asm "..."`. `llc` hands both to its integrated
assembler. hod writes no assembly text and runs no assembler: `hod_asm`
parses the text and encodes each instruction itself, through
`hod_mc_a64`'s and `hod_mc_x64`'s checked encoders, and refuses what it does not understand
by a stable name. A wrong guess is the one real failure, so every form
accepted is one whose bytes are held to LLVM's assembler (§7).

Rules, for every caller:

- **Never an instruction hod did not parse and encode.** No word is
  copied from the text (`.inst` is refused), none is guessed.
- **A template hod does not understand refuses the module, by name**
  (§8). `hod`'s facade then falls back as for any refusal.
- **`hod_interp` refuses inline assembly** by name (`call:inline-asm`,
  `crates/hod_interp/src/interp.rs`): it has no machine to run it on.

Owners: `crates/hod_asm` (the text), `crates/hod_a64/src/inline_asm.rs`
(the lowering, §6; `spec/codegen.md` §5.5.3 "WP-103"),
`crates/hod_x64/src/barrier/template.rs` (x86-64's, §10, §11;
`spec/codegen.md` §5.6.28 "WP-187"),
`crates/hod_a64/src/barrier.rs` (empty templates, `spec/codegen.md`
"Assembly that needs no assembler (WP-075)"), `crates/hod_ir/src/asm_text.rs`
(module-level symbol directives, `spec/ir.md` §2.3).

## 1. What occurs

Measured over the corpus, kiln's tests, libc++'s tests and the system
headers they include (`spec/landed/codegen/WP-103.md` has the table):
empty templates (optimisation barriers) are 98% of all inline assembly;
then system-register reads and writes (`mrs`/`msr` of `fpcr`, `fpsr`,
`tpidr_el0`, `cntvct_el0`), `svc` wrappers, compare-and-flag templates
(`={@cc<cond>}`), short arithmetic with tied operands, `asm goto`, and
hand-written exclusive-monitor loops. Module-level assembly is symbol
directives (already WP-075's) and, in three corpus modules, small
functions and one datum written in assembly.

## 2. Statements (`text.rs`)

As LLVM's assembly lexer reads them for `aarch64-*-linux-*`:

- statements are separated by a newline or `;`;
- `//` starts a comment to the end of the line anywhere outside a string;
  `#` does at the start of a statement; `/* ... */` anywhere (it may span
  lines and does not end the statement);
- a string (`"..."`, backslash escapes) and a character literal (`'c'`)
  are kept whole.

An unterminated block comment, string or character literal is
`asm:lexical`.

## 3. Expressions (`expr.rs`)

Literals: decimal, `0x`, `0b`, octal with a leading `0`, `'c'`. Unary
`-` `~` `+` `!`. Binary operators with GNU precedence: `*` `/` `%` `<<`
`>>` (logical) bind tightest, then `|` `&` `^`, then `+` `-`, then `&&`,
then `||`. A value may name one symbol or local label: `sym`,
`sym + c`, `sym - c`, `c + sym`, `1f`, `2b`. Anything else (comparisons,
a difference of names, `.`, overflow, division by zero) is
`asm:operand/expression`.

## 4. Template substitution (`template.rs`)

As LLVM's `AsmPrinter::EmitInlineAsmStr` (GNU dialect) writes the text
its assembler reads:

- text is copied as it is; `$$` is `$`;
- `$(`, `$|`, `$)` delimit variants: only variant 0 is written; `$|`
  outside a group is `|`;
- `$N`, `${N}`: operand `N` (outputs, then inputs, then labels);
  `${N:m}` with the one-character modifier `m`;
- `${:uid}` a number unique to the call (hod: the instruction's id;
  `llc` counts from 0, which no correct template can tell apart),
  `${:comment}` is `//`, `${:private}` is `.L`.

A reference that does not parse or names no operand is `asm:template`
(it is fatal to `llc`); one in an unwritten variant is still checked.

How an operand prints (`a64::print_operand`), as
`AArch64AsmPrinter::PrintAsmOperand`:

| operand | none | `w` | `x` | `b` `h` `s` `d` `q` | `n` | `a` | `l` | `c` |
|---|---|---|---|---|---|---|---|---|
| general register | `xN` | `wN` | `xN` | `bN`..`qN` | | `[xN]` | | |
| vector register | `vN` | | | `bN`..`qN` | | | | |
| immediate | `v` | `v` (`wzr` for 0) | `v` (`xzr` for 0) | | `-v` | | | `v` |
| memory | `[xN]` | | | | | `[xN]` | | |
| symbol | `sym+c` | | | | | `sym+c` | | `sym+c` |
| label (`!i`) | `.Lhod_asm_goto_N` | | | | | | same | |

Any other pairing is `asm:modifier/<m>`.

## 5. AArch64 instructions (`a64/`)

### 5.1 Operands (`operand.rs`)

Registers, case-insensitive: `x0`..`x30`, `w0`..`w30`, `xzr`, `wzr`,
`sp`, `wsp`, `fp`, `lr`, `x31`/`w31` (zero), and `b0`..`q31`. Vector
registers with an arrangement (`v0.4s`), an element (`v0.s[1]`) and
register lists (`{v0.4s, v1.4s}`, `{v0.4s-v3.4s}`, `{v0.s, v1.s}[1]`)
are §20's; any other vector form parses so that an instruction refuses
it as `asm:operand/vector`. Immediates `#e` or `e`;
shifts and extends (`lsl #n`, `uxtw #n`, ...); memory operands `[xn]`,
`[xn, #i]`, `[xn, #i]!`, `[xn], #i`, `[xn, xm{, lsl #s}]`,
`[xn, wm, uxtw|sxtw {#s}]`; names (conditions, barrier options, system
registers, labels, symbols). Relocation specifiers (`:lo12:`) are
`asm:operand/specifier`; `ldr x0, =v` is `asm:operand/literal-pool`.

### 5.2 Mnemonics (`encode.rs`)

Each accepted form is one `hod_mc_a64` encoder call. Where LLVM's
assembler chooses an encoding for a written form, hod chooses the same
(a negative `add` immediate is a `sub`, `#4096` the shifted form, an
offset the scaled form does not take is the unscaled `ldur`/`stur`
form, a register next to `sp` the extended-register form), and the test
of §7 holds the choice. Covered:

- arithmetic and logic: `add` `adds` `sub` `subs` `cmp` `cmn` `neg`
  `negs` `adc` `adcs` `sbc` `sbcs` `and` `ands` `orr` `eor` `bic` `bics`
  `orn` `eon` `tst` `mvn`, every immediate, shifted and extended form
  (`bic` `bics` `orn` `eon` of an immediate since WP-216, §16);
- moves: `mov` (register, `sp`, every immediate LLVM accepts as one
  `movz`/`movn`/`orr`), `movz` `movn` `movk`;
- shifts, bitfields, extends: `lsl` `lsr` `asr` `ror` (and `-v`),
  `ubfx` `sbfx` `bfxil` `ubfiz` `sbfiz` `bfi` `ubfm` `sbfm` `bfm`
  `sxtb` `sxth` `sxtw` `uxtb` `uxth` `extr` `rbit` `rev` `rev16` `rev32`
  `clz` `cls`;
- conditional: `csel` `csinc` `csinv` `csneg` `cset` `csetm` `cinc`
  `cinv` `cneg` `ccmp` `ccmn`;
- multiply and divide: `mul` `mneg` `madd` `msub` `smull` `umull`
  `smaddl` `umaddl` `smsubl` `umsubl` `smulh` `umulh` `sdiv` `udiv`;
- `crc32{b,h,w,x}`, `crc32c{b,h,w,x}` (feature `crc`);
- branches: `b` `b.<cond>` `bl` `cbz` `cbnz` `tbz` `tbnz` `br` `blr`
  `ret` `adr`, to a numeric local label (resolved here) or a symbol (a
  relocation for the caller);
- loads and stores: `ldr` `str` (every size, `b`..`q` too) `ldrb` `strb`
  `ldrh` `strh` `ldrsb` `ldrsh` `ldrsw` `ldur`-forms, every addressing
  mode of §5.1; `ldp` `stp` `ldpsw`; `ldr` of a label (literal);
  `prfm` `prfum` (§16);
- exclusives and ordered: `ldxr` `ldaxr` `stxr` `stlxr` (and `b`/`h`),
  `ldar` `stlr` (and `b`/`h`), `ldxp` `ldaxp` `stxp` `stlxp`, `clrex`;
- LSE (feature `lse`): `cas` `swp` `ldadd` `ldclr` `ldeor` `ldset`
  `ldsmax` `ldsmin` `ldumax` `ldumin` with every ordering suffix and size;
- system: `nop` `yield` `wfe` `wfi` `sev` `sevl` `hint` `isb` `dmb`
  `dsb` (every option), `svc` `brk` `udf` `hlt`, `mrs`/`msr` of a system
  register (§5.4), `bti`, `paciasp` `pacibsp` `autiasp` `autibsp`
  `xpaclri`;
- scalar floating point (feature `fp`): `fmov` (register, general to
  and from FP, `#imm8`), `fabs` `fneg` `fsqrt` `frint*`, `fadd` `fsub`
  `fmul` `fdiv` `fnmul` `fmax` `fmin` `fmaxnm` `fminnm`, `fmadd`
  `fmsub` `fnmadd` `fnmsub`, `fcmp` `fcmpe` (`#0.0`), `fccmp` `fccmpe`
  `fcsel` `fcvt` `scvtf` `ucvtf` `fcvt{n,a,p,m,z}{s,u}`;
- scalar SIMD (§17, feature `neon` since §20): `fcmeq` on `s`/`d`
  registers (register, and `#0.0`/`#0`); the two-register forms of
  §20 (`fcvtzs d0, d1`, `fcvtns s0, s1`, `scvtf h0, h1`, ...);
- Advanced SIMD on vector operands (§20, feature `neon`): `fmla`
  `fmls` `fmul` `fmulx` by element; the floating-point three-same and
  two-register classes (`fadd` `fmla` `fcvtzs` `fcvtns` `frintn` ...);
  `fcmeq` `fcmge` `fcmgt` `fcmle` `fcmlt` (register and `#0.0`); the
  three-different class with its `2` forms (`umlal`, `smull`, `pmull`
  `pmull2`, `addhn`, ...); `ld1`..`ld4`, `st1`..`st4` (multiple, one
  lane) and `ld1r`..`ld4r`.

Any other mnemonic is `asm:mnemonic/<name>`; a known mnemonic with
operands outside these forms is `asm:operand/<what>` (`form`, `range`,
`register`, `sysreg`, `option`, `vector`, `fp-immediate`, ...); a form
that needs a feature the function lacks is `asm:feature`.

### 5.3 An inline body (`a64::assemble`)

A body is the substituted template. Numeric local labels (`1:`, `1b`,
`2f`) are resolved within it; a reference with no definition in its
direction is `asm:operand/label`, one out of the instruction's reach
`asm:operand/reach`. A named label is `asm:label` (a body may be
emitted more than once by an optimiser; hod refuses rather than
guesses); `.p2align`, `.balign` and `.align` pad as §17 says
(`a64::assemble_at`); any other directive is `asm:directive/<name>`. A
reference to a
symbol is returned as a `Ref` (offset, kind, name, addend) for the
caller.

### 5.4 System registers (`sysreg.rs`, WP-188)

`mrs <Xt>, <reg>` and `msr <reg>, <Xt>` (`xzr` allowed, a W register,
`sp` or a vector register is `asm:operand/register`) take a register by
name, in any case, or in the generic form `S<op0>_<op1>_C<n>_C<m>_<op2>`
(any case; decimal fields without leading zeros, op0 0..3, op1 and op2
0..7, n and m 0..15). The words are `llvm-mc-22`'s: the field
`op0:op1:CRn:CRm:op2` at bit 5 of `0xd520_0000` (`mrs`) or `0xd500_0000`
(`msr`). The generic form is accepted for both directions whatever the
register, as `llvm-mc` accepts it, and needs no feature.

The names are the table `sysreg::TABLE`, generated once from
`llvm-mc-22` (a test regenerates it when the tool is present and
compares): every name `llvm-mc-22` prints, without a feature, for the
ID-register space Linux emulates for user space (op0 3, op1 0, CRn 0,
CRm 0..7: `midr_el1`, `mpidr_el1`, `revidr_el1`, the `id_aa64*_el1`
and AArch32 `id_*_el1`/`mvfr*_el1` registers), and `cntfrq_el0`,
`cntvct_el0`, `cntpct_el0`, `tpidr_el0`, `tpidrro_el0`, `fpcr`, `fpsr`,
`nzcv`, `daif`, `dczid_el0`, `ctr_el0`, `currentel`, `rndr`, `rndrrs`;
each with the directions `llvm-mc` accepts (`msr` to a read-only
register is `asm:operand/sysreg`). `rndr` and `rndrrs` need `+rand` in
the function's `target-features`, else `asm:feature`. Names `llvm-mc`
gates behind another feature (`id_aa64zfr0_el1`, `id_aa64smfr0_el1`,
`id_isar6_el1`, `id_pfr2_el1`, `tpidr2_el0`, `tco`, `dit`, `ssbs`, ...)
and any other name are `asm:operand/sysreg` (the generic form reaches
them). `msr` of an immediate to a PSTATE field (`daifset`, `spsel`, ...)
is `asm:operand/sysreg`.

## 6. Inline assembly in code (`hod_a64::inline_asm`)

The lowering is `spec/codegen.md` §5.5.3 "WP-103". In short:

- **Features** come from the function's `target-features`
  (`Features::from_attribute`): `+lse`, `+crc`, an architecture from
  `v8.1a` on (both), `-fp-armv8`; `neon`, `aes` and `fp16` with LLVM's
  implications, in order (§20).
- **SIMD&FP operands** (§20): `w` takes a value of 16, 32, 64 or 128
  bits, `x` one of 128, `y` none (`asm:constraint/y`); another is
  `a64:inline-asm-operand-type`, as are `i128` and vectors of 2 or 4
  bytes, which `llc` places and hod does not. Clobbers of `v8`..`v15`
  (any view) keep the low halves the procedure call standard saves.
- **Constraints** read: the prefixes `=`, `+` (as LLVM writes it: an
  output and a tied input), `&`, `*` (indirect), `%`; the codes `r`,
  `w`, `x`, `y`, `X`, `m`, `Q`, `o`, `i`, `n`, `I`..`N`, `{reg}`,
  `{@cc<cond>}` (flag output), `!i` (label), digits (tied), `|`
  alternatives, `~{clobber}`. Another code is `asm:constraint/<c>`;
  another clobber `asm:clobber/<name>`.
- **Symbols** a body references resolve to the module's symbols; a
  name the module does not have is `asm:symbol`.
- **`inteldialect`** is `asm:dialect`; `unwind` on an `invoke` whose
  template is not empty is `a64:inline-asm-unwind`.

## 7. Tests

- `crates/hod_asm/tests/llvm_mc.rs`: every accepted form (over 330
  statements, every family of §5.2, the choices LLVM makes) is
  assembled by hod and by `llvm-mc` 22 and 19
  (`-triple=aarch64-linux-gnu -mattr=+lse,+crc`), word for word; the
  refused table checks each name; feature gating; `prfm` with an offset
  only `prfum` takes against `llvm-mc` 22 alone (§16); the aligned
  bodies of §17 at 16 start offsets, and fill 0 as `llvm-mc` 22 writes
  it.
- `crates/hod_a64/tests/inline_asm_align.rs` (§17): templates with
  alignment directives at several offsets of their functions, in hod's
  objects and `llc` 19's and 22's: the bytes `llvm-mc` writes at that
  offset, the sections aligned alike; a cut before an aligned template
  refused.
- `crates/hod_asm/tests/llvm_mc.rs` (§20): the NEON bodies word for
  word against `llvm-mc` 22 and 19 (`-mattr=+lse,+crc,+fp-armv8,+aes,
  +fullfp16`), their refusals by name, and a feature matrix (bodies ×
  `target-features` strings) where hod accepts exactly what `llvm-mc`
  accepts under the same `-mattr`.
- `crates/hod_a64/tests/inline_asm_neon.rs` (§20): the operand grid
  (constraint × type × modifier) against `llc` 19 and 22; the census's
  NEON templates in hod's code; the refusals.
- `crates/hod_asm/tests/census_a64.rs` (§16): the AArch64 census, each
  body by the table; the accepted ones `llvm-mc` 22's and 19's words.
- Unit tests in each module: statements, expressions, substitution,
  local labels, operand printing, refusal-name interning.
- `crates/hod_a64/tests/inline_asm.rs`: the lowering (§6), run against
  `llc`; the template's words appear in the code as `llvm-mc` writes
  them.
- `crates/hod_asm/tests/sysreg.rs` (§5.4): the table regenerated from
  `llvm-mc-22` equals `sysreg::TABLE`; every row in each direction it
  allows (both cases) and the generic form at its edges for each op0
  encode as `llvm-mc-22`; the refusals.
- `crates/hod_a64/tests/sysreg_asm.rs` (§5.4): seeds reading
  `cntfrq_el0` (by name and generic), `cntvct_el0`, `tpidr_el0`,
  `midr_el1`, ID registers, `ctr_el0`, `dczid_el0`, and writing `fpcr`,
  `nzcv`, `fpsr`, built by hod and by `llc` 19 and 22, linked with a C
  driver that checks each against its own `mrs`, run under every
  linker; the system-register words of hod's code equal `llc`'s.
- `crates/hod_cg/tests/emit_module_asm.rs`: module text (§9: every
  directive, section shape, reference kind, and module-symbol meeting)
  emitted by hod and by `llc` 19 and 22: sections with their bytes,
  symbols, relocations and code-section mapping symbols equal; the
  refusal table; x86-64 stays `module:asm`.

## 8. Refusals

`asm:mnemonic/<name>`, `asm:directive/<name>`, `asm:constraint/<code>`,
`asm:clobber/<name>`, `asm:modifier/<m>` carry a part of the input,
lower-cased; a part that is not a short plain word (or past 4096
distinct names) is `other`. The fixed names: `asm:operand/{syntax, form,
range, register, sysreg, option, vector, fp-immediate, specifier,
expression, symbol, label, reach, literal-pool}`, `asm:label`,
`asm:lexical`, `asm:feature`, `asm:template`, `asm:dialect`,
`asm:section`, `asm:symbol`, `asm:unaligned` (an instruction of module
text at an offset that is not a multiple of 4, §9). `asm:align-crossed`
(§18: an x86-64 template branch across an alignment directive that may
pad). From code generation: `limit:aligned-code-moved` (§17: an aligned
AArch64 template after bytes the code's shrinking removes) and
`x64:align-tune-cpu` (§18: padding `nop`s for a tuning hod has not
measured).

The AArch64 mnemonics real templates still need, and their refusals, are
§16's table as §20 leaves it: SVE (`asm:mnemonic/cntb`), the NEON
mnemonics §20 does not take (`asm:mnemonic/<m>`; a vector operand of a
form §20 does not take is `asm:operand/vector`), the directives other
than §17's (`asm:directive/inst`, `/arch_extension`, `/balignw`, ...;
x86-64 takes the same three alignment directives since WP-218, §18),
`asm:mnemonic/adrp` (with `asm:operand/specifier` for `:lo12:`). A prefetch
operation that is no name of §16 is `asm:operand/option`; `prfm` and
`prfum` take no write-back.

## 9. Module-level assembly (`module.rs`, WP-112)

`assemble_module(text, features, &mut ModuleCode)` reads the module's
text (AArch64 ELF only; on x86-64 such text stays `module:asm`, it never
reaches this assembler) into sections, symbols and relocations, as
`llc` (which emits module assembly before anything else) writes them.
Text that is only symbol directives stays WP-075's (`spec/ir.md` §2.3).
The emitter's side is `spec/codegen.md` §9.7.3.

- **Sections.** `.text`, `.data`, `.bss`, `.section name{,"flags"{,@type}}`,
  `.pushsection`/`.popsection`, `.previous`. A name is `.text`, `.rodata`,
  `.data` or `.bss`, alone or followed by `.` and more; its kind (type and
  flags) is LLVM's default for the name, and the flags written may only
  be ones the kind has (`a`, `w` for data and bss, `x` for text) and are
  OR'd with the defaults; `@progbits`/`@nobits` must agree with the kind.
  Anything else (a mergeable `.rodata.str*`/`.rodata.cst*`, a group,
  `.tbss`, a custom name, `.popsection` of an empty stack) is
  `asm:section`. Text before any section directive is in `.text`.
  Alignment starts at 4 for code (LLVM 22; LLVM 19 gives a code section
  the text creates alignment 1, `.text` 4: the one difference, and hod
  follows 22) and at 1 otherwise, raised by each alignment directive.
- **Data.** `.byte`; `.hword`/`.short`/`.2byte`; `.word`/`.long`/`.int`/
  `.4byte`; `.quad`/`.xword`/`.8byte`: comma lists of §3 expressions. A
  constant must fit the field (signed or unsigned), else
  `asm:operand/range`. `name+c` in a 2-, 4- or 8-byte field is a
  relocation (`R_AARCH64_ABS16/32/64`); in a byte, or in `.bss`, it is
  refused. `.ascii`, `.asciz`/`.string` (comma lists, the escapes of
  LLVM's lexer), `.zero n`, `.space`/`.skip n{, fill}`.
- **`.ident "string"`** (one string) writes nothing to the current
  section: the strings go to `.comment` as LLVM's ELF streamer writes
  them (`emitIdent`): `SHT_PROGBITS`, `SHF_MERGE | SHF_STRINGS`, entries
  of one byte, a NUL, then each string with its NUL, in order. The
  string's bytes are the ones written between the quotes: LLVM reads no
  escape here (`\t` is a backslash and a `t`), unlike `.ascii`. (`llc`
  appends the strings of `!llvm.ident` to the same section; hod drops
  that metadata, so with both its `.comment` is the first part of
  `llc`'s.)
- **Alignment.** `.p2align n{, fill}` and AArch64's `.align n` (a power
  of two), `.balign n{, fill}`; no maximum-skip operand. Code is padded
  as LLVM pads it: zeros up to a multiple of 4, then `nop`s; with a fill,
  the fill byte; data with zeros. No mapping symbol marks padding.
- **Instructions** are §5's, only in a code section, each at a multiple
  of 4 (else `asm:section`, `asm:unaligned`). Mapping symbols: `$x` where
  instructions start, `$d` where data starts, in code sections (hod
  writes none in data sections, `spec/codegen.md` §9.4).
- **Labels and symbols.** Named labels, numeric labels (`1:`, `1b`, `2f`,
  resolved within one section). `.globl`/`.global`, `.weak`, `.hidden`,
  `.protected`, `.type name, @function|@object|@notype` (and the `%`,
  `STT_` spellings), `.size name, .-name` or a constant, `.set`/`.equ`/
  `name =` of a constant. `.L` names are temporaries (no symbol). A name
  given contradictory directives (`.globl` and `.weak`, `.set` after use
  or a label, a directive on a temporary, `@gnu_indirect_function`, a
  `.size` of a name not defined here) is `asm:symbol` or `asm:label`.
- **References** resolve as LLVM's ELF writer resolves them: a
  PC-relative field to a local label in the same section is filled in;
  one to a local label elsewhere, and any absolute field to a local
  label, is a relocation against the label's section with the label's
  offset in the addend; a field to a global or weak label, or to an
  undefined name, is a relocation against the symbol; a name `.set`
  gave a constant is that constant.

What the emitter adds (`spec/codegen.md` §9.7.3): a label named as a
module symbol must be the definition of a module declaration (external
linkage, not thread-local), else `asm:symbol`; directives on module
symbols follow WP-075's rules.

## 10. x86-64 instructions (`x64/`, WP-187)

The x86-64 assembler reads AT&T syntax, the dialect `llc` prints for
`x86_64-*-linux-*` (variant 0 of `$(..$|..$)`), and encodes every
instruction through `hod_mc_x64`'s constructors; there is no second
encoder. `crates/hod_asm/tests/llvm_mc_x64.rs` holds every accepted form
to the bytes `llvm-mc` 22 and 19 write (`--triple=x86_64-unknown-linux-gnu`,
object `.text`), keeps a pinned core whose bytes are written in the test,
and checks the refused table by name.

### 10.1 Statements (`x64::clean`)

As LLVM's x86 lexer reads them: `#` and `//` start a comment to the end
of the line anywhere outside a literal, `/* */` is a comment, `;` and
newlines end a statement. Mnemonics are case-insensitive.

### 10.2 Operands (`operand.rs`)

`%reg` (the 8-, 16-, 32-, 64-bit names of the sixteen general
registers, `xmm0`..`xmm15`); `$expr` (an immediate, a constant
expression of §3); memory `seg:disp(base, index, scale)` with `%fs`/`%gs`
segments, a 64-bit base or `%rip`, a 64-bit index other than `%rsp`,
scale 1, 2, 4, 8; a bare expression (an absolute address, or a branch
target). A 32-bit address register is `asm:operand/register`, another
scale `asm:operand/range`, `*` (an indirect branch) `asm:operand/form`.
`ah`..`dh`, x87, MMX, `xmm16` and up, `ymm`, `zmm`, control and segment
registers as operands are refused.

**Symbols.** A memory operand's displacement may name a symbol, with a
constant before it or after its specifier, and a relocation specifier
read without regard to case (`sym@tpoff+8`, `8+sym@gotpcrel(%rip)`).
The field is written zero and returned as a `Ref` (offset in the code,
kind, name, addend: the constant, and for a `%rip` field minus the bytes
from the field to the instruction's end), of the relocation LLVM's
assembler (llvm-mc 19 and 22) writes:

| operand | relocation |
|---|---|
| `sym(%rip)` | `R_X86_64_PC32` |
| `sym@plt(%rip)` | `R_X86_64_PLT32` |
| `sym@gottpoff(%rip)` | `R_X86_64_GOTTPOFF` |
| `sym@gotpcrel(%rip)`, the source of a 32- or 64-bit `mov adc add and cmp or sbb sub xor`, or the memory of `test`, no constant | `R_X86_64_GOTPCRELX`, `R_X86_64_REX_GOTPCRELX` with a REX prefix |
| `sym@gotpcrel(%rip)` otherwise, not `lea` | `R_X86_64_GOTPCREL` |
| `sym@tpoff` without `%rip` | `R_X86_64_TPOFF32` |
| `sym@dtpoff` without `%rip` | `R_X86_64_DTPOFF32` |
| `sym` without `%rip` | `R_X86_64_32S` |

`lea` of `@gotpcrel` (LLVM 19 writes `R_X86_64_GOTPCREL`, 22
`R_X86_64_REX_GOTPCRELX`), another specifier (`@got`, `@gotoff`,
`@ntpoff`, ...), and a specifier on an immediate are
`asm:operand/specifier`; a symbol in an immediate or a branch target,
or a specifier on a numeric label, `asm:operand/symbol`.

### 10.3 Mnemonics (`encode.rs`)

The operation size is the suffix's (`b`, `w`, `l`, `q`) or the general
registers' when there is none; they must agree (`asm:operand/register`),
and a form with neither (`inc (%rax)`) is `asm:operand/form`, as LLVM
refuses it as ambiguous. Families:

- without operands: `nop`, `pause`, `cpuid`, `rdtsc`, `rdtscp`,
  `xgetbv`, `mfence`, `lfence`, `sfence`, `int3`, `ud2`, `hlt`,
  `fwait`/`wait`, `cld`,
  `std`, `syscall`, `pushf[q]`, `popf[q]`, `cbtw`/`cbw`, `cwtl`/`cwde`,
  `cltq`/`cdqe`, `cwtd`/`cwd`, `cltd`/`cdq`, `cqto`/`cqo`;
- `add or adc sbb and sub xor cmp`, `test`; `inc dec not neg mul div
  idiv`; `imul` of one, two and three operands;
- shifts `rol ror rcl rcr shl sal shr sar` (immediate, `%cl`, implicit
  1), `shld`, `shrd`; `bsf bsr tzcnt lzcnt popcnt`; `bt bts btr btc`;
  `bswap`;
- `mov` (and `movabs`), `movz*`/`movs*` with their sizes (`movzbl`,
  `movslq`, ..., `movzx`, `movsx`), `lea`, `xchg`, `xadd`, `cmpxchg`,
  `cmpxchg8b`, `cmpxchg16b`, `push`, `pop` (64-bit), `push $imm`, `int
  $n`, `fnstcw`, `fldcw`;
- `set<cc>`, `cmov<cc>`, `jmp` and `j<cc>` to a numeric local label;
- SSE: `adds/subs/muls/divs/sqrts/mins/maxs` `s`/`d`, `andp andnp orp
  xorp ucomis comis` `s`/`d`, `movss movsd movaps movapd movups movupd
  movdqa movdqu movd movq cvtss2sd cvtsd2ss`;
- the instructions of the census, §13.

Where LLVM chooses among encodings of one written form (`xchg` with
`%rax`, the short immediate forms, `movq $imm` as `movabs`), hod's
constructor makes the same choice. Another mnemonic is
`asm:mnemonic/<name>`; `call` among them (a body calls nothing).

### 10.4 An inline body (`x64::assemble`)

- **Prefixes**: `lock` before an instruction `hod_mc_x64` marks
  lockable (a read-modify-write of memory), in the same statement (`F0` after an
  operand-size `66`, as LLVM writes it) or alone (`F0` first). `rep`
  (`repe`, `repz`; `F3`) and `repne` (`repnz`; `F2`) before a string
  instruction without operands or `nop` (§13), joined or alone: the
  prefix byte first either way. Any other pairing, or a prefix last, is
  `asm:operand/form`.
- **Data**: `.byte`, `.short`/`.value`/`.word`/`.2byte`,
  `.long`/`.int`/`.4byte`, `.quad`/`.8byte` of constants that fit the
  field, signed or unsigned (`asm:operand/range`).
- **Labels**: numeric local labels, branches to them chosen short or near
  as LLVM's relaxation does (short until the distance needs `rel32`,
  repeated to a fixed point). A named label is `asm:label`; `.p2align`,
  `.balign` and `.align` in code are §18's (`x64::assemble_aligned`;
  `x64::assemble` still refuses them); any other directive
  `asm:directive/<name>`. A body defines no symbol; it
  references one only through a memory operand (§10.2 "Symbols").
- **Operands printed** (`print_operand`, as `X86AsmPrinter`): a general
  register in the view of its value's size (an `i32` in `eax`); `b`,
  `w`, `k`, `q` select the 8-, 16-, 32-, 64-bit view; `c` plain; `a`
  `(%reg)`; a constant `$v`, bare with `c`, `P`, `a`, negated with `n`;
  memory `(%reg)`, `8(%reg)` with `H`; an SSE register `%xmmN`. `h`
  (high byte) and every other modifier is `asm:modifier/<m>`.

## 11. Inline assembly in code on x86-64 (`hod_x64::barrier::template`)

The lowering is `spec/codegen.md` §5.6.28. In short:

- **Constraints** read by the shared parser (`hod_cg::asm_constraints`)
  with x86's codes: `r`, `l`, `q` (any general register), `Q` (`rax`,
  `rcx`, `rdx`, `rbx`), `R` (the legacy eight), `x`, `v` (SSE), `m`,
  `o`, `X`, `i`, `n`, `e`, `Z`, `I`..`O` (ranged immediates), `{reg}`,
  `{@cc<cond>}`, `Ws` (clang's `^Ws`) and `s` (a symbol plus a constant
  offset, §13), digits, `|` alternatives, `=`, `+`, `&`, `*`, `%`.
  `llc` refuses `a`, `b`, `c`, `d`, `S`, `D` and `g` in IR (front ends
  write `{ax}` and `imr`), so hod refuses them as
  `asm:constraint/<letter>`; so is `A`, `t`, `u`, `f`, `y`, `p`.
- **Clobbers**: `memory`, `cc`, `flags`, `eflags`, `dirflag`, `fpsr`,
  `fpcr`, `mxcsr`, x87 and MMX registers and `xmm16` and up (none holds
  a value of hod's across the template), general registers by any name
  (`ah`..`dh` as their register), `xmm`/`ymm`/`zmm` 0..15 as the SSE
  register. Another is `asm:clobber/<name>`; `rsp`, `rbp` are
  `x64:inline-asm-frame-register`.
- **`inteldialect`** is `x64:inline-asm-intel`; `unwind` on an `invoke`
  is `x64:inline-asm-unwind`; a `callbr` with a template stays
  `x64:inline-asm`; a bundle `x64:inline-asm-bundle`.

Tests: `crates/hod_x64/tests/inline_asm.rs` (§5.6.28).

## 12. Real-project system-register reads; `callbr` results on AArch64 (WP-196)

- The system-register reads kiln writes for real projects on AArch64
  (abseil's `UnscaledCycleClock::Now`/`Frequency`: `mrs $0, cntvct_el0`
  and `mrs $0, cntfrq_el0` in a `sideeffect` template with one `=r`
  output; `crc_internal::GetCpuType`: `mrs $0, MIDR_EL1`, upper case,
  without `sideeffect`; `optnone` functions with
  `+fp-armv8,+neon,+outline-atomics,+v8a,-fmv`) are §5.4's and compile
  strict; the template's word is `llc`'s up to the general register the
  allocator chose. Test: `crates/hod_a64/tests/sysreg_asm.rs`
  `kiln_shapes_compile_with_llc_s_words`.
- A `callbr` result defined again on an indirect edge (§6,
  `spec/codegen.md` §5.5.3) is released after that edge's `phi` copies:
  a register result whose only uses were on another edge (a loop
  header's `phi`) no longer stays in a register at the end of the block.
  Test: `crates/hod_a64/tests/callbr_phi.rs` (`loop_reg`, built by hod
  plain and stressed and by `llc` 19 and 22, linked and run).

## 13. The x86-64 census (WP-199)

The mnemonics and operand forms of §10 were chosen from the templates
real code writes: the x86-64 real-project lane's modules (each `call asm`
of `.build/corpus-x64` and the lane's saved findings) and the templates
of widely used sources (the C library's and the compilers' headers,
abseil, protobuf, the sanitizer and OpenMP runtimes, LLVM's own). The
census is `crates/hod_asm/tests/census_x64.rs`: each body as it reads
once its operands are substituted, where it is written, how many
templates of that shape were found, and what hod does with it; every
accepted body is `llvm-mc` 22's and 19's bytes. The record
(`spec/landed/codegen/WP-199.md`) has the table.

**Added** (each encoded as `llvm-mc` encodes it, `tests/llvm_mc_x64.rs`):

- the prefetches `prefetchnta`, `prefetcht0`..`t2`, `prefetch`,
  `prefetchw`, `prefetchwt1`; the flushes `clflush`, `clflushopt`,
  `clwb`; `fxsave`/`xsave` and their family (`..64` forms);
- `nop{w,l,q}` with a register or memory operand (`nop` with a register
  takes its size from it; with memory and no suffix it is
  `asm:operand/form`, ambiguous to LLVM too);
- without operands: `rdmsr`, `wrmsr`, `rdpmc`, `xsetbv`, `xend`,
  `xtest`, `rdpkru`, `wrpkru`, `serialize`, `endbr64`, `enclu`,
  `encls`, `enclv`, `pconfig`, `fnclex`, `fninit`, `clc`, `stc`, `cmc`,
  `lahf`, `sahf`, `cli`, `sti`; `hreset $imm`;
- `fnstsw` (no operand, `%ax`, memory), `fnstenv`, `fldenv`, `fnsave`,
  `frstor`, `stmxcsr`, `ldmxcsr`; the waiting forms `fstcw`, `fstsw`,
  `fstenv`, `fsave`, `fclex`, `finit`, each `9B` and the form without
  the wait (LLVM writes two instructions; the bytes are the same);
- `in`/`out` of `al`/`ax`/`eax` with `%dx` or an 8-bit port;
- `crc32{b,w,l,q}` of registers, the byte source into a 64-bit
  destination included (a memory source is `asm:operand/form`);
- `addp subp mulp divp sqrtp minp maxp unpcklp unpckhp` `s`/`d`;
- `rdrand`, `rdseed`, `rdpid`, `rd`/`wr` `fs`/`gs` `base`;
- the string instructions `movs stos lods scas cmps` `b`/`w`/`l`/`q`
  without operands (and `movsd` as `movsl`; `stosd`, `lodsd`, `scasd`,
  `cmpsd` are not their names to LLVM), alone or after `rep`/`repe`/
  `repz`/`repne`/`repnz`.

**CPU features.** `llc`'s assembler takes every mnemonic whatever the
function's `target-features` (it assembles `prefetchw`, `crc32b`,
`clwb`, `rdrand` with none), so hod checks no feature for a template.

**Stays refused**, with the census's count of templates that need it:

| form | refusal | templates |
|---|---|---|
| `.cfi_*` in a body (valgrind's `CALL_FN_*`): taken since WP-212, §15 | — | 15 |
| a call or branch to a symbol (`call __bolt_fini_trampoline`): taken since WP-204, §14 | — | 3 |
| `.p2align 3`..`6` (zstd's loop heads, two copies): in the census and taken since WP-218, §18 | — | 34 |
| the control registers (`mov %cr3, %rax`) | `asm:operand/register` | 2 |
| AVX (`vdivpd`) | `asm:mnemonic/v*` | 2 |
| an indirect branch or call (`jmpq *%rdx`, `call *%rax`) | `asm:operand/form` | 2 |
| a symbol in an immediate (`movabsq $__hot_end, %rcx`) | `asm:operand/symbol` | 1 |
| a difference of labels in data (`.long 1f-1b-6`) | `asm:operand/expression` | 1 |
| `lea` with `@gotpcrel` (LLVM 19 and 22 differ) | `asm:operand/specifier` | 0 |
| `inteldialect` | `x64:inline-asm-intel` | 0 |

The x87 register operands (`fsqrt` with `=t`, `fpatan` with `{st}`,
found in 4 templates) were refused before the body is read, as
`asm:constraint/t`, `asm:constraint/st` (§11); since WP-204 `{st}` and
`{st(1)}` are taken (§14). With both, the census's count is 201 of 220
templates (197 bodies of `census_x64.rs`, and the 4 x87 templates).
After WP-212 (§15) `census_x64.rs` counted 212 of 220. WP-218 (§18)
adds zstd's 34 alignment templates, which were counted in the AArch64
census until WP-217 found that they are x86-64 code. All of them
assemble: **246 of 254**.

**Symbol operands (`Ws`).** protobuf's `port.h` (x86-64 Linux, clang 19
and up) keeps a type's default instance alive with
`asm(".reloc ., BFD_RELOC_NONE, %p0" :: "Ws"(&instance))`; front ends
write the constraint `Ws` (kiln) or `^Ws` (clang), and `llc` reads both.
The census's template 220. hod takes:

- **The operand**: a global (`ptr @g`), or a constant `getelementptr`
  of one (`@g` plus a byte offset). Any other value is
  `asm:constraint/s`. A symbol whose name needs quoting, or a private
  one (`llc` prints `.L` names hod's object does not keep), is
  `asm:operand/symbol`. `s` alone reads the same; `Wd` and `s` are
  refused by clang for x86, so no front end writes them.
- **Printing**, as `llc` 22 and 19 print it: `$0` is `$g+8`; `${0:p}`,
  `${0:c}` and `${0:P}` are `g+8` (no offset when it is 0, `g-4` when it
  is negative). `${0:a}` is `asm:modifier/a` (LLVM 19 prints `g-4`,
  LLVM 22 `g-4(%rip)`); other modifiers are refused as for registers.
  `i` with a symbol stays `asm:operand/symbol` (§13 table).
- **`.reloc ., BFD_RELOC_NONE, sym+off`** (or `R_X86_64_NONE`): an
  `R_X86_64_NONE` relocation at the current offset, with the addend, and
  no bytes. Another offset than `.` is `asm:operand/expression`; another
  relocation kind is `asm:directive/reloc`; a local label as the target
  is `asm:operand/symbol`.

The relocations are `llc` 22's and 19's (PIC in
`symbol_fields_are_llcs_relocations`; static too in the record's
check); `leaq ${1:p}(%rip), %rax` with
`Ws` is `llc`'s bytes and relocation (`forced_ws_address`). One
difference: `llc` 22 given clang's `^Ws` for a static build writes the
`.reloc` of `@g` plus 8 without the 8 when the IR is clang's; kiln's `Ws`
and every PIC build keep it, as hod does always.

On AArch64 the same header writes the plain `asm("" :: "r"(p))` (its
`.reloc` path is x86-64 only); the `S` constraint stays
`asm:constraint/s` there.

## 14. x86-64 leftovers: module assembly, calls, x87 operands (WP-204)

**Module-level assembly on x86-64** (`module.rs` `assemble_module_x64`,
`hod_cg::emit::module_asm`). §9's directives, sections, symbols and data
on x86-64, with the x86 spellings: `.word` and `.value` are 2 bytes,
`.align` takes bytes, `.hword`/`.xword` are unknown (`asm:directive/*`,
as to `llvm-mc`); code sections align to 1 and pad with LLVM's nops (at
most 15 bytes each, longest first). Instructions are §10's, plus `ret`,
`leave` and the x87 instructions without operands. Data relocations are
`R_X86_64_64`, `_32`, `_16`; instruction fields are §10.2's, resolved as
LLVM's assembler resolves them: a `%rip` field or a branch to a label
of local binding in the same section is filled in; to such a label
elsewhere it is relocated against the section with the label's offset
in the addend; a GOT field keeps the symbol (a `.L` name is
`asm:symbol`). Branches relax as LLVM's do: short until out of reach or
relocated, iterated to the least fixed point. `asm::check` admits
x86-64; the classify row `module:asm` is on for both targets.

**`.symver name, base@V`** (`@`, `@@`, `@@@`; x86-64, and AArch64 since
WP-212, §15), applied after every module
symbol is defined, as `llvm-mc` 19 and 22 do: a defined name gets an
alias `base@V` (`base@@V`) with its section, value, size, type, binding
and visibility, the name kept; `@@@` renames a defined name `base@@V`;
an undefined name is renamed `base@V` (`@` and `@@@`), so what refers to
it refers to the version; an undefined unreferenced name writes nothing.
An undefined name with `@@` (an LLVM error) is `asm:symbol`; so is a
`.L` name. The symbols' order in `.symtab` is not `llc`'s (sets agree).

**A call or branch to a symbol** in a template or module text (`call
S`, `jmp S`, `jcc S`, `S@PLT`): the near form. The field is
`R_X86_64_PLT32` (addend −4) for a symbol alone or with `@PLT` (plus any
addend), `R_X86_64_PC32` for any other expression (`S+0` included), as
`llvm-mc` writes them. In a template, a symbol the module lacks is
`asm:symbol`; one of local binding is relocated against its section
(`llc` fills the displacement in: the same target). `call S@GOTPCREL`,
`lock call`, `callw` and a constant target stay refused
(`asm:operand/form`, `asm:mnemonic/callw`). **The stack rule**: hod's
frame has `rbp`, `rsp` is 16-byte aligned at every template and nothing
of hod's lives below it, so a call needs nothing more; the callee's
clobbers are the template's to declare (`call_symbol` in
`tests/data/inline_asm` checks the callee's alignment).

**x87 operands** (`hod_x64::barrier::template`): `{st}` (`{st(0)}`) and
`{st(1)}` as inputs, outputs and clobbers, of `float`, `double` and
`x86_fp80`, as LLVM's `X86FloatingPoint` reads inline assembly: the
inputs are pushed deepest first (`fld`), the template runs, the outputs
are popped from `st(0)` upward (`fstp`), then the inputs the template
did not pop (those neither written nor clobbered) are popped. Inputs,
outputs and popped inputs must each be `st(0)` upward; another set is
`asm:constraint/st`. `$N` of one prints `%st` / `%st(1)`. `t` and `u`
are `llc` errors in IR ("couldn't allocate output register") and stay
`asm:constraint/t`, `asm:constraint/u`; `{st(2)}` and deeper are
`asm:constraint/other`; `{st}` of another type is
`x64:inline-asm-operand-type`. Instructions with `%st(i)` operands
(`fadd %st(1), %st`) stay `asm:operand/register`.

**Empty templates** that one barrier effect cannot express (several
outputs, an output of another type than its tied input, a pointer tied
to an `i32` output stored through it, an `i1` input) go through the
template lowering with no bytes: inputs into registers, outputs from
them. Tied integers of different sizes share a register (the output's
width is stored); an `i1` is a byte, masked to bit 0.

**Stays refused**, by name, with the count in the x86-64 corpus's asm
modules (176) and the census (220):

| form | refusal | corpus | census |
|---|---|---|---|
| `.cfi_*` in a template (valgrind's `CALL_FN_*`): taken since WP-212, §15 | — | 0 | 15 |
| a `callbr` with a template (`asm goto`, `jmp ${0:l}`) | `x64:inline-asm` | 2 | 0 |
| the control registers | `asm:operand/register` | 0 | 2 |
| AVX | `asm:mnemonic/v*` | 0 | 2 |
| an indirect branch or call | `asm:operand/form` | 0 | 2 |
| a symbol in an immediate | `asm:operand/symbol` | 0 | 1 |
| a difference of labels in data | `asm:operand/expression` | 0 | 1 |
| x87 `t`, `u` letters (`llc` errors) | `asm:constraint/t`, `/u` | 0 | 0 |
| an x87 register operand in an instruction | `asm:operand/register` | 0 | 0 |
| `.symver` of an undefined name with `@@` | `asm:symbol` | 0 | 0 |

`.cfi_*` in a body needed two changes outside the assembler, made in
WP-212 (§15): remember/restore state in `hod_eh`'s rows, and `cfi_at`
inserting the prologue's rows in offset order.

## 15. Call-frame directives in x86-64 templates (WP-212)

**Where they come from.** `valgrind.h`'s `CALL_FN_*` macros on amd64
(the census's 15 templates): to call a function without redirection,
the template saves `rbp`, points it at the CFA, says so to the unwinder,
moves `rsp` down and calls; then it restores both:

```
movq %rbp, %r15
movq %rax, %rbp            # the CFA, an input
.cfi_remember_state
.cfi_def_cfa rbp, 0
... call ...
movq %r15, %rbp
.cfi_restore_state
```

The CFA input is `__builtin_dwarf_cfa()` (`llvm.eh.dwarf.cfa`), which
hod does not lower (`intrinsic:llvm.eh.dwarf.cfa`, a follow-up); a
function computing it otherwise (`llvm.frameaddress(0)` plus 16, as the
tests do) is lowered in full.

**The directives** (`x64::assemble_with_cfi`; `x64::assemble` still
refuses each as `asm:directive/<name>`): `.cfi_remember_state`,
`.cfi_restore_state`, `.cfi_def_cfa reg, n`, `.cfi_def_cfa_offset n`,
`.cfi_def_cfa_register reg`, `.cfi_offset reg, n`, `.cfi_restore reg`.
Each emits no bytes and becomes a row at its offset in the template
(after layout, so relaxed branches before it count). A register is a
64-bit general register (DWARF numbers by encoding: `rax` 0, `rdx` 1,
`rcx` 2, `rbx` 3, `rsi` 4, `rdi` 5, `rbp` 6, `rsp` 7, `r8`..`r15`
8..15), `rip` (16), `xmm0`..`xmm15` (17..32), with or without `%`, or a
number; another (a 32-bit name) is `asm:operand/register`. A CFA offset
is 0 or more and a `.cfi_offset` a multiple of 8 (the data alignment
factor is −8), else `asm:operand/range`. The wrong count of operands is
`asm:operand/syntax`. `.cfi_adjust_cfa_offset`, `.cfi_rel_offset`,
`.cfi_escape`, `.cfi_startproc`, `.cfi_endproc` and every other
directive stay `asm:directive/<name>`: LLVM tracks the CFA offset across
a remember/restore pair only approximately, and hod states no rule it
does not track.

**The rows** (`hod_x64::barrier::template::emit`): after the bytes and
relocations, each row is added at the template's start plus its offset
with `FnCx::cfi_at`, which keeps rows in offset order (`spec/codegen.md`
§9.3), so the prologue's rows, added at the end of the function, stay
ahead of them. `hod_eh` writes `RememberState`/`RestoreState` as
`DW_CFA_remember_state`/`DW_CFA_restore_state` (`spec/eh.md` §4.3); the
core's restatement before the shared trap applies them as a stack. The
template's rows hold until the next row, as written: a template that
leaves a rule changed at its end changes it for the code after, as
`llc`'s does. `.debug_frame` (a `nounwind` function with debug
information) gets the same rows.

**Checked** (`crates/hod_x64/tests/inline_asm_cfi.rs`): the
`llvm-dwarfdump` instructions of `.eh_frame` and `.debug_frame` inside
the template (found by a marker `nopl` in `.text`) equal `llc` 19's and
22's, hod plain and stressed, for valgrind's shape, every directive and
nested pairs; a C++ exception thrown by a function the template calls is
caught by the caller of the function holding the template, under every
linker, dynamic and static (without the rows it aborts); `gdb` stopped
in the callee walks back through the template to `main` (without the
rows it does not). `llc`'s own build of such a template is compared,
not run: it does not keep `rsp` 16-byte aligned at the template.

**AArch64**: no template of the AArch64 corpus uses a call-frame
directive (nor of the x86-64 corpus; the census is x86-64's), so they
stay `asm:directive/<name>` there.

The x86-64 census: 212 of 220 bodies assemble (197 before).

**`asm goto` with a template** on x86-64 (a `callbr` whose template is
not empty) stays refused as `x64:inline-asm` (§11, §14's table: 2
corpus modules); it is WP-212's second part, a follow-up.

**`.symver` on AArch64** (`module.rs`, `hod_cg::emit::module_asm`):
§14's `.symver`, read and applied the same way on AArch64 (the code was
already the target's; only the directive was x86-64's). An undefined
name with `@@` stays `asm:symbol`. `emit_module_asm.rs`'s `symver` case
(`@`, `@@`, `@@@`, defined, weak, hidden, internal, undefined and
unreferenced names) has `llc` 19's and 22's sections, symbols (as sets)
and relocations; module text without `.symver` reads as before.

## 16. AArch64 prefetch, and the census of AArch64 templates (WP-216)

**`prfm` and `prfum`** (`a64/prefetch.rs`, `encode.rs`
`prefetch_insn`), in inline bodies and module text alike. `hod_mc_a64`
has no prefetch encoder (code generation writes none); the words are
built in `prefetch.rs` from the load/store-register classes with `size`
3 and `opc` 2, the operation in `Rt`, as `sysreg.rs` builds `mrs`/`msr`.

- **The operation**: one of the 24 names `llvm-mc` 19 and 22 both
  accept, in any case: `pld`, `pli`, `pst` × `l1`, `l2`, `l3`, `slc` ×
  `keep`, `strm` (values 0..23; `slc` is 6, 7, 14, 15, 22, 23 and needs
  no feature), or `#imm5` (`#` optional, a constant expression,
  0..31). Another name (or a register) is `asm:operand/option`; a value
  outside 0..31 is `asm:operand/range`. The two LLVM majors agree on
  every name.
- **`prfm` addressing**: `[xn|sp]`; `[xn, #imm]` (the `#` optional, as
  RocksDB's `[${0:x},$1]` with `I` prints) scaled by 8, 0..32760;
  `[xn, xm{, lsl #0|#3}]`, `[xn, wm, uxtw|sxtw {#0|#3}]`,
  `[xn, xm, sxtx {#0|#3}]` (another amount `asm:operand/range`, a W
  index without an extension or an X index with `uxtw`/`sxtw`, or `sp`
  as the index, `asm:operand/register`); a label or symbol (the literal
  form, `R_AARCH64_LD_PREL_LO19` for a symbol as `ldr`'s). Write-back
  and post-index are `asm:operand/form`; `:lo12:` stays
  `asm:operand/specifier`. With the values 24..31 the register form is
  the word LLVM 22's disassembler calls `rprfm`: the same bytes.
- **An offset the scaled form does not take** (negative, or not a
  multiple of 8) but -256..255 is `prfum`, as `llvm-mc` 22 writes it;
  `llvm-mc` 19 rejects that text ("index must be a multiple of 8").
  kiln writes for LLVM 22, so hod follows 22 (`llvm_mc.rs`
  `ACCEPTED_22` holds both facts). Anything else is
  `asm:operand/range`.
- **`prfum`**: `[xn|sp{, #imm}]`, -256..255; a register offset or a
  label is `asm:operand/form`.

**Also taken** (fixed forms the census of templates found or the row
named): `hlt #imm16` (`0xd440_0000`, the exception class of `brk`);
`xpaclri` (`HINT #7`); `bic`, `bics`, `orn`, `eon` of an immediate,
`and`, `ands`, `orr`, `eor` of the inverted immediate (mimalloc's
`bic x, x, #7`, valgrind's `bic sp, x21, #15`), as LLVM writes them.

**The census** (`crates/hod_asm/tests/census_a64.rs`; how it was taken
is the record's): every `asm(...)` of the real projects built so far
(`.build/oss*/src`), of `/usr/include` (glibc's `fpu_control.h`,
valgrind's headers), and the templates of the saved kiln-built modules
whose body `llvm-mc` 22 reads as AArch64 after its operands are
substituted. 140 of 211 templates assemble (116 before WP-216); every
accepted body is `llvm-mc` 22's and 19's words. The kernel's arm64 UAPI
headers and musl (not installed) have no AArch64 template. WP-217
corrected the count to 177 (§17): 143 assemble.

**Stays refused** after WP-217, with the census's count of templates:

| form | refusal | templates |
|---|---|---|
| `.arch_extension crypto` (before AES in NEON) | `asm:directive/arch_extension` | 1 |
| SVE (`cntb`) | `asm:mnemonic/cntb` | 1 |
| an instruction as data (`.inst 0xd4200000`) | `asm:directive/inst` | 1 |
| `adrp` | `asm:mnemonic/adrp` | 0 |

The 31 NEON templates (`fmla`, `umlal`, `pmull`, `pmull2`, `fcmeq`,
`fcvtzs`/`fcvtzu`/`fcvtns` with arrangements) and the structure loads
(`ld1`, none in the census) are §20's since WP-226: 174 of 177 templates
assemble.
`adrp sym` alone writes `R_AARCH64_ADR_PREL_PG_HI21`, a reference kind
neither caller has, and is of use only with `:lo12:sym` (a relocation
specifier): no census template writes either, so both wait.

## 17. Alignment directives in AArch64 templates; scalar SIMD forms (WP-217)

**The directives** (`align.rs`, `a64::assemble_at`): `.p2align n`,
`.align n` (AArch64: a power of two, as `.p2align`), `.balign n`, in any
case, each `{, fill}{, max}` as `llvm-mc` 19 and 22 read them:

- `n` is a constant expression: 0..16 for the power forms; for
  `.balign` a power of two up to 65536 (0 means 1). Another value is
  `asm:operand/range` (`.balign 3` is an error to `llvm-mc`; hod keeps
  alignments at most 64 KiB).
- `fill` is -128..255, written as that byte; empty (`4,,8`) means none.
  Another value is `asm:operand/range`; `.p2align 4,` (a trailing empty
  operand) is `asm:operand/syntax`; a fourth operand
  `asm:operand/form`.
- `max` below 1 is `asm:operand/range` (an error to `llvm-mc`); at or
  above the alignment it has no effect (`llvm-mc` warns). When the
  padding would be more than `max` bytes, none is written.
- The padding without a fill is `nop` words (code is a multiple of 4
  bytes). With a fill byte `f`, it is words of four `f`s; no `$d` mapping
  symbol marks it (`llvm-mc` writes none).
- **Fill 0** is the one difference between the two majors: `llvm-mc` 19
  writes `nop`s (it reads a fill equal to the target's no-op byte as code
  alignment), 22 writes zero words. hod writes 22's (kiln's LLVM;
  `llvm_mc.rs` `fill_zero_is_llvm_22_s`).
- `.balignw`, `.balignl`, `.p2alignw`, `.p2alignl` and the other
  directives stay `asm:directive/<name>`.

Module text (§9) takes the same operands without `max`.

**Where the boundary is counted.** `llvm-mc` pads to the section
offset; the section's alignment is raised to the directive's even when
`max` skips the padding. In code, hod compiles each function at offset 0
of its own buffer and `add_func` places it at a multiple of its
alignment. A template whose body aligns is assembled once at 0 (its
size, its refusals) and again at its offset in the function's code;
the function's alignment is raised to the body's largest
(`FnCx::raise_align`), so the boundary counted from the function's
start is a boundary of the section.

- With one section per function (`-function-sections`) the bytes are
  `llc`'s: each template's words equal `llvm-mc`'s at the same section
  offset, in hod's object and in `llc` 19's and 22's, and each
  `.text.<f>` is aligned alike (`inline_asm_align.rs`).
- In one `.text`, `llc` pads inside the function from the section
  offset; hod puts the padding before the function (it starts at a
  multiple of the alignment). The bytes differ where the padding is; the
  instruction after each directive is on its boundary in both, and the
  section's alignment is the same.
- **The size bound** holds: the body's size at its offset is the one
  written, and a branch the larger code puts out of reach retries the
  function in far mode, as for any code.
- **Shrinking** (`spec/codegen.md` §5.6.15) removes bytes after the
  code exists. Bytes removed before an aligned template would move it off
  its boundary: the function is refused with `limit:aligned-code-moved`.
  On AArch64 only a probed frame's prologue (`"probe-stack"`) is cut;
  kiln writes no `probe-stack` in the modules built so far.
- **Rows**: line, `eh_frame` and debug rows are offsets in the
  function's code, which holds the padding; none is written for the
  padding itself (`llc` writes none either).

**Scalar SIMD** (`encode.rs` `scalar_simd`), on `s` and `d` registers
of the same size (feature `fp`, as the other forms; `llvm-mc` needs
`neon` for them, which every AArch64 Linux target has): `fcvtzs`,
`fcvtzu` (`0x5ea1_b800`, `0x7ea1_b800`), `fcmeq` register
(`0x5e20_e400`, `rm` at bit 16) and against `#0.0` or `#0`
(`0x5ea0_d800`); bit 22 is the size. Another immediate is
`asm:operand/fp-immediate`, registers of different sizes or an `h`
register `asm:operand/register`, a vector arrangement
`asm:operand/vector`. Since WP-226 (§20) these need `neon`, as
`llvm-mc`'s, and `fcvtzs`/`fcvtzu` are §20's two-register scalar forms
(the same words; `h` registers with `fullfp16`).

**The census** (`census_a64.rs`, §16) is corrected: the 34 `.p2align`
templates are zstd's, all under `#if defined(__GNUC__) &&
defined(__x86_64__)` (kiln's AArch64 zstd modules have no template):
they are x86-64 templates, out of the AArch64 census. upb's
`encode_longvarint` (grpc's and protobuf's copies) aligns its jump
table with `.p2align 3` under `#ifndef __SEH__` inside the string; its
row now holds the directive and assembles. The three scalar SIMD rows
assemble. **143 of 177** templates assemble (140 of 211 before).

**x86-64.** zstd's 34 templates (`.p2align 3` to `6`, in
`ZSTD_compressBlock_*` and `ZSTD_decompressSequences*`) are x86-64
templates; since WP-218 they assemble there (§18), with padding counted
after the function shrinks.

## 18. Alignment directives in x86-64 templates (WP-218)

**The directives** (`align.rs`, `x64::assemble_aligned`) are §17's:
`.p2align n`, `.balign n` and `.align n`, in any case, each with
`{, fill}{, max}`, read as `llvm-mc` 19 and 22 read them. On x86-64 ELF
`.align` counts bytes, as `.balign` does, and it must be a power of two
(`.align 3` is an error to `llvm-mc`). The refusals:

- `.p2align 17`, `.balign 3` and `.align 131072` are
  `asm:operand/range`. So are a fill outside -128..255 and a `max` below
  1.
- `.p2align 4,` is `asm:operand/syntax`. A fourth operand is
  `asm:operand/form`, and so is a `lock` before a directive.
- `.p2alignw`, `.p2alignl`, `.balignw` and `.balignl` stay
  `asm:directive/<name>`.
- When the padding needed is more than `max`, none is written.

**The padding.** Without a fill, the padding is `nop`s of at most the
function's longest length, longest first: `hod_mc_x64::nop`'s forms,
with the 11- to 15-byte ones using extra `66` prefixes
(`align::fill_x64`). With a fill byte, every byte is that byte.

- **Fill 0x90, written positive**: `llvm-mc` 19 writes `nop`s, 22
  writes `0x90` bytes. hod writes 22's (kiln's LLVM), as with AArch64's
  fill 0 (§17).
- Fill 0 is zeros in both majors. Fill -112 is `0x90` bytes in both.

**The longest `nop` follows the tuning** (`hod_x64::tuning`, measured
with `llc` 19 and 22 for each of the 131 CPUs that 22 lists; see that
module for the tables):

- **The tuning CPU** is the `tune-cpu` attribute when there is one,
  even an empty one. Otherwise it is `target-cpu`, or `-mcpu` when the
  function has no `target-cpu`, with `x86-64` read as `generic`.
  Otherwise there is none.
- **No CPU, or an empty one**, is LLVM's default `i586` tuning: 10
  bytes.
- **The tuning features.** Each CPU has at most one of
  `fast-7bytenop`, `fast-11bytenop` and `fast-15bytenop`. A `+name` or
  `-name` in `target-features`, then in `-mattr`, turns it on or off.
- **The longest `nop`** is 7 bytes with `fast-7bytenop`, else 15 with
  `fast-15bytenop`, else 11 with `fast-11bytenop`, else 10.
- kiln writes `"target-cpu"="x86-64" "tune-cpu"="generic"`, which gives
  15.
- `diamondrapids`, `novalake` and `wildcatlake` are 15 to LLVM 22. LLVM
  19 does not know them and tunes as for no CPU. hod follows 22.
- **Another CPU name** is one hod has not measured. A template that pads
  with `nop`s in such a function is refused as `x64:align-tune-cpu`. One
  with a fill byte, or with no padding, is not.
- `-mcpu` and `-mattr` change nothing else in hod's code
  (`spec/abi-x64.md` §1.3).

One quirk of `llc` is not hod's. `llc` caches a subtarget under its
`target-cpu`, `tune-cpu` and features joined with no separator. So a
function with `target-cpu` `skylake` and an empty `tune-cpu` is given
the subtarget of an earlier function whose `tune-cpu` is `skylake`, and
gets that function's 15 instead of 10. hod applies the rule to each
function on its own. The two agree whenever the joined strings are
distinct, which is always the case with kiln's attributes.

**Where the boundary is counted.** It is counted as on AArch64 (§17):
from the start of the function. The function is aligned to the largest
alignment of its templates (`FnCx::align_code` raises it), so a boundary
counted from the function's start is also a boundary of the section.

- **With `-function-sections`** the bytes are `llc`'s. Each template's
  bytes equal `llvm-mc`'s at the same section offset with the
  function's tuning. This holds in hod's objects (plain, stressed, far)
  and in `llc` 19's and 22's, and each section has `llc`'s alignment.
- **In one `.text`**, `llc` pads inside the function from the section
  offset, while hod puts the function itself on the boundary. hod's
  placement was kept, for three reasons:
  - hod compiles each function at offset 0, in parallel, before its
    place in the section is known.
  - The aligned instruction's address modulo the alignment is the same
    either way.
  - The section's alignment is the same.

  The tests show the bytes are still `llvm-mc`'s at the template's
  section offset in both objects. The bytes hod adds before a function
  are the section's padding between functions, which is never executed.

**Shrinking** (`spec/codegen.md` §5.6.15, "Alignment points"). x86-64
code shrinks after it is complete: the prologue is cut and branches are
relaxed. A template is therefore emitted with each directive's largest
padding, `min(align - 1, max)` bytes. Shrinking then keeps what the
directive's final offset needs, cuts the rest and fills the kept bytes
again. The relaxed branches are a fixed point that takes the padding
into account. Every table of code offsets moves with the cut, as for
any cut: labels, fixups, relocations, line, CFI, call-site and
debug-call rows, and islands. x86-64 writes no jump tables.

- **No row is written inside the padding.** A template's `.cfi_*` row
  after a directive (§15) moves with the code after it.
- **A branch across a directive.** A branch of the template to a label
  on the other side of a directive that may pad is refused as
  `asm:align-crossed`. Its distance would depend on the final padding,
  which the template's own relaxation cannot see. A branch over a
  directive that never pads, such as `.p2align 0`, is taken, and so is
  one beside a directive. No template of the census branches across a
  directive.
- **The far mode** (§6.3) relaxes nothing. Padding is still cut to what
  each offset needs.
- **AArch64** keeps §17's rule: it refuses (`limit:aligned-code-moved`)
  rather than recomputing the padding, since only a probed prologue is
  ever cut there.

**Evidence.**

- `crates/hod_asm/tests/llvm_mc_x64_align.rs`: 21 bodies, among them
  zstd's run, max-skip, fills, case forms, and a branch that is short or
  near depending on the padding. Each was assembled at 64 start offsets
  with the four `nop` tables, and every result equals `llvm-mc` 22's and
  19's bytes.
  - Reserve-then-cut (the shrinking's arithmetic) gives the same bytes.
  - The refusals are checked by name.
  - Fill 0x90 equals 22's.
- `crates/hod_x64/tests/inline_asm_align.rs` checks these against
  `llc` and `llvm-mc`:
  - The census-like templates, with `-function-sections` and in one
    `.text`.
  - The rule for every CPU, and for `-mcpu`/`-mattr`.
  - The refusal of an unmeasured tuning.

  It also runs zstd-shaped seeds: a loop after `.p2align 6`/`5`/`4,,10`/
  `3` in a probed frame with a stack protector, a VLA, branches before
  and after, and an invoke after the padding. With these seeds it checks:
  - The programs, linked with bfd, lld and mold, dynamic and static,
    and run.
  - `llvm-dwarfdump --verify`, and line and unwind rows on instruction
    boundaries.
  - gdb's backtrace and stepping through the padding.
- The record (`spec/landed/asm/WP-218.md`) has the counts. It also has
  zstd's two modules built by kiln with `hod --strict`, whose loop heads
  are on their boundaries as `llc`'s are.

## 19. Constraints with alternatives (WP-219)

A constraint string with `|` (C++'s `,`) gives every operand the same
number of alternatives. Which alternative is used, and which output is
tied to which input, is LLVM's `TargetLowering::ParseConstraints` and
`InlineAsm::ConstraintInfo::selectAlternative`. hod follows both on
x86-64 (`hod_x64::barrier`, `barrier::template::decide`) and AArch64
(`hod_a64::barrier`, `inline_asm::decide`). The shared part is
`hod_cg::asm_constraints::Ties`. Every rule below was measured against
`llc` 19 and 22 at `-O0` (AArch64 also with `-global-isel=0
-fast-isel=0`); the two majors agree on every case.

**Choice.** Each alternative's weight is the sum of its operands'
weights. The first alternative with the highest sum wins. When every
alternative has an invalid operand, alternative 0 is used.

- An operand's weight is the best of its codes: invalid -1, a fixed
  register or a digit 0, a register class 1, memory 2, an immediate 3.
- On x86, every generic code weighs 0 for an integer operand.
- `x` weighs 1 for a 128-bit value, or for a 256-bit value with AVX.
  `v` also weighs 1 for a 512-bit value with AVX-512. Otherwise `x` and
  `v` are invalid.
- An indirect operand (`*`) is weighed by its pointer: `r`, `x` and `w`
  are invalid, and `m` weighs 2. A direct output has no value yet, so
  every code weighs 0 for it.

**Ties.**

- A digit in an input *without* alternatives ties the output at the top
  level.
- An output *with* alternatives keeps only the tie of the chosen
  alternative, so such an input's digit is dropped.
- A digit in an input with alternatives ties the output in that
  alternative only.
- Hence `=*imr|x,0` is untied: the output is memory, and the template
  reads and writes the variable itself. The input's value is not stored.
- `=*imr|x,0|0` is tied: the output is in a register, and the input is
  stored through the pointer after the template.
- A tied input whose output is in memory is placed in that memory. Its
  own value is not read.

C++'s `"+g,x"`, as kiln writes it (`=*imr|x,0`), therefore becomes the
variable's memory for every type. Eigen's `EIGEN_OPTIMIZATION_BARRIER`
compiles to nothing, as `llc` builds it, and so does AArch64's
`"+g,w"` (`=*imr|w,0`).

**Measured** (x86-64; `tests/asm_alternatives.rs`
`alternatives_are_llcs_choice` compares the operand class, memory,
xmm, ymm or gpr, of hod's and `llc`'s instruction):

| constraints | integer, `float`, `double`, pointer | 128-bit vector, `i128` |
|---|---|---|
| `=*imr,X,0` (indirect, any order) | memory | memory |
| `=*x,R,0` (both invalid) | xmm (the first) | xmm |
| `=*r,X,0` | gpr (the first) | gpr: `llc` cannot allocate it |
| `=r,X,0`, direct | gpr | gpr: `llc` cannot allocate it |
| `=x,R,0`, direct | xmm | xmm (ymm for 256 bits with AVX) |
| input `imr,X`, `rm,X`, `m,X`, `X,m` | memory | memory (`i128`: xmm) |
| input `r,X` | gpr | xmm |
| input `X,r` | gpr for integers, else xmm | xmm |

In the table, `,` stands for IR's `|`: `X` is `x`, and `R` is `r`.
hod's choice is `llc`'s in every row. Two placements were not taken
before and still are not:

- An `i128` in an SSE register in a template is
  `x64:inline-asm-operand-type`.
- A vector in a general register (`=*r|x,0`), which `llc` cannot
  allocate, is `x64:inline-asm-operand`, as in base.
AArch64 (`hod_a64/tests/asm_alternatives.rs`):

- `=*imr|w,0` is memory, and `=*imr|w,0|0` is a register.
- Input `imr|w` is memory.
- Input `r|w` is a general register for integers and pointers, and a
  vector register for floating point and vectors.

**Refused by name** (not measured, or `llc` rejects it):

- `x64:inline-asm-alternative` / `a64:inline-asm-alternative`. Each of
  these is refused:
  - an operand with fewer alternatives than the constraint string, but
    more than one;
  - with alternatives, a tie some alternative puts in effect between
    operands of different types (`=*r|m,0|0` with `i32` and `i64`). The
    dropped tie of `=*r|m,0` with different types (Benchmark's
    `DoNotOptimize` of a struct) is not one: it compiles to memory, and
    `llc` writes nothing;
  - on x86, a 256-bit operand when the AVX level is only assumed, or a
    512-bit one when AVX-512 is not known.
- `asm:constraint/tied`, which `llc` reports as "failed to parse
  constraints". An empty template on x86-64 gives
  `x64:inline-asm-constraint` instead. Each of these is refused:
  - an input with alternatives naming an output without them
    (`=*imr,0|0`);
  - a digit naming an input or a missing alternative;
  - two inputs tied to one output.
- A direct output untied with a memory code (`=rm|x,0`) crashes `llc`.
  hod's behaviour there is unchanged.

**Census.** These are every `,` alternative found in the real sources
built so far (`.build/oss*/src`) and in `/usr/include`, with the
occurrences and the files that hold them:

- Google Benchmark's `DoNotOptimize`/`ClobberMemory`: `+m,r` 17 in 5
  files, `r,m` 13 in 6, `+r,m` 9 in 5, and `i,r,m` 1 (protobuf's copy).
- Eigen: `+x,m` 3 and `+w,m` 3 (`GeneralBlockPanelKernel.h`). The
  `Macros.h` barriers are `+g,x`, `+g,w`, `+r,w`, `+r,v` and `+r,v,wa`
  (1 each).

In the saved kiln-built modules (`.build/corpus`, `corpus-x64`,
`findings`), these occur, all with empty templates:

- Benchmark's `=*r|m,0` (8), `=*m|r,0` (4) and `r|m` (4), all in the
  conformance entry `dcl.asm`.
- Eigen's `=*imr|x,0` (2, in findings).

`+r,v` and `+r,v,wa` are Eigen's PowerPC barriers. `+r,w` is its
AArch64 barrier for GCC 5 and older. None of the three is built for
x86-64 or AArch64 here.

- x86-64 code shrinks: every `-O0` branch is relaxable and the prologue
  is cut (§5.6.15), so nearly every function with such a template would
  be `limit:aligned-code-moved`. Taking them needs padding counted after
  the shrinking, or a shrinking that keeps it.
- The padding's `nop`s follow the function's tuning: `llc` 19 and 22
  write 15-byte `nop`s for `tune-cpu` (else `target-cpu`) `generic`,
  `x86-64`, `x86-64-v2`..`-v4` (and `skylake`), 10-byte ones with no
  CPU (`llc`'s default `i586` tuning) or `tune-cpu` `i586` (measured
  with 31 bytes of padding: 15, 15, 1 against 10, 10, 10, 1). kiln writes
  `"target-cpu"="x86-64" "tune-cpu"="generic"`: the 15-byte table of
  `hod_mc_x64::nop`.
- Fill `0x90` differs as fill 0 does on AArch64: `nop`s to 19, `0x90`
  bytes to 22.

## 20. NEON in AArch64 templates (WP-226)

**Operands** (`a64/operand.rs`), any case:

- `v<n>.<T>` with `T` one of `8b` `16b` `4h` `8h` `2s` `4s` `1d` `2d`
  `1q` (and `4b`, `2h`, which no form here takes);
- `v<n>.<t>[i]` (`t` one of `b` `h` `s` `d` `q`, `i` 0..255);
- a list `{v<a>.<T>, v<b>.<T>, ...}` or `{v<a>.<T>-v<b>.<T>}` of one
  to four consecutive registers (counted modulo 32, `v31` then `v0`)
  of one arrangement, or of one element followed by `[i]`.

Another vector form (no arrangement, `.3s`, a gap in a list, mixed
arrangements, five registers) is `asm:operand/vector` for the
instruction that meets it.

**Mnemonics** (`a64/neon.rs`), on vector operands, with feature
`neon`. Each word equals `llvm-mc` 19's and 22's.

- **By element**: `fmla`, `fmls`, `fmul` and `fmulx`.
  - `.4s`/`.2s` by `.s[0..3]` takes `v0`..`v31`.
  - `.2d` by `.d[0..1]` takes `v0`..`v31`.
  - `.8h`/`.4h` by `.h[0..7]` needs `fullfp16`. Its element register
    must be `v0`..`v15`; `v16` and above are `asm:operand/register`.
  - An index out of range is `asm:operand/range`; another lane view is
    `asm:operand/vector`.
- **Three same, floating point**:
  - `fmla` and `fmls`;
  - the whole class of `hod_mc_a64`'s `Fp3`: `fadd` `fsub` `fmul`
    `fdiv` `fmax` `fmin` `fmaxnm` `fminnm` `fmulx` `frecps` `frsqrts`
    `fabd` `facge` `facgt` `faddp` `fmaxp` `fminp` `fmaxnmp` `fminnmp`;
  - `fcmeq`, `fcmge` and `fcmgt`; `fcmle` and `fcmlt` of two registers
    are `fcmge` and `fcmgt` with the sources swapped, as `llvm-mc`
    writes them.
- **Compares with zero** (`#0.0` or `#0`): `fcmeq` `fcmge` `fcmgt`
  `fcmle` `fcmlt`. Another immediate is `asm:operand/fp-immediate`.
- **Two-register, floating point**: the class of `Fp2`:
  - `fcvt{n,m,a,p,z}{s,u}`;
  - `scvtf` `ucvtf` `frecpe` `frsqrte` `fabs` `fneg` `fsqrt`;
  - `frint{n,m,p,z,a,x,i}`.

  The vector arrangements are `.2s`, `.4s`, `.2d`, and `.4h`/`.8h` with
  `fullfp16`. The scalar forms are `s0, s1`, `d0, d1`, and `h0, h1`
  with `fullfp16`; every form needs `neon`. Left out: `frint32*`,
  `frint64*` (`FEAT_FRINTTS`) and `frecpx`, which are
  `asm:mnemonic/<m>`. The fixed-point forms (`fcvtzs v0.4s, v1.4s,
  #3`) are `asm:operand/form`.
- **Three different**: every operation of `Long3` and its `2` form:
  - `saddl` `uaddl` `ssubl` `usubl` `sabal` `uabal` `sabdl` `uabdl`;
  - `smlal` `umlal` `smlsl` `umlsl` `smull` `umull`;
  - `sqdmlal` `sqdmlsl` `sqdmull`;
  - `addhn` `raddhn` `subhn` `rsubhn`;
  - `pmull`.

  Rules:
  - The narrow element is the sources' (the result's for the
    high-narrowing forms): 64 bits of it, or 128 bits for the `2` form.
  - The other operands must be twice as wide; anything else is
    `asm:operand/vector`.
  - `pmull`/`pmull2` of `1d`/`2d` into `1q` needs `aes`.
- **Structure loads and stores**:
  - `ld1`..`ld4` and `st1`..`st4` of multiple structures (`ld1` takes
    one to four registers, `ld<n>` exactly `n`; `1d` only for `ld1`);
  - of one lane, with an element list and an index;
  - `ld1r`..`ld4r`.

  The address and post-index:
  - The address is `[xn|sp]`.
  - Post-index is `#<bytes moved>` or an X register (`rm` 31 is the
    immediate form).
  - `xzr` or a W register is `asm:operand/register`.
  - Another immediate is `asm:operand/range`.
  - An offset or pre-index write-back is `asm:operand/form`.

**Neighbours taken because they share an encoding class** with a census
form (`fmla`, `fcvtzs`/`fcvtzu`/`fcvtns`, `fcmeq`, `umlal`, `pmull`/
`pmull2`):
- the rest of `Fp3`, `Fp2` and `Long3`;
- `fmls`/`fmul`/`fmulx` by element;
- the other `fcm*` compares;
- the structure forms beyond `ld1`.

Each is in `llvm_mc.rs`'s 182 NEON bodies. `mov v0.16b, v1.16b` (an
`orr` alias) and the integer vector classes (`add v0.4s`, `orr`,
`ext`, ...) stay out: a vector operand of a general-register mnemonic
is `asm:operand/register`; a NEON-only mnemonic outside these classes is
`asm:mnemonic/<m>`.

**Features**: `Features::from_attribute` reads `target-features` in
order, with LLVM's implications as `llvm-mc` 19 and 22 apply them
(measured):

| item | sets | clears |
|---|---|---|
| `+neon` | `neon`, `fp` | |
| `+aes`, `+crypto` | `aes`, `neon`, `fp` | |
| `+fullfp16`, `+fp16fml`, `+sve` | `fp16`, `fp` | |
| `-fp-armv8` | | `fp`, `neon`, `aes`, `fp16` |
| `-neon` | | `neon`, `aes` |
| `-aes` | | `aes` |
| `-fullfp16` | | `fp16` |

`+v8.2a`, `+v9a` and `+sha2` imply neither `aes` nor `fp16`. The base
(no attribute) has `fp` and `neon`, as every AArch64 Linux target.
kiln writes `+fp-armv8,+neon,...`, and adds `+aes` for
`target("aes")`, `+fullfp16` for `target("fp16")`, and `+crypto` for
`-march=...+crypto`. A form whose feature is missing is `asm:feature`.

**Operands of the SIMD&FP classes** (`hod_a64::inline_asm`), as `llc`
19 and 22 allocate and print them. The two majors agree on every cell
measured.

- **`w`**:
  - takes values of 16, 32, 64 or 128 bits: `half`, `bfloat`,
    `float`, `double`, `fp128`, `i16`, `i32`, `i64`, `ptr`, and vectors
    of 8 or 16 bytes;
  - `i8`, `<1 x i8>` and `<3 x float>` are `llc` errors ("couldn't
    allocate"), and `a64:inline-asm-operand-type` in hod;
  - `llc` also places `i128` and vectors of 2 or 4 bytes (`<2 x i8>`,
    `<2 x half>`); hod refuses them as `a64:inline-asm-operand-type`
    (no such value lives in a SIMD&FP register in hod's code).
- **`x`**: 128-bit values only (`v0`..`v15`).
- **`y`**: no value hod holds (SVE's types): `asm:constraint/y`.
- **Modifiers**:
  - none prints `v<n>`;
  - `b` `h` `s` `d` `q` print that view;
  - `w` and `x` print the view of the value's size (`h`, `s`, `d`, `q`);
  - `a` prints `[s0]` and `z` prints `z0` to `llc`; hod refuses them
    (`asm:modifier/a`, `asm:modifier/z`);
  - `v`, `c`, `n` and `t` are `llc` errors and `asm:modifier/<m>` in
    hod.
- **Clobbers** `~{v8}`..`~{v15}`, in any view (`q9`, `d10`, `s11`):
  the function saves and restores the low 64 bits the procedure call
  standard keeps, as for any clobber of a callee-saved register.

**The census** (`census_a64.rs`): the 31 NEON templates assemble.

| row | projects | templates |
|---|---|---|
| `fmla v8.4s, v9.4s, v10.s[0]` | Eigen | 8 |
| `fcvtzs`, `fcvtzu` (`.4s`, `.2d`, `.4h`, `.8h`) | highway | 10 |
| `fcmeq v8.8h, ...` | highway | 5 |
| `fcvtns v8.4s, v9.4s` | OpenCV | 1 |
| `umlal v8.2d, v9.2s, v10.2s` | xxhash (zstd's copies) | 3 |
| `pmull`, `pmull2` | abseil (and grpc's copy) | 4 |

**174 of 177** templates assemble (143 before). Three stay refused, one
template each:
- `.arch_extension crypto` (`asm:directive/arch_extension`);
- `cntb` (SVE, `asm:mnemonic/cntb`);
- `.inst` (`asm:directive/inst`).
