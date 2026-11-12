//! Assembly-text formatting and operand samples for the L1 tests. Nothing
//! here uses the encoder: register names come from register numbers,
//! memory operands are formatted from the parameters they were built from,
//! operation names come from tables written out below.

use hod_mc_x64::{Cond, Index, Mem, Reg, Scale, Seg, Size};

/// The register numbered `n`.
pub fn r(n: u8) -> Reg {
    Reg::new(n).expect("register number 0..=15")
}

/// The index register numbered `n` (not 4).
pub fn ix(n: u8) -> Index {
    r(n).index().expect("an index register")
}

const Q: [&str; 16] = [
    "rax", "rcx", "rdx", "rbx", "rsp", "rbp", "rsi", "rdi", "r8", "r9", "r10", "r11", "r12", "r13",
    "r14", "r15",
];
const D: [&str; 8] = ["eax", "ecx", "edx", "ebx", "esp", "ebp", "esi", "edi"];
const W: [&str; 8] = ["ax", "cx", "dx", "bx", "sp", "bp", "si", "di"];
const B: [&str; 8] = ["al", "cl", "dl", "bl", "spl", "bpl", "sil", "dil"];

/// The name of register `n` at `size`.
pub fn reg(size: Size, n: u8) -> String {
    let n = n as usize;
    match (size, n) {
        (Size::Q, _) => Q[n].to_string(),
        (Size::D, 0..8) => D[n].to_string(),
        (Size::W, 0..8) => W[n].to_string(),
        (Size::B, 0..8) => B[n].to_string(),
        (Size::D, _) => format!("r{n}d"),
        (Size::W, _) => format!("r{n}w"),
        (Size::B, _) => format!("r{n}b"),
    }
}

/// `byte ptr` .. `qword ptr`.
pub fn ptr(size: Size) -> &'static str {
    match size {
        Size::B => "byte ptr",
        Size::W => "word ptr",
        Size::D => "dword ptr",
        Size::Q => "qword ptr",
    }
}

pub const SIZES: [Size; 4] = [Size::B, Size::W, Size::D, Size::Q];
/// The sizes of instructions with no byte form.
pub const WIDE: [Size; 3] = [Size::W, Size::D, Size::Q];

/// Condition suffixes in `cc` field order, as LLVM prints them.
pub const CONDS: [(Cond, &str); 16] = [
    (Cond::O, "o"),
    (Cond::No, "no"),
    (Cond::B, "b"),
    (Cond::Ae, "ae"),
    (Cond::E, "e"),
    (Cond::Ne, "ne"),
    (Cond::Be, "be"),
    (Cond::A, "a"),
    (Cond::S, "s"),
    (Cond::Ns, "ns"),
    (Cond::P, "p"),
    (Cond::Np, "np"),
    (Cond::L, "l"),
    (Cond::Ge, "ge"),
    (Cond::Le, "le"),
    (Cond::G, "g"),
];

/// A memory operand as the test writes it: its parts, from which both the
/// crate's [`Mem`] and the assembly text are made.
#[derive(Clone, Copy, Debug)]
pub struct M {
    pub base: MBase,
    pub index: Option<(u8, u8)>,
    pub disp: i32,
    pub seg: Option<Seg>,
}

#[derive(Clone, Copy, Debug)]
pub enum MBase {
    None,
    Reg(u8),
    Rip,
}

impl M {
    pub fn base(b: u8, disp: i32) -> Self {
        Self { base: MBase::Reg(b), index: None, disp, seg: None }
    }

    pub fn bi(b: u8, i: u8, scale: u8, disp: i32) -> Self {
        Self { base: MBase::Reg(b), index: Some((i, scale)), disp, seg: None }
    }

    pub fn index(i: u8, scale: u8, disp: i32) -> Self {
        Self { base: MBase::None, index: Some((i, scale)), disp, seg: None }
    }

    pub fn abs(disp: i32) -> Self {
        Self { base: MBase::None, index: None, disp, seg: None }
    }

    pub fn rip(disp: i32) -> Self {
        Self { base: MBase::Rip, index: None, disp, seg: None }
    }

    pub fn seg(self, seg: Seg) -> Self {
        Self { seg: Some(seg), ..self }
    }

    fn scale(factor: u8) -> Scale {
        match factor {
            1 => Scale::S1,
            2 => Scale::S2,
            4 => Scale::S4,
            8 => Scale::S8,
            _ => panic!("scale {factor}"),
        }
    }

    /// The crate's operand.
    pub fn mem(self) -> Mem {
        let m = match (self.base, self.index) {
            (MBase::Reg(b), None) => Mem::base(r(b), self.disp),
            (MBase::Reg(b), Some((i, s))) => {
                Mem::base_index(r(b), ix(i), Self::scale(s), self.disp)
            }
            (MBase::None, Some((i, s))) => Mem::index(ix(i), Self::scale(s), self.disp),
            (MBase::None, None) => Mem::abs(self.disp),
            (MBase::Rip, None) => Mem::rip(self.disp),
            (MBase::Rip, Some(_)) => panic!("no RIP-relative operand has an index"),
        };
        match self.seg {
            Some(seg) => m.with_seg(seg),
            None => m,
        }
    }

    /// `fs:[base + index*scale + disp]`, without a size.
    pub fn text(self) -> String {
        let mut terms = Vec::new();
        match self.base {
            MBase::Reg(b) => terms.push(Q[b as usize].to_string()),
            MBase::Rip => terms.push("rip".to_string()),
            MBase::None => {}
        }
        if let Some((i, s)) = self.index {
            terms.push(format!("{}*{s}", Q[i as usize]));
        }
        let mut text = terms.join(" + ");
        if terms.is_empty() {
            text = self.disp.to_string();
        } else if self.disp > 0 {
            text = format!("{text} + {}", self.disp);
        } else if self.disp < 0 {
            text = format!("{text} - {}", (self.disp as i64).unsigned_abs());
        }
        let seg = match self.seg {
            Some(Seg::Fs) => "fs:",
            Some(Seg::Gs) => "gs:",
            None => "",
        };
        format!("{seg}[{text}]")
    }

    /// `dword ptr fs:[..]`.
    pub fn sized(self, size: Size) -> String {
        format!("{} {}", ptr(size), self.text())
    }
}

/// Displacements at every boundary of the disp8/disp32 choice.
pub const DISPS: [i32; 10] = [0, 1, -1, 127, -128, 128, -129, 0x1234_5678, i32::MAX, i32::MIN];

/// Every memory form: each base with each displacement class; each base
/// with each index and scale; no base; absolute; RIP-relative; segments.
pub fn all_mems() -> Vec<M> {
    let mut out = Vec::new();
    for b in 0..16 {
        for d in DISPS {
            out.push(M::base(b, d));
        }
        for i in (0..16).filter(|&i| i != 4) {
            for s in [1, 2, 4, 8] {
                out.push(M::bi(b, i, s, DISPS[(b as usize + i as usize + s as usize) % 7]));
            }
        }
        for d in [0, 1, -128, 128, i32::MIN] {
            out.push(M::bi(b, (b + 5) % 16 | 1, 4, d));
        }
    }
    for i in (0..16).filter(|&i| i != 4) {
        for s in [1, 2, 4, 8] {
            for d in [0, -1, 0x7fff_ffff] {
                out.push(M::index(i, s, d));
            }
        }
    }
    for d in DISPS {
        out.push(M::abs(d));
        out.push(M::rip(d));
    }
    for seg in [Seg::Fs, Seg::Gs] {
        out.push(M::abs(0).seg(seg));
        out.push(M::abs(0x28).seg(seg));
        out.push(M::base(0, 8).seg(seg));
        out.push(M::base(13, 0).seg(seg));
        out.push(M::bi(12, 9, 8, -4).seg(seg));
        out.push(M::rip(16).seg(seg));
    }
    out
}

/// A sample of memory forms for the instruction sweeps: each special case
/// of the ModRM/SIB layout and each REX bit.
pub fn some_mems() -> Vec<M> {
    vec![
        M::base(0, 0),
        M::base(3, 8),
        M::base(4, 0),
        M::base(5, 0),
        M::base(12, -8),
        M::base(13, 0),
        M::base(15, 0x1000),
        M::base(7, i32::MIN),
        M::bi(0, 1, 1, 0),
        M::bi(5, 9, 8, 0),
        M::bi(4, 5, 2, 0x80),
        M::bi(13, 12, 4, -1),
        M::bi(9, 14, 8, 0x7fff_ffff),
        M::index(2, 4, 16),
        M::index(11, 1, 0),
        M::abs(0x40),
        M::rip(0),
        M::rip(-0x20),
        M::base(1, 0x28).seg(Seg::Fs),
        M::abs(0x28).seg(Seg::Fs),
        M::bi(8, 10, 2, 3).seg(Seg::Gs),
    ]
}

/// A small deterministic generator for samples.
#[derive(Debug)]
pub struct Rng(u64);

impl Rng {
    pub fn new(seed: u64) -> Self {
        Self(seed.wrapping_mul(0x9e37_79b9_7f4a_7c15) | 1)
    }

    pub fn next(&mut self) -> u64 {
        // xorshift64*
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        self.0.wrapping_mul(0x2545_f491_4f6c_dd1d)
    }
}

/// Immediates at the boundaries of each size's forms, with the ones that
/// fit `size`.
pub fn imms(size: Size) -> Vec<i64> {
    let all = [
        0,
        1,
        -1,
        5,
        127,
        -128,
        128,
        -129,
        255,
        0x1234,
        0x7fff,
        -0x8000,
        0x8000,
        0xffff,
        -0x8001,
        0x1_0000,
        0x1234_5678,
        0x7fff_ffff,
        -0x8000_0000,
        0x8000_0000,
        0xffff_ffff,
    ];
    all.into_iter().filter(|&v| fits(size, v)).collect()
}

/// Whether `v` is an immediate of `size`, stated independently of the
/// crate.
pub fn fits(size: Size, v: i64) -> bool {
    match size {
        Size::B => (-128..=255).contains(&v),
        Size::W => (-32768..=65535).contains(&v),
        Size::D => (-(1 << 31)..=(1 << 32) - 1).contains(&v),
        Size::Q => (-(1 << 31)..(1 << 31)).contains(&v),
    }
}

/// Values just outside `size`'s immediates.
pub fn bad_imms(size: Size) -> Vec<i64> {
    match size {
        Size::B => vec![256, -129, 0x1000],
        Size::W => vec![0x1_0000, -0x8001],
        Size::D => vec![0x1_0000_0000, -0x8000_0001],
        Size::Q => vec![0x8000_0000, -0x8000_0001, 0xffff_ffff],
    }
}

/// Whether LLVM refuses an immediate that does not fit `size`. Its
/// assembler truncates an 8-, 16- or 32-bit immediate that does not fit
/// (`add al, 256` is `add al, 0`), so only the 64-bit rejections have text
/// it refuses: `spec/mc-x64.md` "Where hod and LLVM's assembler differ".
pub fn llvm_checks_imm(size: Size) -> bool {
    size == Size::Q
}
