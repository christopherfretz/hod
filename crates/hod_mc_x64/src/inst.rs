//! The encoded instruction, and the one routine that lays out prefixes,
//! REX, opcode, `ModRM`, `SIB`, displacement and immediate (`spec/mc-x64.md`
//! "Shape of the API", "Instruction layout").

use crate::fixup::Fixup;
use crate::reg::{Base, Mem, Reg};

/// The longest instruction the architecture allows.
pub const MAX_LEN: usize = 15;

const SITE_REL8: u8 = 1;
const SITE_REL32: u8 = 2;
const SITE_ABS32: u8 = 3;
const SITE_ABS32U: u8 = 4;
const SITE_ABS64: u8 = 5;
const SITE_MASK: u8 = 7;
const FLAG_REX: u8 = 8;
const FLAG_LOCKABLE: u8 = 16;

/// One encoded instruction: up to 15 bytes, in order.
///
/// It also records where its relocatable field is, if it has one (a
/// branch displacement, the `disp32` of a RIP-relative operand, the
/// absolute `disp32` of an operand marked
/// [`with_abs32`](crate::Mem::with_abs32), or the immediate of a
/// relocatable-immediate form such as [`movabs_ri`](crate::movabs_ri)),
/// whether
/// it has a REX prefix, and whether a `lock` prefix may be added to it.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct Inst {
    bytes: [u8; MAX_LEN],
    len: u8,
    /// Offset of the relocatable field (never 0: a field follows an
    /// opcode byte).
    site: u8,
    /// Where a `lock` prefix goes: after the segment and operand-size
    /// prefixes.
    lock_at: u8,
    flags: u8,
}

/// The relocatable field of an instruction, as a relocation or a fixup
/// sees it.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct Site {
    /// Byte offset of the field from the start of the instruction.
    pub offset: usize,
    /// The field's width.
    pub fixup: Fixup,
    /// The addend of a relocation at the field. For a PC-relative field,
    /// `-(len - offset)`, so that `S + addend - P` is the distance from the
    /// end of the instruction; for an absolute field ([`Fixup::Abs32`],
    /// [`Fixup::Abs32U`], [`Fixup::Abs64`]), 0.
    pub addend: i64,
}

impl Inst {
    pub(crate) const EMPTY: Self =
        Self { bytes: [0; MAX_LEN], len: 0, site: 0, lock_at: 0, flags: 0 };

    /// The instruction's bytes.
    #[inline]
    pub const fn bytes(&self) -> &[u8] {
        self.bytes.split_at(self.len as usize).0
    }

    /// The instruction's length in bytes, `1..=15`.
    #[inline]
    pub const fn len(&self) -> usize {
        self.len as usize
    }

    /// Always `false`: present because `len` is.
    #[inline]
    pub const fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// The relocatable field, if the instruction has one.
    #[inline]
    pub const fn site(&self) -> Option<Site> {
        let fixup = match self.flags & SITE_MASK {
            SITE_REL8 => Fixup::Rel8,
            SITE_REL32 => Fixup::Rel32,
            SITE_ABS32 => Fixup::Abs32,
            SITE_ABS32U => Fixup::Abs32U,
            SITE_ABS64 => Fixup::Abs64,
            _ => return None,
        };
        let offset = self.site as usize;
        let addend = if fixup.is_pc_relative() { offset as i64 - self.len as i64 } else { 0 };
        Some(Site { offset, fixup, addend })
    }

    /// Whether the instruction has a REX prefix: the choice between
    /// `R_X86_64_GOTPCRELX` and `R_X86_64_REX_GOTPCRELX`.
    #[inline]
    pub const fn has_rex(&self) -> bool {
        self.flags & FLAG_REX != 0
    }

    /// Whether [`lock`](crate::lock) accepts the instruction.
    #[inline]
    pub const fn is_lockable(&self) -> bool {
        self.flags & FLAG_LOCKABLE != 0
    }

    /// The same instruction with its relocatable field set to `disp`: for
    /// a PC-relative field the distance from the end of the instruction
    /// to the target, for an absolute field the value. `None` when it has
    /// no such field or `disp` does not fit it.
    #[inline]
    pub const fn with_disp(self, disp: i64) -> Option<Self> {
        let Some(site) = self.site() else { return None };
        let mut out = self;
        if site.fixup.patch(&mut out.bytes, site.offset, disp) { Some(out) } else { None }
    }

    #[inline]
    pub(crate) const fn push(&mut self, b: u8) {
        self.bytes[self.len as usize] = b;
        self.len += 1;
    }

    #[inline]
    pub(crate) const fn push_imm(&mut self, v: u64, n: u8) {
        let mut i = 0;
        while i < n {
            self.push((v >> (8 * i)) as u8);
            i += 1;
        }
    }

    /// Marks the next `fixup.size()` bytes, to be pushed next, as the
    /// relocatable field.
    #[inline]
    pub(crate) const fn mark_site(&mut self, fixup: Fixup) {
        self.site = self.len;
        self.flags |= match fixup {
            Fixup::Rel8 => SITE_REL8,
            Fixup::Rel32 => SITE_REL32,
            Fixup::Abs32 => SITE_ABS32,
            Fixup::Abs32U => SITE_ABS32U,
            Fixup::Abs64 => SITE_ABS64,
        };
    }

    #[inline]
    pub(crate) const fn lockable(mut self) -> Self {
        self.flags |= FLAG_LOCKABLE;
        self
    }

    /// Inserts `F0` at the lock position.
    #[inline]
    pub(crate) const fn insert_lock(self) -> Self {
        let mut out = self;
        let at = self.lock_at as usize;
        out.bytes[at] = 0xf0;
        let mut i = at;
        while i < self.len as usize {
            out.bytes[i + 1] = self.bytes[i];
            i += 1;
        }
        out.len += 1;
        if out.site != 0 {
            out.site += 1;
        }
        out.flags &= !FLAG_LOCKABLE;
        out
    }
}

/// The register-or-memory operand of a `ModRM` instruction.
#[derive(Clone, Copy)]
pub(crate) enum Rm {
    Reg(Reg),
    Mem(Mem),
}

/// Everything before the operands: prefixes and opcode.
#[derive(Clone, Copy)]
pub(crate) struct Op {
    /// Operand-size prefix `66`.
    pub p66: bool,
    /// A mandatory `F2`/`F3` prefix, or 0.
    pub rep: u8,
    /// `REX.W`.
    pub w: bool,
    /// The opcode is in the `0F` map.
    pub map0f: bool,
    pub opcode: u8,
}

impl Op {
    /// A one-byte-map opcode.
    pub(crate) const fn one(opcode: u8) -> Self {
        Self { p66: false, rep: 0, w: false, map0f: false, opcode }
    }

    /// A `0F`-map opcode.
    pub(crate) const fn two(opcode: u8) -> Self {
        Self { p66: false, rep: 0, w: false, map0f: true, opcode }
    }

    /// With the operand-size prefixes for `size`; for [`Size::B`], the
    /// opcode as given (the byte form), otherwise `opcode | wide`.
    pub(crate) const fn sized(self, size: crate::Size, wide: u8) -> Self {
        let opcode = if matches!(size, crate::Size::B) { self.opcode } else { self.opcode | wide };
        Self {
            p66: matches!(size, crate::Size::W),
            w: matches!(size, crate::Size::Q),
            opcode,
            ..self
        }
    }

    pub(crate) const fn rep(self, rep: u8) -> Self {
        Self { rep, ..self }
    }
}

/// The REX bits and whether a REX prefix is needed regardless.
#[derive(Clone, Copy)]
struct Rex {
    bits: u8,
    force: bool,
}

/// Prefixes, REX and opcode. `seg` is the memory operand's segment
/// override.
const fn head(out: &mut Inst, op: Op, seg: Option<crate::Seg>, rex: Rex) {
    if let Some(seg) = seg {
        out.push(seg.prefix());
    }
    if op.p66 {
        out.push(0x66);
    }
    out.lock_at = out.len;
    if op.rep != 0 {
        out.push(op.rep);
    }
    let bits = rex.bits | if op.w { 8 } else { 0 };
    if bits != 0 || rex.force {
        out.push(0x40 | bits);
        out.flags |= FLAG_REX;
    }
    if op.map0f {
        out.push(0x0f);
    }
    out.push(op.opcode);
}

/// An instruction with a `ModRM` byte. `reg` is the reg field (a register
/// number, or the `/digit` opcode extension); `reg_byte` says it holds an
/// 8-bit register and `rm_byte` that a register `rm` is one. Immediate
/// bytes are pushed by the caller.
pub(crate) const fn modrm(op: Op, reg: u8, reg_byte: bool, rm: Rm, rm_byte: bool) -> Inst {
    let mut out = Inst::EMPTY;
    let r = (reg >> 3) & 1;
    let force_reg = reg_byte && reg >= 4 && reg < 8;
    match rm {
        Rm::Reg(rm) => {
            let force = force_reg || (rm_byte && rm.needs_rex_as_byte());
            head(&mut out, op, None, Rex { bits: r << 2 | rm.high(), force });
            out.push(0xc0 | (reg & 7) << 3 | rm.low());
        }
        Rm::Mem(m) => {
            let (x, b) = match (m.get_index(), m.get_base()) {
                (Some((i, _)), Base::Reg(base)) => (i.reg().high(), base.high()),
                (Some((i, _)), _) => (i.reg().high(), 0),
                (None, Base::Reg(base)) => (0, base.high()),
                (None, _) => (0, 0),
            };
            head(&mut out, op, m.seg(), Rex { bits: r << 2 | x << 1 | b, force: force_reg });
            mem_operand(&mut out, reg & 7, m);
        }
    }
    out
}

/// `ModRM`, `SIB` and displacement for a memory operand: the shortest
/// displacement (always `disp32` for an operand marked `with_abs32`, whose
/// displacement is then the instruction's site), a `SIB` byte only when
/// the base is `rsp`/`r12`, there is an index, or there is neither base nor
/// RIP (`spec/mc-x64.md` "Memory operands").
const fn mem_operand(out: &mut Inst, reg: u8, m: Mem) {
    let disp = m.disp();
    let reg = reg << 3;
    match (m.get_base(), m.get_index()) {
        (Base::Rip, _) => {
            out.push(reg | 0b101);
            out.mark_site(Fixup::Rel32);
            out.push_imm(disp as u32 as u64, 4);
        }
        (Base::None, index) => {
            out.push(reg | 0b100);
            let sib = match index {
                Some((i, s)) => (s as u8) << 6 | i.reg().low() << 3 | 0b101,
                None => 0b00_100_101,
            };
            out.push(sib);
            if m.is_abs32() {
                out.mark_site(Fixup::Abs32);
            }
            out.push_imm(disp as u32 as u64, 4);
        }
        (Base::Reg(base), index) => {
            // mod: no displacement, disp8 or disp32. A base whose low bits
            // are 101 (rbp, r13) has no "no displacement" form.
            let md = if m.is_abs32() {
                0b10
            } else if disp == 0 && base.low() != 0b101 {
                0b00
            } else if disp >= -128 && disp <= 127 {
                0b01
            } else {
                0b10
            };
            match index {
                Some((i, s)) => {
                    out.push(md << 6 | reg | 0b100);
                    out.push((s as u8) << 6 | i.reg().low() << 3 | base.low());
                }
                None if base.low() == 0b100 => {
                    out.push(md << 6 | reg | 0b100);
                    out.push(0b00_100_100);
                }
                None => out.push(md << 6 | reg | base.low()),
            }
            match md {
                0b01 => out.push(disp as u8),
                0b10 => {
                    if m.is_abs32() {
                        out.mark_site(Fixup::Abs32);
                    }
                    out.push_imm(disp as u32 as u64, 4);
                }
                _ => {}
            }
        }
    }
}

/// An instruction whose register is in the opcode's low three bits
/// (`B8+r`, `50+r`, `0F C8+r`, ...). `byte` says it is an 8-bit register.
pub(crate) const fn plus_r(op: Op, r: Reg, byte: bool) -> Inst {
    let mut out = Inst::EMPTY;
    let op = Op { opcode: op.opcode | r.low(), ..op };
    head(&mut out, op, None, Rex { bits: r.high(), force: byte && r.needs_rex_as_byte() });
    out
}

/// An instruction with no register operand in its encoding.
pub(crate) const fn plain(op: Op) -> Inst {
    let mut out = Inst::EMPTY;
    head(&mut out, op, None, Rex { bits: 0, force: false });
    out
}

/// Appends an immediate of `n` bytes.
pub(crate) const fn imm(mut inst: Inst, v: i64, n: u8) -> Inst {
    inst.push_imm(v as u64, n);
    inst
}

/// Appends an absolute immediate field of kind `fixup` holding `v`: the
/// instruction's site.
pub(crate) const fn abs_imm(mut inst: Inst, fixup: Fixup, v: i64) -> Inst {
    inst.mark_site(fixup);
    inst.push_imm(v as u64, fixup.size() as u8);
    inst
}

/// Appends a PC-relative field holding `disp`.
pub(crate) const fn rel(mut inst: Inst, fixup: Fixup, disp: i64) -> Inst {
    inst.mark_site(fixup);
    inst.push_imm(disp as u64, fixup.size() as u8);
    inst
}

/// `modrm` for a register-register form: `reg` and `rm` both general
/// registers of `size`.
pub(crate) const fn rr(op: Op, size: crate::Size, reg: Reg, rm: Reg) -> Inst {
    let byte = matches!(size, crate::Size::B);
    modrm(op, reg.code(), byte, Rm::Reg(rm), byte)
}

/// `modrm` for a register-memory form: `reg` a general register of `size`.
pub(crate) const fn rmem(op: Op, size: crate::Size, reg: Reg, m: Mem) -> Inst {
    modrm(op, reg.code(), matches!(size, crate::Size::B), Rm::Mem(m), false)
}

/// `modrm` with an opcode extension `/digit` and a register of `size`.
pub(crate) const fn digit_r(op: Op, digit: u8, size: crate::Size, rm: Reg) -> Inst {
    modrm(op, digit, false, Rm::Reg(rm), matches!(size, crate::Size::B))
}

/// `modrm` with an opcode extension `/digit` and a memory operand.
pub(crate) const fn digit_m(op: Op, digit: u8, m: Mem) -> Inst {
    modrm(op, digit, false, Rm::Mem(m), false)
}
