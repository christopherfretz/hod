//! Inline assembly whose template assembles to nothing (`spec/ir.md`
//! §2.3, `spec/codegen.md` §5.6.8 "Intrinsics (WP-128)"): optimization
//! barriers such as `asm volatile("" : : "r,m"(x) : "memory")` and
//! `asm("" : "+r"(x))`. A template that assembles to something is
//! [`INLINE_ASM`]: hod has no x86 assembler for templates.
//!
//! **Owner: WP-128.** Tests: `tests/barrier.rs`. Entry: `target.rs`
//! (`lower_call`). The decisions are `hod_a64::barrier`'s (WP-075); only
//! the constraint codes, their weights and the clobbers are x86's.
//!
//! The template does nothing, so what such a call does is what its
//! constraints make `llc` do around it:
//!
//! - an input is evaluated (a deferred operand is emitted) and otherwise
//!   has no effect: whether `llc` puts it in a register or spills it for
//!   an `m` alternative, nothing reads it;
//! - an output tied to an input (`=r,0`) holds the input: a direct
//!   output is the call's result, a copy of the input; an indirect
//!   output (`=*r`, the operand is the address) that `llc` gives a
//!   register is stored to its address after the template, so the
//!   input's value is stored there;
//! - an indirect output `llc` gives a memory operand is the memory
//!   itself, which the template leaves alone, tied input or not;
//! - an output nothing ties is undefined after the template: refused
//!   ([`OUTPUT`]), as is one in memory `llc` would give a register
//!   alternative to (the choice is not modelled);
//! - clobbers ask nothing of a template that writes nothing, but must
//!   name what the target has ([`CLOBBER`]): `memory`, the flags
//!   (`cc`, `flags`, `dirflag`, `fpsr`, `fpcr`), a general register by
//!   any of its names, `xmm`/`ymm`/`zmm` 0 to 31;
//! - `sideeffect`, `alignstack`, `inteldialect` and `unwind` change
//!   nothing; an `invoke` continues at its normal destination (the
//!   template cannot throw);
//! - a `callbr` (WP-181, `spec/codegen.md` §5.6.23) has one label
//!   constraint `!i` per indirect destination, which takes no argument;
//!   the template never jumps, so it continues at its fallthrough with
//!   that edge's `phi`s, as `llc` builds it. Its indirect destinations
//!   are blocks like any other (emitted, their labels bound, reached from
//!   elsewhere or not at all): nothing is written for those edges.
//!
//! Which alternative of a multi-alternative constraint (`r|m`) `llc`
//! takes is LLVM's choice by weights (`TargetLowering::ParseConstraints`
//! with `X86TargetLowering::getSingleConstraintMatchWeight`): for each
//! alternative, the sum over the operands of the best weight of its
//! codes, the first highest sum wins (all invalid: the first). On x86
//! every general code of an integer operand weighs the same (0,
//! `CW_SpecificReg`), so integers keep the first alternative; a pointer
//! or floating-point operand takes the generic weights (`m` 2, `r`
//! invalid). Codes of one alternative (`imr`) are LLVM's
//! `ComputeConstraintToUse`: a register class when the output is tied.

use hod_cg::{CgResult, Class, FnCx};
use hod_ir::{Const, Inst, InstKind, MemFlags, Opcode, TypeId, TypeKind, Value, ValueKind};

use crate::common::{branch, deferred_first, unsupported, value_ty};
use crate::target::X64;

/// An output the template leaves undefined: one no input is tied to, or
/// an indirect one whose operand `llc` may give a register.
pub const OUTPUT: &str = "x64:inline-asm-output";
/// More than one effect: several tied outputs, a struct of results.
pub const OUTPUTS: &str = "x64:inline-asm-outputs";
/// A constraint this module does not read: an unknown code, a label
/// (`!i`) outside a `callbr` or a count of labels that is not the
/// `callbr`'s count of indirect destinations, more operands or
/// alternatives than it keeps, an operand count that does not match the
/// call.
pub const CONSTRAINT: &str = "x64:inline-asm-constraint";
/// A clobber that names neither memory, the flags nor a register of the
/// target.
pub const CLOBBER: &str = "x64:inline-asm-clobber";
/// An operand of a type the chosen constraint does not take: a tied pair
/// of different types, an `i`/`n` operand that is not a constant
/// integer, a value stored through an output that is not a scalar of
/// 8 to 64 bits.
pub const OPERAND: &str = "x64:inline-asm-operand";
/// A call of inline assembly with an operand bundle.
pub const BUNDLE: &str = "x64:inline-asm-bundle";
/// Inline assembly whose template assembles to something: hod has no
/// x86 assembler for templates.
pub const INLINE_ASM: &str = "x64:inline-asm";

/// A `callbr` whose result a `phi` of one of its destinations takes: the
/// shared walk writes a `callbr`'s `phi`s before its lowering, as for
/// every terminator but `invoke` (`hod_cg` `driver.rs`), so that `phi`
/// would be given the result before it exists. (A result whose only use
/// is such a `phi` is a register value and stops the walk first with an
/// internal error.)
pub const RESULT_PHI: &str = "x64:callbr-result-phi";

/// Every name this module refuses with.
pub const UNSUPPORTED: &[&str] =
    &[OUTPUT, OUTPUTS, CONSTRAINT, CLOBBER, OPERAND, BUNDLE, INLINE_ASM, RESULT_PHI];

/// The most operands (constraints that are not clobbers) and
/// alternatives kept.
const MAX_OPS: usize = 16;
const MAX_ALTS: usize = 4;

/// Codes of one alternative, as bits.
const R: u16 = 1 << 0; // r, l: any general register
const M: u16 = 1 << 1; // m, o
const I: u16 = 1 << 2; // i, n
const XMM: u16 = 1 << 3; // x
const G: u16 = 1 << 4; // g
const XX: u16 = 1 << 5; // X
const SPEC: u16 = 1 << 6; // {reg}
const TIED: u16 = 1 << 7; // a digit
const CLASS: u16 = 1 << 8; // q, Q, R: general registers by class
const SMALL: u16 = 1 << 9; // I: an integer constant 0 to 31

#[derive(Clone, Copy, Default, Debug)]
struct Alt {
    codes: u16,
    tied: u8,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
enum Kind {
    #[default]
    Input,
    Output,
}

#[derive(Clone, Copy, Default, Debug)]
struct Op {
    kind: Kind,
    indirect: bool,
    alts: [Alt; MAX_ALTS],
    nalts: u8,
    /// The call argument of an input or an indirect output.
    arg: Option<u8>,
}

/// The operands of a constraint string, without allocating.
struct Ops {
    list: [Op; MAX_OPS],
    len: usize,
    direct_outputs: usize,
    /// The label constraints (`!i`) of a `callbr`: they take no call
    /// argument and name the indirect destinations in order.
    labels: usize,
}

/// Whether `name` (between the braces of `~{...}`) is something an
/// x86-64 clobber may name: memory, the flags, or a register.
fn known_clobber(name: &[u8]) -> bool {
    matches!(name, b"memory" | b"cc" | b"flags" | b"dirflag" | b"fpsr" | b"fpcr")
        || known_register(name)
}

/// Whether `name` is an x86-64 register LLVM knows by that name: a
/// general register by any of its widths' names, or `xmm`, `ymm`, `zmm`
/// 0 to 31.
fn known_register(name: &[u8]) -> bool {
    const LEGACY: [&[u8]; 8] = [b"ax", b"bx", b"cx", b"dx", b"si", b"di", b"bp", b"sp"];
    const BYTES: [&[u8]; 8] = [b"al", b"bl", b"cl", b"dl", b"sil", b"dil", b"bpl", b"spl"];
    let number = |digits: &[u8], limit: u32| {
        !digits.is_empty()
            && digits.len() <= 2
            && digits.iter().all(u8::is_ascii_digit)
            && (digits.len() == 1 || digits[0] != b'0')
            && digits.iter().fold(0u32, |n, &d| n * 10 + u32::from(d - b'0')) <= limit
    };
    if LEGACY.contains(&name) || BYTES.contains(&name) {
        return true;
    }
    if let Some(rest) = name.strip_prefix(b"r").or_else(|| name.strip_prefix(b"e"))
        && LEGACY.contains(&rest)
    {
        return true;
    }
    if let Some(rest) = name.strip_prefix(b"r") {
        let digits = match rest.last() {
            Some(b'd' | b'w' | b'b') => &rest[..rest.len() - 1],
            _ => rest,
        };
        if number(digits, 15) && digits.len() == 2 || matches!(digits, b"8" | b"9") {
            return true;
        }
    }
    [&b"xmm"[..], b"ymm", b"zmm"]
        .iter()
        .any(|prefix| name.strip_prefix(*prefix).is_some_and(|digits| number(digits, 31)))
}

/// Parses one alternative's codes.
fn parse_alt(text: &[u8]) -> CgResult<Alt> {
    let mut alt = Alt::default();
    let mut i = 0;
    while i < text.len() {
        let c = text[i];
        match c {
            b'r' | b'l' => alt.codes |= R,
            b'q' | b'Q' | b'R' => {
                alt.codes |= CLASS;
            }
            b'm' | b'o' => alt.codes |= M,
            b'i' | b'n' => alt.codes |= I,
            b'I' => alt.codes |= SMALL,
            b'x' => alt.codes |= XMM,
            b'g' => alt.codes |= G,
            b'X' => alt.codes |= XX,
            b'{' => {
                let Some(close) = text[i..].iter().position(|&b| b == b'}') else {
                    return unsupported(CONSTRAINT);
                };
                if !known_register(&text[i + 1..i + close]) {
                    return unsupported(CONSTRAINT);
                }
                alt.codes |= SPEC;
                i += close;
            }
            b'0'..=b'9' => {
                let mut n = 0u32;
                while i < text.len() && text[i].is_ascii_digit() {
                    n = n * 10 + u32::from(text[i] - b'0');
                    i += 1;
                }
                if alt.codes & TIED != 0 || n >= MAX_OPS as u32 {
                    return unsupported(CONSTRAINT);
                }
                alt.codes |= TIED;
                alt.tied = n as u8;
                continue;
            }
            _ => return unsupported(CONSTRAINT),
        }
        i += 1;
    }
    if alt.codes == 0 {
        return unsupported(CONSTRAINT);
    }
    Ok(alt)
}

impl Ops {
    /// Reads `constraints`. Clobbers are checked and dropped.
    fn parse(constraints: &[u8]) -> CgResult<Ops> {
        let mut ops = Ops { list: [Op::default(); MAX_OPS], len: 0, direct_outputs: 0, labels: 0 };
        if constraints.is_empty() {
            return Ok(ops);
        }
        let mut args = 0u8;
        for part in constraints.split(|&b| b == b',') {
            if let Some(rest) = part.strip_prefix(b"~") {
                let name = rest.strip_prefix(b"{").and_then(|r| r.strip_suffix(b"}"));
                if !name.is_some_and(known_clobber) {
                    return unsupported(CLOBBER);
                }
                continue;
            }
            if part == b"!i" {
                ops.labels += 1;
                continue;
            }
            if ops.len == MAX_OPS {
                return unsupported(CONSTRAINT);
            }
            let mut op = Op::default();
            let mut rest = part;
            if let Some(r) = rest.strip_prefix(b"=") {
                op.kind = Kind::Output;
                rest = r;
            }
            loop {
                match rest.first() {
                    Some(b'*') => op.indirect = true,
                    Some(b'&' | b'%') => {}
                    _ => break,
                }
                rest = &rest[1..];
            }
            for text in rest.split(|&b| b == b'|') {
                if usize::from(op.nalts) == MAX_ALTS {
                    return unsupported(CONSTRAINT);
                }
                op.alts[usize::from(op.nalts)] = parse_alt(text)?;
                op.nalts += 1;
            }
            if op.kind == Kind::Output
                && op.alts[..usize::from(op.nalts)].iter().any(|a| a.codes & TIED != 0)
            {
                return unsupported(CONSTRAINT);
            }
            if op.kind == Kind::Output && !op.indirect {
                ops.direct_outputs += 1;
            } else {
                op.arg = Some(args);
                args += 1;
            }
            ops.list[ops.len] = op;
            ops.len += 1;
        }
        // Outputs come first, and a digit names an output.
        let outputs = ops.list[..ops.len].iter().take_while(|op| op.kind == Kind::Output).count();
        let tied_ok = ops.list[..ops.len].iter().all(|op| {
            op.alts[..usize::from(op.nalts)]
                .iter()
                .all(|a| a.codes & TIED == 0 || usize::from(a.tied) < outputs)
        });
        if ops.list[outputs..ops.len].iter().any(|op| op.kind == Kind::Output) || !tied_ok {
            return unsupported(CONSTRAINT);
        }
        Ok(ops)
    }
}

/// LLVM's weight of one code for an operand
/// (`X86TargetLowering::getSingleConstraintMatchWeight`): `None` is
/// `CW_Invalid`. The codes x86 does not handle itself take the generic
/// weight and then, for an integer operand, `CW_SpecificReg` (0), as do
/// the register classes `q`, `Q`, `R`; for another operand those keep
/// the generic weight, and the classes stay invalid. (`a`, `b`, `c`,
/// `d`, `S`, `D`, `A` are clang's spellings; IR writes `{ax}` and `llc`
/// does not read them: [`CONSTRAINT`].)
fn code_weight(cx: &FnCx<'_, X64>, operand: Option<Value>, code: u16) -> Option<i32> {
    // A direct output has no operand: every code is `CW_Default`.
    let Some(value) = operand else { return Some(0) };
    let ty = value_ty(cx, value);
    let kind = cx.types().kind(ty);
    let integer = matches!(kind, TypeKind::Int(_));
    match code {
        XMM => (matches!(kind, TypeKind::Int(_) | TypeKind::Fp128 | TypeKind::Vector { .. })
            && cx.types().size_in_bits_of(ty) == 128)
            .then_some(1),
        SMALL => {
            let small = match value.kind() {
                ValueKind::Const(id) => {
                    matches!(cx.module.consts.get(id), Const::Int { value, .. } if value <= 31)
                }
                _ => false,
            };
            small.then_some(3)
        }
        _ if integer => Some(0),
        I => is_const_int(cx, value).then_some(3),
        M => Some(2),
        R | G | CLASS => None,
        _ => Some(0),
    }
}

fn is_const_int(cx: &FnCx<'_, X64>, value: Value) -> bool {
    match value.kind() {
        ValueKind::Const(id) => matches!(cx.module.consts.get(id), Const::Int { .. }),
        _ => false,
    }
}

/// The weight of alternative `index` of `op` (its first when it has
/// fewer).
fn alt_weight(cx: &FnCx<'_, X64>, op: &Op, operand: Option<Value>, index: usize) -> Option<i32> {
    let alt = op.alts[if index < usize::from(op.nalts) { index } else { 0 }];
    let mut best = None;
    for code in [R, M, I, XMM, G, XX, SPEC, TIED, CLASS, SMALL] {
        if alt.codes & code != 0 {
            best = best.max(code_weight(cx, operand, code));
        }
    }
    best
}

/// What a barrier does besides ordering: at most one effect.
enum Effect {
    None,
    /// The result is this input's value.
    Result(Value),
    /// An indirect output tied to this input stores it.
    Store {
        value: Value,
        ptr: Value,
    },
}

/// The alternative LLVM takes: the first of the greatest total weight,
/// or the first when every one is invalid.
fn choose(cx: &FnCx<'_, X64>, ops: &Ops, args: &[Value]) -> usize {
    let value_of = |op: &Op| op.arg.map(|a| args[usize::from(a)]);
    let count = ops.list[..ops.len].iter().map(|op| usize::from(op.nalts)).max().unwrap_or(1);
    let mut chosen = 0;
    if count > 1 {
        let mut best_sum = -1;
        for index in 0..count {
            let mut sum = Some(0);
            for op in &ops.list[..ops.len] {
                sum = match (sum, alt_weight(cx, op, value_of(op), index)) {
                    (Some(s), Some(w)) => Some(s + w),
                    _ => None,
                };
            }
            if let Some(sum) = sum.filter(|&s| s > best_sum) {
                best_sum = sum;
                chosen = index;
            }
        }
    }
    chosen
}

/// The effect of the barrier `inst` with operands `ops` (module
/// documentation).
fn effect<'a>(
    cx: &FnCx<'a, X64>,
    inst: Inst<'a>,
    site: hod_ir::CallSite<'a>,
    ops: &Ops,
) -> CgResult<Effect> {
    let args = site.args();
    let value_of = |op: &Op| op.arg.map(|a| args[usize::from(a)]);
    let chosen = choose(cx, ops, args);
    let alt_of = |op: &Op| op.alts[if chosen < usize::from(op.nalts) { chosen } else { 0 }];
    // The input tied to each output, under that alternative.
    let mut tied_to: [Option<u8>; MAX_OPS] = [None; MAX_OPS];
    for (index, op) in ops.list[..ops.len].iter().enumerate() {
        let alt = alt_of(op);
        if op.kind == Kind::Input && alt.codes & TIED != 0 {
            if tied_to[usize::from(alt.tied)].is_some() {
                return unsupported(CONSTRAINT);
            }
            tied_to[usize::from(alt.tied)] = Some(index as u8);
        }
        if op.kind == Kind::Input
            && alt.codes & !(I | SMALL) == 0
            && !value_of(op).is_some_and(|v| is_const_int(cx, v))
        {
            return unsupported(OPERAND);
        }
    }
    // What each output does: at most one effect.
    let mut effect = Effect::None;
    for (index, op) in ops.list[..ops.len].iter().enumerate() {
        if op.kind != Kind::Output {
            continue;
        }
        let alt = alt_of(op);
        let register = alt.codes & (R | XMM | SPEC | XX | CLASS) != 0;
        let memory = alt.codes & M != 0;
        let input = tied_to[index].map(|i| &ops.list[usize::from(i)]);
        let this = if op.indirect {
            // `llc` takes memory for an output of memory codes only, and
            // a register for a tied one with a register code.
            let ptr = value_of(op).expect("an indirect output has an operand");
            match input {
                _ if memory && !register => Effect::None,
                Some(input) if register => {
                    let value = value_of(input).expect("an input has an operand");
                    if input.indirect {
                        if value != ptr {
                            return unsupported(OUTPUT);
                        }
                        Effect::None
                    } else {
                        let elem = site.arg_attrs(usize::from(op.arg.expect("operand"))).mem_type();
                        if elem != Some(value_ty(cx, value)) || !storable(cx, value) {
                            return unsupported(OPERAND);
                        }
                        Effect::Store { value, ptr }
                    }
                }
                _ => return unsupported(OUTPUT),
            }
        } else {
            let Some(input) = input.filter(|_| register && !memory) else {
                return unsupported(OUTPUT);
            };
            let value = value_of(input).expect("an input has an operand");
            if input.indirect
                || value_ty(cx, value) != inst.ty()
                || cx.class_of(value) == Class::Mem
            {
                return unsupported(OPERAND);
            }
            Effect::Result(value)
        };
        if !matches!(this, Effect::None) {
            if !matches!(effect, Effect::None) {
                return unsupported(OUTPUTS);
            }
            effect = this;
        }
    }
    Ok(effect)
}

/// A call whose callee is inline assembly: the empty-template call
/// `inst` (module documentation).
///
/// # Errors
/// [`INLINE_ASM`] for a template that is not empty; the names of this
/// module.
pub fn asm<'a>(cx: &mut FnCx<'a, X64>, inst: Inst<'a>) -> CgResult<()> {
    let Some(site) = inst.call() else {
        return cx.bug("an inline-assembly lowering of something that is no call");
    };
    let ValueKind::Asm(id) = site.callee_value().kind() else {
        return cx.bug("an inline-assembly lowering of another callee");
    };
    let asm = cx.module.asm(id);
    if !hod_ir::asm_template_is_empty(hod_ir::Arch::X86_64, asm.template) {
        return unsupported(INLINE_ASM);
    }
    if site.bundle_count() != 0 {
        return unsupported(BUNDLE);
    }
    let ops = Ops::parse(asm.constraints)?;
    if ops.direct_outputs > 1 || (ops.direct_outputs == 0) != (inst.ty() == TypeId::VOID) {
        return unsupported(OUTPUTS);
    }
    let args = site.args();
    let operands = ops.list[..ops.len].iter().filter(|op| op.arg.is_some()).count();
    let labels = if inst.opcode() == Opcode::CallBr { site.indirect_dests().count() } else { 0 };
    if operands != args.len() || ops.labels != labels {
        return unsupported(CONSTRAINT);
    }
    let effect = effect(cx, inst, site, &ops)?;
    if inst.opcode() == Opcode::CallBr && inst.ty() != TypeId::VOID {
        let result = inst.result();
        for succ in inst.successors() {
            for phi in cx.body.block(succ).phis() {
                if let InstKind::Phi(list) = phi.kind()
                    && list.values().contains(&result)
                {
                    return unsupported(RESULT_PHI);
                }
            }
        }
    }
    deferred_first(cx, inst, None)?;
    match effect {
        Effect::None => {}
        Effect::Result(value) => {
            let r = cx.use_reg(value)?;
            cx.finish(inst, r)?;
        }
        Effect::Store { value, ptr } => {
            // Alignment does not change an x86-64 store's code.
            let mem = MemFlags::aligned(1).map_err(|e| hod_cg::CgError::internal(e.to_string()))?;
            crate::mem::store(cx, inst, value, ptr, mem)?;
        }
    }
    if matches!(inst.opcode(), Opcode::Invoke | Opcode::CallBr) {
        // An empty template never jumps: a `callbr` goes on at its
        // fallthrough, as `llc` builds it; its indirect destinations are
        // entered only from elsewhere (their labels are bound where the
        // blocks are emitted, a `blockaddress` of them included).
        let Some(normal) = site.normal_dest() else {
            return cx.bug("an invoke or callbr without a normal destination");
        };
        cx.phi_stores(normal)?;
        if !cx.falls_through(normal) {
            branch(cx, cx.block_label(normal))?;
        }
    }
    Ok(())
}

/// Whether `value` is a scalar `llc` stores from one register: an
/// integer of 8, 16, 32 or 64 bits, a pointer, a `float` or a `double`.
fn storable(cx: &FnCx<'_, X64>, value: Value) -> bool {
    match cx.types().kind(value_ty(cx, value)) {
        TypeKind::Int(bits) => matches!(bits, 8 | 16 | 32 | 64),
        TypeKind::Ptr { addr_space: 0 } | TypeKind::Float | TypeKind::Double => true,
        _ => false,
    }
}
