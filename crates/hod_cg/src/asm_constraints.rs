//! The constraint string of an inline-assembly call, read into a
//! target-neutral description (`spec/codegen.md` §7.3 "Inline-assembly
//! constraints"; design 0006 §1.2, R3).
//!
//! **Owner: WP-129.** Tests: the unit tests below (a toy x86-like
//! syntax) and `hod_a64`'s `tests/inline_asm.rs`.
//!
//! What is the same on every target is read here: the operand list
//! (outputs `=`, inputs, `asm goto` labels `!i`, clobbers `~{..}`), the
//! modifiers `*` (indirect), `&` (early clobber) and `%` (commutative),
//! alternatives separated by `|`, a digit naming the output an input is
//! tied to, an explicit register `{name}`, a flag output `{@cc<cond>}`,
//! the order rule (outputs, then inputs, then labels; a digit names an
//! output), the limits ([`MAX_OPS`] operands, [`MAX_ALTS`] alternatives)
//! and the choice among alternatives by weights ([`Constraints::choose`],
//! as `llc` chooses). What a letter means, which register a name is,
//! which clobbers are known, and what a code weighs for an operand are the
//! target's ([`ConstraintSyntax`], and the `weight` callback).
//!
//! Nothing here allocates: the description is fixed-size.

use hod_ir::{InlineAsm, Value};

use crate::{CgError, PReg};

/// The most operands (outputs, inputs and labels) a call may have.
pub const MAX_OPS: usize = 16;
/// The most alternatives (`r|m`) an operand may have.
pub const MAX_ALTS: usize = 4;

/// The code bit of an explicit register (`{x0}`). The bits 29 to 31 are
/// the parser's; a target's letter codes use the bits below.
pub const SPEC: u32 = 1 << 29;
/// The code bit of a digit: the input is tied to an output.
pub const TIED: u32 = 1 << 30;
/// The code bit of a flag output (`={@cc<cond>}`).
pub const FLAG: u32 = 1 << 31;

/// One alternative of an operand.
#[derive(Clone, Copy, Debug)]
pub struct Alt<C> {
    /// The codes: the target's letter bits, and [`SPEC`], [`TIED`],
    /// [`FLAG`].
    pub codes: u32,
    /// The output a [`TIED`] input is tied to (an index into the
    /// operand list, below the number of outputs).
    pub tied: u8,
    /// The explicit register.
    pub reg: Option<PReg>,
    /// The condition of a flag output.
    pub cond: Option<C>,
}

impl<C> Default for Alt<C> {
    fn default() -> Self {
        Alt { codes: 0, tied: 0, reg: None, cond: None }
    }
}

/// What an operand is.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Kind {
    /// An input (no prefix).
    #[default]
    Input,
    /// An output (`=`).
    Output,
    /// An `asm goto` target (`!i`).
    Label,
}

/// One operand.
#[derive(Clone, Copy, Debug)]
pub struct Op<C> {
    /// Input, output or label.
    pub kind: Kind,
    /// `*`: the operand is a pointer to the memory the constraint is for.
    pub indirect: bool,
    /// `&`: an output written before every input is read.
    pub early_clobber: bool,
    /// `%`: the operand commutes with the next.
    pub commutative: bool,
    /// The alternatives, [`Op::nalts`] of them.
    pub alts: [Alt<C>; MAX_ALTS],
    /// The number of alternatives (a label has one, `i`).
    pub nalts: u8,
    /// The call argument of an input or an indirect output.
    pub arg: Option<u8>,
    /// The field of the call's result of a direct output.
    pub field: Option<u8>,
}

impl<C> Default for Op<C> {
    fn default() -> Self {
        Op {
            kind: Kind::Input,
            indirect: false,
            early_clobber: false,
            commutative: false,
            alts: [Alt::default(), Alt::default(), Alt::default(), Alt::default()],
            nalts: 0,
            arg: None,
            field: None,
        }
    }
}

impl<C: Copy> Op<C> {
    /// Alternative `index`, or the first if the operand has fewer (`llc`
    /// reads a missing alternative as the first).
    pub fn alt(&self, index: usize) -> Alt<C> {
        self.alts[if index < usize::from(self.nalts) { index } else { 0 }]
    }
}

/// The parsed constraint string.
#[derive(Clone, Copy, Debug)]
pub struct Constraints<C> {
    /// The operands in order, [`Constraints::len`] of them.
    pub list: [Op<C>; MAX_OPS],
    /// The number of operands.
    pub len: usize,
    /// The outputs that are fields of the call's result (not `*`).
    pub direct_outputs: u8,
    /// The `asm goto` labels.
    pub labels: u8,
    /// The registers clobbered, as bits by [`PReg::index`].
    pub clobbers: u64,
    /// `~{memory}` was given.
    pub memory: bool,
}

/// A clobber name as the target reads it.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Clobber {
    /// Known and needing nothing (`cc`, the flags register).
    Ignored,
    /// A register.
    Reg(PReg),
    /// Not known: refused as [`ConstraintError::Clobber`].
    Unknown,
}

/// Why a constraint string is refused. The target turns it into its
/// refusal names (`asm:constraint/<code>`, `asm:clobber/<name>`, its own
/// limit name).
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum ConstraintError<'s> {
    /// A constraint not covered, by its text: an unterminated or unknown
    /// `{..}`, a bad flag condition, a label that is not `!i`, `empty`,
    /// `order`.
    Constraint(&'s [u8]),
    /// A letter no target code has.
    Letter(u8),
    /// A clobber not covered, by its text.
    Clobber(&'s [u8]),
    /// More than [`MAX_OPS`] operands or [`MAX_ALTS`] alternatives, two
    /// digits in one alternative, or a digit of [`MAX_OPS`] or more.
    TooMany,
    /// The target's own refusal (a frame or scratch register named).
    Target(CgError),
}

/// What a target says about the parts of a constraint that are its own.
pub trait ConstraintSyntax {
    /// The condition of a flag output.
    type Cond: Copy;

    /// The letter code at the start of `code` (never empty, never `{` or
    /// a digit of an input): its bits (below [`SPEC`]) and its length in
    /// bytes; `None` for a code the target does not have.
    fn letters(&self, code: &[u8], output: bool) -> Option<(u32, usize)>;

    /// The condition of a flag output `{@cc<cond>}` (`cond` is not
    /// empty), or `None`.
    fn flag(&self, cond: &[u8]) -> Option<Self::Cond>;

    /// The register `{name}` of an operand: `Ok(None)` for no register,
    /// `Err` for a register the target refuses as an operand.
    ///
    /// # Errors
    /// The target's refusal.
    fn operand_reg(&self, name: &[u8]) -> Result<Option<PReg>, CgError>;

    /// The clobber `~{name}` (not `memory`, which is read here).
    ///
    /// # Errors
    /// The target's refusal.
    fn clobber(&self, name: &[u8]) -> Result<Clobber, CgError>;
}

/// The flags of an inline-assembly value, by name.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
#[allow(clippy::struct_excessive_bools, reason = "LLVM's four flags, one each")]
pub struct AsmFlags {
    /// `sideeffect`.
    pub side_effect: bool,
    /// `alignstack`.
    pub align_stack: bool,
    /// `inteldialect`.
    pub intel_dialect: bool,
    /// `unwind`.
    pub unwind: bool,
}

impl AsmFlags {
    /// The flags of `asm`.
    pub fn of(asm: &InlineAsm<'_>) -> AsmFlags {
        AsmFlags {
            side_effect: asm.has(InlineAsm::SIDE_EFFECT),
            align_stack: asm.has(InlineAsm::ALIGN_STACK),
            intel_dialect: asm.has(InlineAsm::INTEL_DIALECT),
            unwind: asm.has(InlineAsm::UNWIND),
        }
    }
}

/// One alternative's codes.
fn parse_alt<'s, S: ConstraintSyntax>(
    syntax: &S,
    code: &'s [u8],
    output: bool,
) -> Result<Alt<S::Cond>, ConstraintError<'s>> {
    let mut alt = Alt::default();
    let mut i = 0;
    while i < code.len() {
        let c = code[i];
        match c {
            b'{' => {
                let Some(close) = code[i..].iter().position(|&b| b == b'}') else {
                    return Err(ConstraintError::Constraint(&code[i..]));
                };
                let name = &code[i + 1..i + close];
                if let Some(c) = name.strip_prefix(b"@cc").filter(|_| output) {
                    let Some(cond) = (!c.is_empty()).then(|| syntax.flag(c)).flatten() else {
                        return Err(ConstraintError::Constraint(name));
                    };
                    alt.codes |= FLAG;
                    alt.cond = Some(cond);
                    i += close + 1;
                    continue;
                }
                match syntax.operand_reg(name) {
                    Err(e) => return Err(ConstraintError::Target(e)),
                    Ok(None) => return Err(ConstraintError::Constraint(name)),
                    Ok(Some(r)) => {
                        alt.codes |= SPEC;
                        alt.reg = Some(r);
                    }
                }
                i += close + 1;
            }
            b'0'..=b'9' if !output => {
                let mut n = 0u32;
                while i < code.len() && code[i].is_ascii_digit() {
                    n = n.wrapping_mul(10).wrapping_add(u32::from(code[i] - b'0'));
                    i += 1;
                }
                if alt.codes & TIED != 0 || n >= MAX_OPS as u32 {
                    return Err(ConstraintError::TooMany);
                }
                alt.codes |= TIED;
                alt.tied = n as u8;
            }
            _ => {
                let Some((bits, len)) = syntax.letters(&code[i..], output) else {
                    return Err(ConstraintError::Letter(c));
                };
                debug_assert!(
                    bits & (SPEC | TIED | FLAG) == 0,
                    "a letter code in the parser's bits"
                );
                alt.codes |= bits;
                i += len.max(1);
            }
        }
    }
    if alt.codes == 0 {
        return Err(ConstraintError::Constraint(b"empty"));
    }
    Ok(alt)
}

/// Reads the constraint string `constraints` of an inline-assembly call.
///
/// # Errors
/// [`ConstraintError`].
pub fn parse<'s, S: ConstraintSyntax>(
    syntax: &S,
    constraints: &'s [u8],
) -> Result<Constraints<S::Cond>, ConstraintError<'s>> {
    let mut ops = Constraints {
        list: [Op::default(); MAX_OPS],
        len: 0,
        direct_outputs: 0,
        labels: 0,
        clobbers: 0,
        memory: false,
    };
    if constraints.is_empty() {
        return Ok(ops);
    }
    let mut args = 0u8;
    for part in constraints.split(|&b| b == b',') {
        if let Some(rest) = part.strip_prefix(b"~") {
            clobber(syntax, &mut ops, rest)?;
            continue;
        }
        if ops.len == MAX_OPS {
            return Err(ConstraintError::TooMany);
        }
        let mut op = Op::default();
        let mut rest = part;
        if let Some(r) = rest.strip_prefix(b"=") {
            op.kind = Kind::Output;
            rest = r;
        } else if let Some(r) = rest.strip_prefix(b"!") {
            if r != b"i" {
                return Err(ConstraintError::Constraint(part));
            }
            op.kind = Kind::Label;
            op.nalts = 1;
            // A label's one alternative is the target's own `i`.
            op.alts[0].codes = syntax.letters(b"i", false).map_or(0, |(bits, _)| bits);
            ops.list[ops.len] = op;
            ops.len += 1;
            ops.labels += 1;
            continue;
        }
        loop {
            match rest.first() {
                Some(b'*') => op.indirect = true,
                Some(b'&') => op.early_clobber = true,
                Some(b'%') => op.commutative = true,
                _ => break,
            }
            rest = &rest[1..];
        }
        for code in rest.split(|&b| b == b'|') {
            if usize::from(op.nalts) == MAX_ALTS {
                return Err(ConstraintError::TooMany);
            }
            op.alts[usize::from(op.nalts)] = parse_alt(syntax, code, op.kind == Kind::Output)?;
            op.nalts += 1;
        }
        if op.kind == Kind::Output && !op.indirect {
            op.field = Some(ops.direct_outputs);
            ops.direct_outputs += 1;
        } else {
            op.arg = Some(args);
            args += 1;
        }
        ops.list[ops.len] = op;
        ops.len += 1;
    }
    // Outputs first, then inputs, then labels; a digit names an output.
    let outputs = ops.list[..ops.len].iter().take_while(|op| op.kind == Kind::Output).count();
    let inputs = ops.list[outputs..ops.len].iter().take_while(|op| op.kind == Kind::Input).count();
    let tied_ok = ops.list[..ops.len].iter().all(|op| {
        op.alts[..usize::from(op.nalts)]
            .iter()
            .all(|a| a.codes & TIED == 0 || usize::from(a.tied) < outputs)
    });
    if outputs + inputs + usize::from(ops.labels) != ops.len || !tied_ok {
        return Err(ConstraintError::Constraint(b"order"));
    }
    Ok(ops)
}

/// One clobber, `rest` after the `~`.
fn clobber<'s, S: ConstraintSyntax>(
    syntax: &S,
    ops: &mut Constraints<S::Cond>,
    rest: &'s [u8],
) -> Result<(), ConstraintError<'s>> {
    let Some(name) = rest.strip_prefix(b"{").and_then(|r| r.strip_suffix(b"}")) else {
        return Err(ConstraintError::Clobber(rest));
    };
    if name == b"memory" {
        ops.memory = true;
        return Ok(());
    }
    match syntax.clobber(name) {
        Err(e) => Err(ConstraintError::Target(e)),
        Ok(Clobber::Ignored) => Ok(()),
        Ok(Clobber::Reg(r)) => {
            ops.clobbers |= 1 << r.index();
            Ok(())
        }
        Ok(Clobber::Unknown) => Err(ConstraintError::Clobber(name)),
    }
}

impl<C: Copy> Constraints<C> {
    /// The operands, in order.
    pub fn ops(&self) -> &[Op<C>] {
        &self.list[..self.len]
    }

    /// The alternative `llc` takes: of the indices below the greatest
    /// number of alternatives, the first whose weights summed over the
    /// operands are greatest; an alternative in which an operand weighs
    /// `None` (cannot take it) is never taken; 0 if none can, or if no
    /// operand has two. `weight(operand, codes)` is an alternative's
    /// weight for the call argument `operand` (`None` for an operand
    /// without one: a direct output or a label).
    pub fn choose(
        &self,
        args: &[Value],
        mut weight: impl FnMut(Option<Value>, u32) -> Option<i32>,
    ) -> usize {
        let count = self.ops().iter().map(|op| usize::from(op.nalts)).max().unwrap_or(1);
        if count <= 1 {
            return 0;
        }
        let (mut chosen, mut best) = (0, -1);
        for index in 0..count {
            let mut sum = Some(0);
            for op in self.ops() {
                let operand = op.arg.map(|a| args[usize::from(a)]);
                sum = match (sum, weight(operand, op.alt(index).codes)) {
                    (Some(s), Some(w)) => Some(s + w),
                    _ => None,
                };
            }
            if let Some(sum) = sum.filter(|&s| s > best) {
                best = sum;
                chosen = index;
            }
        }
        chosen
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A toy x86-like syntax: `r` general, `x` vector, `m` memory, `i`
    /// immediate, `Yz` (two bytes) the first vector register; registers
    /// `{ax}`..`{dx}` are 0..3, `{sp}` is refused, `{xmm0}` is 16; flag
    /// conditions `e`, `ne`; clobbers `flags`, `dirflag`, `fpsr` ignored.
    struct Toy;

    const R: u32 = 1;
    const X: u32 = 2;
    const M: u32 = 4;
    const I: u32 = 8;
    const YZ: u32 = 16;

    impl ConstraintSyntax for Toy {
        type Cond = u8;

        fn letters(&self, code: &[u8], _output: bool) -> Option<(u32, usize)> {
            Some(match code {
                [b'Y', b'z', ..] => (YZ, 2),
                [b'r', ..] => (R, 1),
                [b'x', ..] => (X, 1),
                [b'm', ..] => (M, 1),
                [b'i', ..] => (I, 1),
                _ => return None,
            })
        }

        fn flag(&self, cond: &[u8]) -> Option<u8> {
            match cond {
                b"e" => Some(4),
                b"ne" => Some(5),
                _ => None,
            }
        }

        fn operand_reg(&self, name: &[u8]) -> Result<Option<PReg>, CgError> {
            Ok(Some(PReg::new(match name {
                b"ax" => 0,
                b"cx" => 1,
                b"dx" => 2,
                b"bx" => 3,
                b"xmm0" => 16,
                b"sp" => return Err(CgError::unsupported("toy:frame-register")),
                _ => return Ok(None),
            })))
        }

        fn clobber(&self, name: &[u8]) -> Result<Clobber, CgError> {
            match name {
                b"flags" | b"dirflag" | b"fpsr" => Ok(Clobber::Ignored),
                _ => match self.operand_reg(name)? {
                    Some(r) => Ok(Clobber::Reg(r)),
                    None => Ok(Clobber::Unknown),
                },
            }
        }
    }

    fn p(s: &[u8]) -> Result<Constraints<u8>, ConstraintError<'_>> {
        parse(&Toy, s)
    }

    /// §7.3 "Inline-assembly constraints": the operand list, its kinds,
    /// modifiers, ties, registers and clobbers.
    #[test]
    fn operands_and_clobbers() {
        let c = p(b"=&r,={ax},=*m,0,%x|Yz,{dx},!i,~{bx},~{memory},~{dirflag},~{flags}").unwrap();
        assert_eq!((c.len, c.direct_outputs, c.labels), (7, 2, 1));
        assert!(c.memory);
        assert_eq!(c.clobbers, 1 << 3);
        let ops = c.ops();
        assert!(ops[0].early_clobber && ops[0].kind == Kind::Output && ops[0].field == Some(0));
        assert_eq!((ops[1].alts[0].codes, ops[1].alts[0].reg), (SPEC, Some(PReg::new(0))));
        assert!(ops[2].indirect && ops[2].arg == Some(0) && ops[2].field.is_none());
        assert_eq!((ops[3].alts[0].codes, ops[3].alts[0].tied, ops[3].arg), (TIED, 0, Some(1)));
        assert!(ops[4].commutative);
        assert_eq!((ops[4].nalts, ops[4].alts[0].codes, ops[4].alts[1].codes), (2, X, YZ));
        assert_eq!(ops[6].kind, Kind::Label);
        assert_eq!(ops[6].alts[0].codes, I);
        // A missing alternative reads as the first.
        assert_eq!(ops[0].alt(1).codes, R);
        let f = p(b"={@ccne},r").unwrap();
        assert_eq!((f.list[0].alts[0].codes, f.list[0].alts[0].cond), (FLAG, Some(5)));
        assert_eq!(p(b"").unwrap().len, 0);
    }

    /// The refusals, each by its kind, in the order the string is read.
    #[test]
    fn refusals() {
        use ConstraintError as E;
        assert_eq!(p(b"{sp}").unwrap_err(), E::Target(CgError::unsupported("toy:frame-register")));
        assert_eq!(p(b"~{sp}").unwrap_err(), E::Target(CgError::unsupported("toy:frame-register")));
        assert_eq!(p(b"{zz}").unwrap_err(), E::Constraint(b"zz"));
        assert_eq!(p(b"{ax").unwrap_err(), E::Constraint(b"{ax"));
        assert_eq!(p(b"={@cczz}").unwrap_err(), E::Constraint(b"@cczz"));
        assert_eq!(p(b"={@cc}").unwrap_err(), E::Constraint(b"@cc"));
        // A flag on an input is a register name.
        assert_eq!(p(b"{@cce}").unwrap_err(), E::Constraint(b"@cce"));
        assert_eq!(p(b"g").unwrap_err(), E::Letter(b'g'));
        assert_eq!(p(b"=0").unwrap_err(), E::Letter(b'0'));
        assert_eq!(p(b"*").unwrap_err(), E::Constraint(b"empty"));
        assert_eq!(p(b"r,=r").unwrap_err(), E::Constraint(b"order"));
        assert_eq!(p(b"=r,1").unwrap_err(), E::Constraint(b"order"));
        assert_eq!(p(b"!r").unwrap_err(), E::Constraint(b"!r"));
        assert_eq!(p(b"~bx").unwrap_err(), E::Clobber(b"bx"));
        assert_eq!(p(b"~{st}").unwrap_err(), E::Clobber(b"st"));
        assert_eq!(p(b"=r,00").unwrap().list[1].alts[0].tied, 0);
        assert_eq!(p(b"=r,01").unwrap_err(), E::Constraint(b"order"));
        assert_eq!(p(b"=r,16").unwrap_err(), E::TooMany);
        assert_eq!(p(b"=r,0 0").unwrap_err(), E::Letter(b' '));
        assert_eq!(p(b"r|r|r|r|r").unwrap_err(), E::TooMany);
        assert_eq!(p(&b"r,".repeat(17)[..33]).unwrap_err(), E::TooMany);
        assert_eq!(p(&b"r,".repeat(16)[..31]).unwrap().len, 16);
    }

    /// The choice among alternatives: the first of the greatest sum; an
    /// alternative an operand cannot take is never chosen.
    #[test]
    fn choice_by_weight() {
        let c = p(b"r|m|i,r|m|i").unwrap();
        let args = [Value::NONE, Value::NONE];
        let w = |_: Option<Value>, codes: u32| match codes {
            R => Some(1),
            M => Some(2),
            _ => None,
        };
        assert_eq!(c.choose(&args, w), 1);
        let one = p(b"r,m").unwrap();
        assert_eq!(one.choose(&args, w), 0);
        let none = p(b"i|i,r|r").unwrap();
        assert_eq!(none.choose(&args, w), 0);
    }
}
