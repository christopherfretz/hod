# `hod_asm`: assembly text in IR

LLVM IR carries assembly text in two places: the template of an
inline-assembly callee (`call`, `invoke`, `callbr` of `asm "..."`) and
module-level `module asm "..."`. `llc` hands both to its integrated
assembler. hod writes no assembly text and runs no assembler: `hod_asm`
parses the text and encodes each instruction itself, through
`hod_mc_a64`'s checked encoders, and refuses what it does not understand
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
registers (`v0.4s`, `v0.s[1]`) and register lists parse so that an
instruction refuses them as `asm:operand/vector`. Immediates `#e` or `e`;
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
  `orn` `eon` `tst` `mvn`, every immediate, shifted and extended form;
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
- exclusives and ordered: `ldxr` `ldaxr` `stxr` `stlxr` (and `b`/`h`),
  `ldar` `stlr` (and `b`/`h`), `ldxp` `ldaxp` `stxp` `stlxp`, `clrex`;
- LSE (feature `lse`): `cas` `swp` `ldadd` `ldclr` `ldeor` `ldset`
  `ldsmax` `ldsmin` `ldumax` `ldumin` with every ordering suffix and size;
- system: `nop` `yield` `wfe` `wfi` `sev` `sevl` `hint` `isb` `dmb`
  `dsb` (every option), `svc` `brk` `udf`, `mrs`/`msr` of `nzcv`
  `fpcr` `fpsr` `tpidr_el0` `cntvct_el0` (and their `s3_...` names),
  `bti`, `paciasp` `pacibsp` `autiasp` `autibsp`;
- scalar floating point (feature `fp`): `fmov` (register, general to
  and from FP, `#imm8`), `fabs` `fneg` `fsqrt` `frint*`, `fadd` `fsub`
  `fmul` `fdiv` `fnmul` `fmax` `fmin` `fmaxnm` `fminnm`, `fmadd`
  `fmsub` `fnmadd` `fnmsub`, `fcmp` `fcmpe` (`#0.0`), `fccmp` `fccmpe`
  `fcsel` `fcvt` `scvtf` `ucvtf` `fcvt{n,a,p,m,z}{s,u}`.

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
guesses); any directive is `asm:directive/<name>`. A reference to a
symbol is returned as a `Ref` (offset, kind, name, addend) for the
caller.

## 6. Inline assembly in code (`hod_a64::inline_asm`)

The lowering is `spec/codegen.md` §5.5.3 "WP-103". In short:

- **Features** come from the function's `target-features`
  (`Features::from_attribute`): `+lse`, `+crc`, an architecture from
  `v8.1a` on (both), `-fp-armv8`.
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
  refused table checks each name; feature gating.
- Unit tests in each module: statements, expressions, substitution,
  local labels, operand printing, refusal-name interning.
- `crates/hod_a64/tests/inline_asm.rs`: the lowering (§6), run against
  `llc`; the template's words appear in the code as `llvm-mc` writes
  them.

## 8. Refusals

`asm:mnemonic/<name>`, `asm:directive/<name>`, `asm:constraint/<code>`,
`asm:clobber/<name>`, `asm:modifier/<m>` carry a part of the input,
lower-cased; a part that is not a short plain word (or past 4096
distinct names) is `other`. The fixed names: `asm:operand/{syntax, form,
range, register, sysreg, option, vector, fp-immediate, specifier,
expression, symbol, label, reach, literal-pool}`, `asm:label`,
`asm:lexical`, `asm:feature`, `asm:template`, `asm:dialect`,
`asm:section`, `asm:symbol`.

## 9. Module-level assembly (planned)

Not yet assembled: module text that is not only symbol directives stays
`module:asm` (WP-075's `asm_text.rs` reads the directives-only form).
The plan is `PARKED.md` of WP-103 and the backlog row that follows it:
`hod_asm::module` reads statements into section fragments (labels,
`.globl`/`.weak`/`.hidden`/`.type`/`.size`, `.text`/`.data`/`.section`/
`.pushsection`/`.popsection`/`.previous`, `.byte`..`.quad`, `.ascii`/
`.asciz`/`.string`, `.zero`/`.space`, `.balign`/`.p2align`, and
instructions through §5), and module emission writes them through
`hod_obj` before the module's own symbols are defined, so that a
declaration the text defines becomes a definition.
