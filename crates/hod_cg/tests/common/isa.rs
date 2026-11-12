//! The instruction set of the test machine ("TM").
//!
//! A byte-coded register machine that exists only to run what `hod_cg`
//! generates: twelve general registers, eight float registers, a frame
//! pointer, an argument pointer and a stack pointer. Its encoder is used
//! by the test target (`target.rs`) and its decoder by the executor
//! (`exec.rs`).
//!
//! Every instruction is an opcode byte followed by its fields. Operands
//! of the arithmetic instructions are [`Opnd`]s, so one instruction form
//! serves a value in a register and a value in a frame slot (the `Mem`
//! class of `spec/codegen.md` §2.2), at any width up to 128 bits.
//!
//! A register is 64 bits wide. An instruction that produces a value
//! narrower than that leaves **ones** in the bits above it, so code that
//! reads a value wider than it was produced computes something wrong
//! instead of something accidentally right (§2.2: those bits are
//! unspecified).

/// The number of registers: general registers are `0..12`, float
/// registers `16..24`.
pub const REGS: usize = 24;
/// The first float register.
pub const FPR0: u8 = 16;
/// Memory-operand bases that are not numbered registers.
pub const FP: u8 = 0xF0;
pub const AP: u8 = 0xF1;
pub const SP: u8 = 0xF2;

/// How far a short branch form reaches, in bytes, either way. Small on
/// purpose: generated functions regularly outgrow it, so the far-mode
/// retry (§6.3) is exercised by ordinary programs.
pub const NEAR_REACH: i64 = 1024;
/// How far a far branch form reaches: a function beyond it is
/// `limit:function-size`.
pub const FAR_REACH: i64 = 1 << 20;

pub const ENTER: u8 = 0x01;
pub const LEAVE: u8 = 0x02;
pub const RET: u8 = 0x03;
pub const TRAP: u8 = 0x04;
pub const MOV: u8 = 0x05;
pub const MOVI: u8 = 0x06;
pub const LEA: u8 = 0x07;
pub const LOAD: u8 = 0x08;
pub const STORE: u8 = 0x09;
pub const COPY: u8 = 0x0A;
pub const BIN: u8 = 0x0B;
pub const ICMP: u8 = 0x0C;
pub const FBIN: u8 = 0x0D;
pub const FNEG: u8 = 0x0E;
pub const FCMP: u8 = 0x0F;
pub const CAST: u8 = 0x10;
pub const SEL: u8 = 0x11;
pub const INTR: u8 = 0x12;
pub const CHECKED: u8 = 0x13;
pub const JMP: u8 = 0x14;
pub const JMPF: u8 = 0x15;
pub const JNZ: u8 = 0x16;
pub const JNZF: u8 = 0x17;
pub const JCMP: u8 = 0x18;
pub const JCMPF: u8 = 0x19;
pub const CALL: u8 = 0x1A;
pub const CALLR: u8 = 0x1B;
pub const TAILJ: u8 = 0x1C;
pub const TAILR: u8 = 0x1D;
pub const ADDSP: u8 = 0x1E;
pub const LDPC: u8 = 0x1F;
pub const LDPCF: u8 = 0x20;
pub const DYNSLOT: u8 = 0x21;
pub const CHK: u8 = 0x22;
pub const JMPR: u8 = 0x23;
pub const LEAPC: u8 = 0x24;

/// The integer operations of `INTR`.
pub mod intr {
    pub const ABS: u8 = 0;
    pub const SMAX: u8 = 1;
    pub const SMIN: u8 = 2;
    pub const UMAX: u8 = 3;
    pub const UMIN: u8 = 4;
    pub const CTPOP: u8 = 5;
    pub const CTLZ: u8 = 6;
    pub const CTTZ: u8 = 7;
    pub const BSWAP: u8 = 8;
    pub const BITREVERSE: u8 = 9;
    pub const FSHL: u8 = 10;
    pub const FSHR: u8 = 11;
    pub const SADD_SAT: u8 = 12;
    pub const UADD_SAT: u8 = 13;
    pub const SSUB_SAT: u8 = 14;
    pub const USUB_SAT: u8 = 15;
}

/// The operations of `CHECKED`.
pub mod checked {
    pub const SADD: u8 = 0;
    pub const UADD: u8 = 1;
    pub const SSUB: u8 = 2;
    pub const USUB: u8 = 3;
    pub const SMUL: u8 = 4;
    pub const UMUL: u8 = 5;
}

/// An operand: a register, memory at a base plus a displacement, or an
/// immediate.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Opnd {
    Reg(u8),
    /// `base` is a register number, or [`FP`], [`AP`], [`SP`].
    Mem {
        base: u8,
        disp: i32,
    },
    Imm(u64),
}

/// An instruction being encoded: `Enc::op(BIN).u8(..).opnd(..).bytes()`.
#[derive(Clone, Copy, Debug)]
pub struct Enc {
    bytes: [u8; 64],
    len: usize,
}

impl Enc {
    pub fn op(op: u8) -> Enc {
        let mut enc = Enc { bytes: [0; 64], len: 1 };
        enc.bytes[0] = op;
        enc
    }

    fn put(mut self, bytes: &[u8]) -> Enc {
        self.bytes[self.len..self.len + bytes.len()].copy_from_slice(bytes);
        self.len += bytes.len();
        self
    }

    pub fn u8(self, value: u8) -> Enc {
        self.put(&[value])
    }

    pub fn u16(self, value: u16) -> Enc {
        self.put(&value.to_le_bytes())
    }

    pub fn u32(self, value: u32) -> Enc {
        self.put(&value.to_le_bytes())
    }

    pub fn u64(self, value: u64) -> Enc {
        self.put(&value.to_le_bytes())
    }

    pub fn opnd(self, opnd: Opnd) -> Enc {
        match opnd {
            Opnd::Reg(reg) => self.u8(0).u8(reg),
            Opnd::Mem { base, disp } => self.u8(1).u8(base).u32(disp as u32),
            Opnd::Imm(value) => self.u8(2).u64(value),
        }
    }

    /// The length so far: the offset of the next field.
    pub fn len(&self) -> u32 {
        self.len as u32
    }

    pub fn bytes(&self) -> &[u8] {
        &self.bytes[..self.len]
    }
}

/// A decoded instruction. Branch targets are code offsets.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Ins {
    Enter { frame: u32 },
    Leave,
    Ret,
    Trap,
    Mov { dst: u8, src: u8 },
    MovI { dst: u8, imm: u64 },
    Lea { dst: u8, mem: Opnd },
    Load { dst: u8, src: Opnd, bits: u8 },
    Store { src: u8, dst: Opnd, bits: u8 },
    Copy { dst: Opnd, src: Opnd, bytes: u32 },
    Bin { op: u8, bits: u8, dst: Opnd, a: Opnd, b: Opnd },
    ICmp { pred: u8, bits: u8, dst: u8, a: Opnd, b: Opnd },
    FBin { op: u8, double: bool, dst: u8, a: u8, b: u8 },
    FNeg { double: bool, dst: u8, src: u8 },
    FCmp { pred: u8, double: bool, dst: u8, a: u8, b: u8 },
    Cast { op: u8, from: u8, to: u8, dst: Opnd, src: Opnd },
    Sel { bits: u8, dst: Opnd, cond: u8, a: Opnd, b: Opnd },
    Intr { op: u8, bits: u8, dst: Opnd, a: Opnd, b: Opnd, c: Opnd },
    Checked { op: u8, bits: u8, dst: Opnd, flag_at: u32, a: Opnd, b: Opnd },
    Jmp { target: u32 },
    Jnz { reg: u8, target: u32 },
    JCmp { pred: u8, bits: u8, a: Opnd, b: Opnd, target: u32 },
    Call { addr: u64 },
    CallR { reg: u8 },
    TailJ { addr: u64 },
    TailR { reg: u8 },
    AddSp { delta: i32 },
    LdPc { dst: u8, bits: u8, target: u32 },
    LeaPc { dst: u8, target: u32 },
    JmpR { reg: u8 },
    DynSlot { ptr: Opnd, size: u32, align: u32 },
    Chk { reg: u8, bits: u8, signed: bool },
}

struct Dec<'a> {
    code: &'a [u8],
    at: usize,
}

impl Dec<'_> {
    fn take<const N: usize>(&mut self) -> Result<[u8; N], String> {
        let bytes =
            self.code.get(self.at..self.at + N).ok_or("an instruction runs off the code")?;
        self.at += N;
        let mut out = [0; N];
        out.copy_from_slice(bytes);
        Ok(out)
    }

    fn u8(&mut self) -> Result<u8, String> {
        Ok(self.take::<1>()?[0])
    }

    fn flag(&mut self) -> Result<bool, String> {
        Ok(self.u8()? != 0)
    }

    fn u32(&mut self) -> Result<u32, String> {
        Ok(u32::from_le_bytes(self.take()?))
    }

    fn u64(&mut self) -> Result<u64, String> {
        Ok(u64::from_le_bytes(self.take()?))
    }

    fn opnd(&mut self) -> Result<Opnd, String> {
        match self.u8()? {
            0 => Ok(Opnd::Reg(self.u8()?)),
            1 => Ok(Opnd::Mem { base: self.u8()?, disp: self.u32()? as i32 }),
            2 => Ok(Opnd::Imm(self.u64()?)),
            tag => Err(format!("operand tag {tag}")),
        }
    }

    /// A branch field: the target is the field's offset plus its value.
    fn rel(&mut self, far: bool) -> Result<u32, String> {
        let at = self.at as i64;
        let rel = if far {
            i64::from(i32::from_le_bytes(self.take()?))
        } else {
            i64::from(i16::from_le_bytes(self.take()?))
        };
        u32::try_from(at + rel).map_err(|_| "a branch to before the code".to_owned())
    }
}

/// Decodes the instruction at `pc`: the instruction and the offset of
/// the next one.
pub fn decode(code: &[u8], pc: usize) -> Result<(Ins, usize), String> {
    let mut d = Dec { code, at: pc };
    let op = d.u8()?;
    let ins = match op {
        ENTER => Ins::Enter { frame: d.u32()? },
        LEAVE => Ins::Leave,
        RET => Ins::Ret,
        TRAP => Ins::Trap,
        MOV => Ins::Mov { dst: d.u8()?, src: d.u8()? },
        MOVI => Ins::MovI { dst: d.u8()?, imm: d.u64()? },
        LEA => Ins::Lea { dst: d.u8()?, mem: d.opnd()? },
        LOAD => Ins::Load { dst: d.u8()?, src: d.opnd()?, bits: d.u8()? },
        STORE => Ins::Store { src: d.u8()?, dst: d.opnd()?, bits: d.u8()? },
        COPY => Ins::Copy { dst: d.opnd()?, src: d.opnd()?, bytes: d.u32()? },
        BIN => Ins::Bin { op: d.u8()?, bits: d.u8()?, dst: d.opnd()?, a: d.opnd()?, b: d.opnd()? },
        ICMP => {
            Ins::ICmp { pred: d.u8()?, bits: d.u8()?, dst: d.u8()?, a: d.opnd()?, b: d.opnd()? }
        }
        FBIN => Ins::FBin { op: d.u8()?, double: d.flag()?, dst: d.u8()?, a: d.u8()?, b: d.u8()? },
        FNEG => Ins::FNeg { double: d.flag()?, dst: d.u8()?, src: d.u8()? },
        FCMP => {
            Ins::FCmp { pred: d.u8()?, double: d.flag()?, dst: d.u8()?, a: d.u8()?, b: d.u8()? }
        }
        CAST => {
            Ins::Cast { op: d.u8()?, from: d.u8()?, to: d.u8()?, dst: d.opnd()?, src: d.opnd()? }
        }
        SEL => {
            Ins::Sel { bits: d.u8()?, dst: d.opnd()?, cond: d.u8()?, a: d.opnd()?, b: d.opnd()? }
        }
        INTR => Ins::Intr {
            op: d.u8()?,
            bits: d.u8()?,
            dst: d.opnd()?,
            a: d.opnd()?,
            b: d.opnd()?,
            c: d.opnd()?,
        },
        CHECKED => Ins::Checked {
            op: d.u8()?,
            bits: d.u8()?,
            dst: d.opnd()?,
            flag_at: d.u32()?,
            a: d.opnd()?,
            b: d.opnd()?,
        },
        JMP | JMPF => Ins::Jmp { target: d.rel(op == JMPF)? },
        JNZ | JNZF => Ins::Jnz { reg: d.u8()?, target: d.rel(op == JNZF)? },
        JCMP | JCMPF => Ins::JCmp {
            pred: d.u8()?,
            bits: d.u8()?,
            a: d.opnd()?,
            b: d.opnd()?,
            target: d.rel(op == JCMPF)?,
        },
        CALL => Ins::Call { addr: d.u64()? },
        CALLR => Ins::CallR { reg: d.u8()? },
        TAILJ => Ins::TailJ { addr: d.u64()? },
        TAILR => Ins::TailR { reg: d.u8()? },
        ADDSP => Ins::AddSp { delta: d.u32()? as i32 },
        LDPC | LDPCF => Ins::LdPc { dst: d.u8()?, bits: d.u8()?, target: d.rel(op == LDPCF)? },
        LEAPC => Ins::LeaPc { dst: d.u8()?, target: d.rel(true)? },
        JMPR => Ins::JmpR { reg: d.u8()? },
        DYNSLOT => Ins::DynSlot { ptr: d.opnd()?, size: d.u32()?, align: d.u32()? },
        CHK => Ins::Chk { reg: d.u8()?, bits: d.u8()?, signed: d.flag()? },
        other => return Err(format!("opcode {other:#04x} at {pc:#x} is no instruction")),
    };
    Ok((ins, d.at))
}
