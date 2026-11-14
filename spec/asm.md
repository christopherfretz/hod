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
  `dsb` (every option), `svc` `brk` `udf`, `mrs`/`msr` of a system
  register (§5.4), `bti`, `paciasp` `pacibsp` `autiasp` `autibsp`;
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
text at an offset that is not a multiple of 4, §9).

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
  repeated to a fixed point). A named label is `asm:label`, any other
  directive `asm:directive/<name>`. A body defines no symbol; it
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
  `{@cc<cond>}`, digits, `|` alternatives, `=`, `+`, `&`, `*`, `%`.
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
| `.cfi_*` in a body (valgrind's `CALL_FN_*`) | `asm:directive/cfi_remember_state` | 15 |
| a call or branch to a symbol (`call __bolt_fini_trampoline`) | `asm:mnemonic/call` | 3 |
| the control registers (`mov %cr3, %rax`) | `asm:operand/register` | 2 |
| AVX (`vdivpd`) | `asm:mnemonic/v*` | 2 |
| an indirect branch or call (`jmpq *%rdx`, `call *%rax`) | `asm:operand/form` | 2 |
| a symbol in an immediate (`movabsq $__hot_end, %rcx`) | `asm:operand/symbol` | 1 |
| a difference of labels in data (`.long 1f-1b-6`) | `asm:operand/expression` | 1 |
| `lea` with `@gotpcrel` (LLVM 19 and 22 differ) | `asm:operand/specifier` | 0 |
| `inteldialect` | `x64:inline-asm-intel` | 0 |

The x87 register operands (`fsqrt` with `=t`, `fpatan` with `{st}`,
found in 4 templates) are refused before the body is read, as
`asm:constraint/t`, `asm:constraint/st` (§11).
