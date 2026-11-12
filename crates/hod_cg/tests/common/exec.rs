//! The executor of the test machine: runs the code and records of a
//! `Chunk` that the test target (`target.rs`) produced.
//!
//! It is strict where a real machine is forgiving, so that a wrong
//! policy shows as a failed run and not as a lucky one:
//!
//! - a call overwrites every register that is not an argument register,
//!   and a return every register that is not a return register;
//! - a narrow result leaves ones above its width (see `isa.rs`);
//! - the stack starts out filled with `0xAA`;
//! - a `ret` checks that the stack, frame and argument pointers are what
//!   they were at the call;
//! - any access outside the data and the stack is an error.
//!
//! Its arithmetic is its own, written from `spec/interp.md` §4: the
//! interpreter's `ops` module is private to `hod_interp`.

use std::sync::Arc;

use hod_cg::{Chunk, ClauseRow, FuncRecord, NO_PAD, RuntimeFn, SymRef};
use hod_interp::{Event, Val, host};
use hod_ir::{FloatPredicate, IntPredicate, Module, Opcode, SymId, SymbolKind};

use super::isa::{self, Ins, Opnd, REGS};

/// Where the data and the stack are.
const BASE: u64 = 0x1_0000;
const STACK_BYTES: usize = 256 << 10;
/// The addresses of functions, of runtime functions and of code.
const FN_BASE: u64 = 0x4000_0000_0000;
const RT_BASE: u64 = 0x5000_0000_0000;
const CODE_BASE: u64 = 0x6000_0000_0000;
/// What a register holds after something clobbered it.
const POISON: u64 = 0xDEAD_0BAD_F00D_DEAD;
/// The most instructions a run executes.
const MAX_STEPS: u64 = 50_000_000;

/// What a run produced.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Ran {
    /// The two general return registers.
    pub ret: [u64; 2],
    /// The float return register.
    pub fret: u64,
    /// The calls of host functions, as the interpreter records them.
    pub trace: Vec<Event>,
    /// Instructions executed.
    pub steps: u64,
}

/// A call that has not returned.
#[derive(Clone, Copy)]
struct Frame {
    ret: usize,
    sp: u64,
    fp: u64,
    ap: u64,
}

struct Machine<'m> {
    module: &'m Module,
    chunk: &'m Chunk,
    code: Vec<u8>,
    /// By symbol: the offset of its code.
    entry_of: Vec<Option<usize>>,
    mem: Vec<u8>,
    regs: [u64; REGS],
    fp: u64,
    ap: u64,
    sp: u64,
    pc: usize,
    calls: Vec<Frame>,
    trace: Vec<Event>,
    names: [Arc<str>; 2],
    source_calls: u64,
    done: bool,
}

fn mask(bits: u8) -> u128 {
    if bits >= 128 { u128::MAX } else { (1 << bits) - 1 }
}

fn sext(bits: u8, value: u128) -> i128 {
    let shift = 128 - u32::from(bits.clamp(1, 128));
    ((value << shift) as i128) >> shift
}

fn int_binary(op: Opcode, bits: u8, a: u128, b: u128) -> Result<u128, String> {
    let (sa, sb) = (sext(bits, a), sext(bits, b));
    let shift = || {
        if b >= u128::from(bits) { Err("a shift by the width or more".to_owned()) } else { Ok(b as u32) }
    };
    let value = match op {
        Opcode::Add => a.wrapping_add(b),
        Opcode::Sub => a.wrapping_sub(b),
        Opcode::Mul => a.wrapping_mul(b),
        Opcode::UDiv => a.checked_div(b).ok_or("division by zero")?,
        Opcode::URem => a.checked_rem(b).ok_or("division by zero")?,
        Opcode::SDiv | Opcode::SRem => {
            if sb == 0 {
                return Err("division by zero".to_owned());
            }
            if sb == -1 && sa == sext(bits, 1 << (bits - 1)) {
                return Err("signed division overflow".to_owned());
            }
            (if op == Opcode::SDiv { sa.wrapping_div(sb) } else { sa.wrapping_rem(sb) }) as u128
        }
        Opcode::Shl => a << shift()?,
        Opcode::LShr => a >> shift()?,
        Opcode::AShr => (sa >> shift()?) as u128,
        Opcode::And => a & b,
        Opcode::Or => a | b,
        Opcode::Xor => a ^ b,
        other => return Err(format!("{} is no integer operation", other.name())),
    };
    Ok(value & mask(bits))
}

fn icmp(pred: IntPredicate, bits: u8, a: u128, b: u128) -> bool {
    let (sa, sb) = (sext(bits, a), sext(bits, b));
    match pred {
        IntPredicate::Eq => a == b,
        IntPredicate::Ne => a != b,
        IntPredicate::Ugt => a > b,
        IntPredicate::Uge => a >= b,
        IntPredicate::Ult => a < b,
        IntPredicate::Ule => a <= b,
        IntPredicate::Sgt => sa > sb,
        IntPredicate::Sge => sa >= sb,
        IntPredicate::Slt => sa < sb,
        IntPredicate::Sle => sa <= sb,
    }
}

fn fcmp(pred: FloatPredicate, a: f64, b: f64) -> bool {
    let unordered = a.is_nan() || b.is_nan();
    match pred {
        FloatPredicate::False => false,
        FloatPredicate::Oeq => a == b,
        FloatPredicate::Ogt => a > b,
        FloatPredicate::Oge => a >= b,
        FloatPredicate::Olt => a < b,
        FloatPredicate::Ole => a <= b,
        FloatPredicate::One => !unordered && a != b,
        FloatPredicate::Ord => !unordered,
        FloatPredicate::Uno => unordered,
        FloatPredicate::Ueq => unordered || a == b,
        FloatPredicate::Ugt => unordered || a > b,
        FloatPredicate::Uge => unordered || a >= b,
        FloatPredicate::Ult => unordered || a < b,
        FloatPredicate::Ule => unordered || a <= b,
        FloatPredicate::Une => unordered || a != b,
        FloatPredicate::True => true,
    }
}

macro_rules! float_binary {
    ($op:expr, $a:expr, $b:expr) => {
        match $op {
            Opcode::FAdd => $a + $b,
            Opcode::FSub => $a - $b,
            Opcode::FMul => $a * $b,
            Opcode::FDiv => $a / $b,
            Opcode::FRem => $a % $b,
            other => return Err(format!("{} is no float operation", other.name())),
        }
    };
}

fn int_intrinsic(op: u8, bits: u8, a: u128, b: u128, c: u128) -> Result<u128, String> {
    use isa::intr;
    let all = mask(bits);
    let (sa, sb) = (sext(bits, a), sext(bits, b));
    let width = u32::from(bits);
    let (min, max) = (sext(bits, 1 << (bits - 1)), (all >> 1) as i128);
    let value = match op {
        intr::ABS => sa.wrapping_abs() as u128,
        intr::SMAX => sa.max(sb) as u128,
        intr::SMIN => sa.min(sb) as u128,
        intr::UMAX => a.max(b),
        intr::UMIN => a.min(b),
        intr::CTPOP => u128::from(a.count_ones()),
        intr::CTLZ => u128::from(if a == 0 { width } else { a.leading_zeros() - (128 - width) }),
        intr::CTTZ => u128::from(if a == 0 { width } else { a.trailing_zeros() }),
        intr::BSWAP => a.swap_bytes() >> (128 - width),
        intr::BITREVERSE => a.reverse_bits() >> (128 - width),
        intr::FSHL | intr::FSHR => {
            let amount = (c % u128::from(bits)) as u32;
            match (op == intr::FSHL, amount) {
                (true, 0) => a,
                (false, 0) => b,
                (true, _) => (a << amount) | (b >> (width - amount)),
                (false, _) => (b >> amount) | (a << (width - amount)),
            }
        }
        intr::SADD_SAT => (sa + sb).clamp(min, max) as u128,
        intr::SSUB_SAT => (sa - sb).clamp(min, max) as u128,
        intr::UADD_SAT => (a + b).min(all),
        intr::USUB_SAT => a.saturating_sub(b),
        other => return Err(format!("integer operation {other}")),
    };
    Ok(value & all)
}

/// The wrapped result and whether it overflowed. `bits` is at most 64,
/// so nothing overflows the 128-bit arithmetic it is computed in.
fn checked(op: u8, bits: u8, a: u128, b: u128) -> Result<(u128, bool), String> {
    use isa::checked;
    let all = mask(bits);
    let (sa, sb) = (sext(bits, a), sext(bits, b));
    let signed = |exact: i128| {
        let wrapped = exact as u128 & all;
        (wrapped, sext(bits, wrapped) != exact)
    };
    Ok(match op {
        checked::SADD => signed(sa + sb),
        checked::SSUB => signed(sa - sb),
        checked::SMUL => signed(sa * sb),
        checked::UADD => ((a + b) & all, a + b > all),
        checked::USUB => (a.wrapping_sub(b) & all, a < b),
        checked::UMUL => ((a * b) & all, a * b > all),
        other => return Err(format!("checked operation {other}")),
    })
}

impl<'m> Machine<'m> {
    fn new(module: &'m Module, chunk: &'m Chunk) -> Result<Machine<'m>, String> {
        // The data: every variable with an initializer, in symbol order.
        let count = module.symbol_count();
        let mut addrs = vec![0u64; count];
        let mut mem = Vec::new();
        let mut patches = Vec::new();
        for symbol in module.symbols() {
            match symbol.kind() {
                SymbolKind::Function { .. } => {
                    addrs[symbol.id().index()] = FN_BASE + 16 * symbol.id().index() as u64;
                }
                SymbolKind::Variable { init: Some(init), .. } => {
                    let align = symbol.align().unwrap_or_else(|| module.types.align_of(symbol.ty()));
                    let at = mem.len().next_multiple_of(align.max(1) as usize);
                    mem.resize(at, 0);
                    let (mut image, mut fixups) = (Vec::new(), Vec::new());
                    module
                        .consts
                        .flatten(&module.types, init, &mut image, &mut fixups)
                        .map_err(|error| format!("an initializer: {error}"))?;
                    mem.extend_from_slice(&image);
                    addrs[symbol.id().index()] = BASE + at as u64;
                    patches.extend(fixups.into_iter().map(|fixup| (at, fixup)));
                }
                _ => {}
            }
        }
        for (at, fixup) in patches {
            let hod_ir::FixupTarget::Sym(sym) = fixup.target else {
                return Err("an initializer with a fixup that is no symbol".to_owned());
            };
            let value = addrs[sym.index()].wrapping_add(fixup.addend as u64);
            let (at, len) = (at + fixup.offset as usize, usize::from(fixup.size).min(8));
            mem[at..at + len].copy_from_slice(&value.to_le_bytes()[..len]);
        }
        let data = mem.len().next_multiple_of(16);
        mem.resize(data, 0);
        mem.resize(data + STACK_BYTES, 0xAA);

        // The code, with its relocations applied.
        let mut code = chunk.code.clone();
        let mut entry_of = vec![None; count];
        for func in &chunk.funcs {
            let start = func.code.start as usize;
            let slot = entry_of.get_mut(func.sym.index()).ok_or("a function of no symbol")?;
            if slot.replace(start).is_some() {
                return Err(format!("{:?} was compiled twice", func.sym));
            }
            for reloc in &chunk.relocs[func.relocs.start as usize..func.relocs.end as usize] {
                let target = match reloc.sym {
                    SymRef::Module(sym) => match addrs.get(sym.index()) {
                        Some(&addr) if addr != 0 => addr,
                        _ => return Err(format!("a relocation against {sym:?}, which has no address")),
                    },
                    SymRef::Runtime(func) => {
                        let index = RuntimeFn::ALL.iter().position(|&known| known == func);
                        RT_BASE + 16 * index.unwrap_or(0) as u64
                    }
                };
                let at = start + reloc.offset as usize;
                let field = code.get_mut(at..at + 8).ok_or("a relocation outside the code")?;
                field.copy_from_slice(&target.wrapping_add(reloc.addend as u64).to_le_bytes());
            }
        }
        let top = BASE + mem.len() as u64;
        Ok(Machine {
            module,
            chunk,
            code,
            entry_of,
            mem,
            regs: [POISON; REGS],
            fp: top,
            ap: top,
            sp: top,
            pc: 0,
            calls: Vec::new(),
            trace: Vec::new(),
            names: [host::SINK.into(), host::SOURCE.into()],
            source_calls: 0,
            done: false,
        })
    }

    // ---- memory and operands ----

    fn range(&self, addr: u64, len: usize) -> Result<std::ops::Range<usize>, String> {
        let start = addr.checked_sub(BASE).map(|at| at as usize);
        match start {
            Some(start) if start.checked_add(len).is_some_and(|end| end <= self.mem.len()) => {
                Ok(start..start + len)
            }
            _ => Err(format!("an access of {len} bytes at {addr:#x}, outside memory")),
        }
    }

    fn load(&self, addr: u64, bytes: usize) -> Result<u128, String> {
        let mut wide = [0u8; 16];
        wide[..bytes].copy_from_slice(&self.mem[self.range(addr, bytes)?]);
        Ok(u128::from_le_bytes(wide))
    }

    fn store(&mut self, addr: u64, bytes: usize, value: u128) -> Result<(), String> {
        let range = self.range(addr, bytes)?;
        self.mem[range].copy_from_slice(&value.to_le_bytes()[..bytes]);
        Ok(())
    }

    fn reg(&self, reg: u8) -> Result<u64, String> {
        self.regs.get(usize::from(reg)).copied().ok_or_else(|| format!("register {reg}"))
    }

    fn set_reg(&mut self, reg: u8, value: u64) -> Result<(), String> {
        *self.regs.get_mut(usize::from(reg)).ok_or_else(|| format!("register {reg}"))? = value;
        Ok(())
    }

    fn addr(&self, opnd: Opnd) -> Result<u64, String> {
        let Opnd::Mem { base, disp } = opnd else {
            return Err(format!("{opnd:?} where a memory operand is required"));
        };
        let base = match base {
            isa::FP => self.fp,
            isa::AP => self.ap,
            isa::SP => self.sp,
            numbered => self.reg(numbered)?,
        };
        Ok(base.wrapping_add(i64::from(disp) as u64))
    }

    fn read(&self, opnd: Opnd, bits: u8) -> Result<u128, String> {
        let value = match opnd {
            Opnd::Reg(_) if bits > 64 => return Err("a wide value in a register".to_owned()),
            Opnd::Reg(reg) => u128::from(self.reg(reg)?),
            Opnd::Imm(value) => u128::from(value),
            Opnd::Mem { .. } => self.load(self.addr(opnd)?, usize::from(bits.div_ceil(8)))?,
        };
        Ok(value & mask(bits))
    }

    fn write(&mut self, opnd: Opnd, bits: u8, value: u128) -> Result<(), String> {
        let value = value & mask(bits);
        match opnd {
            Opnd::Reg(_) if bits > 64 => Err("a wide value into a register".to_owned()),
            // Ones above the value: those bits are nobody's.
            Opnd::Reg(reg) => {
                let above = if bits >= 64 { 0 } else { u64::MAX << bits };
                self.set_reg(reg, value as u64 | above)
            }
            Opnd::Mem { .. } => self.store(self.addr(opnd)?, usize::from(bits.div_ceil(8)), value),
            Opnd::Imm(_) => Err("a write to an immediate".to_owned()),
        }
    }

    fn float(&self, reg: u8, double: bool) -> Result<f64, String> {
        let bits = self.reg(reg)?;
        Ok(if double { f64::from_bits(bits) } else { f64::from(f32::from_bits(bits as u32)) })
    }

    // ---- calls ----

    fn clobber(&mut self, keep: &[u8]) {
        for (index, reg) in self.regs.iter_mut().enumerate() {
            if !keep.contains(&(index as u8)) {
                *reg = POISON;
            }
        }
    }

    /// Transfers control to `addr`: a compiled function, a host function
    /// or a runtime function. A tail call does not return here.
    fn enter(&mut self, addr: u64, next: usize, tail: bool) -> Result<(), String> {
        // What a callee may rely on: the argument registers.
        self.clobber(&[0, 1, 2, 3, isa::FPR0, isa::FPR0 + 1]);
        if !tail {
            self.calls.push(Frame { ret: next, sp: self.sp, fp: self.fp, ap: self.ap });
        }
        if let Some(index) = addr.checked_sub(RT_BASE).filter(|at| *at < 16 * 64) {
            let func = RuntimeFn::ALL.get((index / 16) as usize).copied();
            self.runtime(func.ok_or("a call of no runtime function")?)?;
            return self.ret();
        }
        let sym = addr.checked_sub(FN_BASE).map(|at| (at / 16) as usize);
        let Some(sym) = sym.filter(|&sym| sym < self.entry_of.len() && addr % 16 == 0) else {
            return Err(format!("a call of {addr:#x}, which is no function"));
        };
        if let Some(start) = self.entry_of[sym] {
            self.pc = start;
            return Ok(());
        }
        let name = self.module.symbol(SymId::new(sym as u32)).name();
        if name == host::SINK.as_bytes() {
            self.trace.push(Event {
                name: Arc::clone(&self.names[0]),
                args: vec![Val::i64(self.regs[0])],
                result: None,
            });
        } else if name == host::SOURCE.as_bytes() {
            let value = host::source_value(self.source_calls);
            self.source_calls += 1;
            self.regs[0] = value;
            self.trace.push(Event {
                name: Arc::clone(&self.names[1]),
                args: Vec::new(),
                result: Some(Val::i64(value)),
            });
        } else if name == b"throw" {
            return self.unwind(self.regs[0]);
        } else {
            return Err(format!("a call of {}, which has no body", String::from_utf8_lossy(name)));
        }
        self.ret()
    }

    /// Returns from the innermost call, or ends the run.
    fn ret(&mut self) -> Result<(), String> {
        self.clobber(&[0, 1, isa::FPR0]);
        let Some(frame) = self.calls.pop() else {
            self.done = true;
            return Ok(());
        };
        if (self.sp, self.fp, self.ap) != (frame.sp, frame.fp, frame.ap) {
            return Err(format!(
                "a return with sp/fp/ap {:#x}/{:#x}/{:#x}, which were {:#x}/{:#x}/{:#x} at the call",
                self.sp, self.fp, self.ap, frame.sp, frame.fp, frame.ap
            ));
        }
        self.pc = frame.ret;
        Ok(())
    }

    /// The runtime functions, by the test target's convention: general
    /// arguments in `g0`–`g2`, the rest on the stack.
    fn runtime(&mut self, func: RuntimeFn) -> Result<(), String> {
        let [a, b, c] = [self.regs[0], self.regs[1], self.regs[2]];
        match func {
            RuntimeFn::Memcpy | RuntimeFn::Memmove => {
                let (to, from) = (self.range(a, c as usize)?, self.range(b, c as usize)?);
                self.mem.copy_within(from, to.start);
            }
            RuntimeFn::Memset => {
                let to = self.range(a, c as usize)?;
                self.mem[to].fill(b as u8);
            }
            RuntimeFn::UDivTi3 | RuntimeFn::UModTi3 | RuntimeFn::DivTi3 | RuntimeFn::ModTi3 => {
                // The second operand is split between `g2` and the
                // stack: that is what the target's `assign` says.
                let x = u128::from(a) | (u128::from(b) << 64);
                let y = u128::from(c) | (self.load(self.sp, 8)? << 64);
                let op = match func {
                    RuntimeFn::UDivTi3 => Opcode::UDiv,
                    RuntimeFn::UModTi3 => Opcode::URem,
                    RuntimeFn::DivTi3 => Opcode::SDiv,
                    _ => Opcode::SRem,
                };
                let value = int_binary(op, 128, x, y)?;
                (self.regs[0], self.regs[1]) = (value as u64, (value >> 64) as u64);
            }
            RuntimeFn::Fmod => {
                let (x, y) = (self.float(isa::FPR0, true)?, self.float(isa::FPR0 + 1, true)?);
                self.regs[usize::from(isa::FPR0)] = (x % y).to_bits();
            }
            RuntimeFn::Fmodf => {
                let x = f32::from_bits(self.regs[usize::from(isa::FPR0)] as u32);
                let y = f32::from_bits(self.regs[usize::from(isa::FPR0) + 1] as u32);
                self.regs[usize::from(isa::FPR0)] = u64::from((x % y).to_bits()) | (u64::MAX << 32);
            }
            RuntimeFn::UnwindResume => {
                // Its own frame is not one to unwind into.
                self.calls.pop();
                let exception = a;
                self.unwind(exception)?;
                // `enter` returns through `ret`: give it this frame.
                self.calls.push(Frame { ret: self.pc, sp: self.sp, fp: self.fp, ap: self.ap });
            }
            other => return Err(format!("runtime function {} is not provided", other.name())),
        }
        Ok(())
    }

    fn func_at(&self, pc: usize) -> Option<&'m FuncRecord> {
        self.chunk.funcs.iter().find(|func| (func.code.start as usize..func.code.end as usize).contains(&pc))
    }

    /// Unwinds to the innermost call site that has a landing pad
    /// (`spec/codegen.md` §9.4): the pad is entered with the stack
    /// pointer as it was at the call, the exception in the first
    /// `eh_data` register and the selector in the second.
    fn unwind(&mut self, exception: u64) -> Result<(), String> {
        let chunk = self.chunk;
        while let Some(frame) = self.calls.pop() {
            let Some(func) = self.func_at(frame.ret.wrapping_sub(1)) else { continue };
            let at = (frame.ret - func.code.start as usize) as u32;
            let sites = &chunk.call_sites[func.call_sites.start as usize..func.call_sites.end as usize];
            let Some(site) = sites
                .iter()
                .find(|site| site.start < at && at <= site.start + site.len && site.pad != NO_PAD)
            else {
                continue;
            };
            let pad = &chunk.pads[(func.pads.start + site.pad) as usize];
            let selector = match chunk.clauses.get((func.clauses.start + pad.clauses.start) as usize) {
                Some(ClauseRow::Catch(id)) if !pad.clauses.is_empty() => u64::from(*id),
                _ => 0,
            };
            self.clobber(&[]);
            (self.regs[0], self.regs[1]) = (exception, selector);
            (self.sp, self.fp, self.ap) = (frame.sp, frame.fp, frame.ap);
            self.pc = func.code.start as usize + site.landing as usize;
            return Ok(());
        }
        Err("an exception that nothing catches".to_owned())
    }

    // ---- the run loop ----

    #[allow(clippy::too_many_lines)]
    fn step(&mut self) -> Result<(), String> {
        let (ins, next) = isa::decode(&self.code, self.pc)?;
        self.pc = next;
        match ins {
            Ins::Enter { frame } => {
                let ap = self.sp;
                self.sp = self.sp.wrapping_sub(u64::from(frame));
                self.store(self.sp, 8, u128::from(self.fp))?;
                self.store(self.sp + 8, 8, u128::from(self.ap))?;
                (self.fp, self.ap) = (self.sp, ap);
            }
            Ins::Leave => {
                let (fp, ap) = (self.load(self.fp, 8)? as u64, self.load(self.fp + 8, 8)? as u64);
                self.sp = self.ap;
                (self.fp, self.ap) = (fp, ap);
            }
            Ins::Ret => self.ret()?,
            Ins::Trap => return Err("a trap instruction was reached".to_owned()),
            Ins::Mov { dst, src } => self.set_reg(dst, self.reg(src)?)?,
            Ins::MovI { dst, imm } => self.set_reg(dst, imm)?,
            Ins::Lea { dst, mem } => self.set_reg(dst, self.addr(mem)?)?,
            Ins::Load { dst, src, bits } => {
                self.addr(src)?;
                let value = self.read(src, bits)?;
                self.write(Opnd::Reg(dst), bits, value)?;
            }
            Ins::Store { src, dst, bits } => {
                self.addr(dst)?;
                let value = self.read(Opnd::Reg(src), bits)?;
                self.write(dst, bits, value)?;
            }
            Ins::Copy { dst, src, bytes } => {
                let len = bytes as usize;
                let (to, from) = (self.range(self.addr(dst)?, len)?, self.range(self.addr(src)?, len)?);
                self.mem.copy_within(from, to.start);
            }
            Ins::Bin { op, bits, dst, a, b } => {
                let op = Opcode::ALL.get(usize::from(op)).copied().ok_or("an operation")?;
                let value = int_binary(op, bits, self.read(a, bits)?, self.read(b, bits)?)?;
                self.write(dst, bits, value)?;
            }
            Ins::ICmp { pred, bits, dst, a, b } => {
                let pred = IntPredicate::ALL.get(usize::from(pred)).copied().ok_or("a predicate")?;
                let value = icmp(pred, bits, self.read(a, bits)?, self.read(b, bits)?);
                self.write(Opnd::Reg(dst), 1, u128::from(value))?;
            }
            Ins::JCmp { pred, bits, a, b, target } => {
                let pred = IntPredicate::ALL.get(usize::from(pred)).copied().ok_or("a predicate")?;
                if icmp(pred, bits, self.read(a, bits)?, self.read(b, bits)?) {
                    self.pc = target as usize;
                }
            }
            Ins::FBin { op, double, dst, a, b } => {
                let op = Opcode::ALL.get(usize::from(op)).copied().ok_or("an operation")?;
                let (x, y) = (self.reg(a)?, self.reg(b)?);
                if double {
                    let (x, y) = (f64::from_bits(x), f64::from_bits(y));
                    let value = float_binary!(op, x, y);
                    self.write(Opnd::Reg(dst), 64, u128::from(value.to_bits()))?;
                } else {
                    let (x, y) = (f32::from_bits(x as u32), f32::from_bits(y as u32));
                    let value = float_binary!(op, x, y);
                    self.write(Opnd::Reg(dst), 32, u128::from(value.to_bits()))?;
                }
            }
            Ins::FNeg { double, dst, src } => {
                let bits = if double { 64 } else { 32 };
                let value = self.read(Opnd::Reg(src), bits)? ^ (1 << (bits - 1));
                self.write(Opnd::Reg(dst), bits, value)?;
            }
            Ins::FCmp { pred, double, dst, a, b } => {
                let pred =
                    FloatPredicate::ALL.get(usize::from(pred)).copied().ok_or("a predicate")?;
                let value = fcmp(pred, self.float(a, double)?, self.float(b, double)?);
                self.write(Opnd::Reg(dst), 1, u128::from(value))?;
            }
            Ins::Cast { op, from, to, dst, src } => {
                let op = Opcode::ALL.get(usize::from(op)).copied().ok_or("an operation")?;
                let value = self.cast(op, from, to, self.read(src, from)?)?;
                self.write(dst, to, value)?;
            }
            Ins::Sel { bits, dst, cond, a, b } => {
                let which = if self.reg(cond)? & 1 != 0 { a } else { b };
                let value = self.read(which, bits)?;
                self.write(dst, bits, value)?;
            }
            Ins::Intr { op, bits, dst, a, b, c } => {
                let (a, b, c) = (self.read(a, bits)?, self.read(b, bits)?, self.read(c, bits)?);
                self.write(dst, bits, int_intrinsic(op, bits, a, b, c)?)?;
            }
            Ins::Checked { op, bits, dst, flag_at, a, b } => {
                let (value, overflow) = checked(op, bits, self.read(a, bits)?, self.read(b, bits)?)?;
                let at = self.addr(dst)?;
                self.store(at, usize::from(bits.div_ceil(8)), value)?;
                self.store(at + u64::from(flag_at), 1, u128::from(overflow))?;
            }
            Ins::Jmp { target } => self.pc = target as usize,
            Ins::Jnz { reg, target } => {
                if self.reg(reg)? & 1 != 0 {
                    self.pc = target as usize;
                }
            }
            Ins::Call { addr } => self.enter(addr, next, false)?,
            Ins::CallR { reg } => self.enter(self.reg(reg)?, next, false)?,
            Ins::TailJ { addr } => self.enter(addr, next, true)?,
            Ins::TailR { reg } => self.enter(self.reg(reg)?, next, true)?,
            Ins::AddSp { delta } => self.sp = self.sp.wrapping_add(i64::from(delta) as u64),
            Ins::LdPc { dst, bits, target } => {
                let len = usize::from(bits / 8);
                let at = target as usize;
                let bytes = self.code.get(at..at + len).ok_or("an island outside the code")?;
                let mut wide = [0u8; 16];
                wide[..len].copy_from_slice(bytes);
                self.write(Opnd::Reg(dst), bits, u128::from_le_bytes(wide))?;
            }
            Ins::LeaPc { dst, target } => self.set_reg(dst, CODE_BASE + u64::from(target))?,
            Ins::JmpR { reg } => {
                let to = self.reg(reg)?.checked_sub(CODE_BASE).filter(|&at| at < self.code.len() as u64);
                self.pc = to.ok_or("an indirect branch to no code")? as usize;
            }
            Ins::DynSlot { ptr, size, align } => {
                self.sp = self.sp.wrapping_sub(u64::from(size)) & !(u64::from(align) - 1);
                self.range(self.sp, size as usize)?;
                self.write(ptr, 64, u128::from(self.sp))?;
            }
            Ins::Chk { reg, bits, signed } => {
                let value = self.reg(reg)?;
                let wide = u128::from(value) & mask(bits);
                let expected = if signed { sext(bits, wide) as u64 } else { wide as u64 };
                if value != expected {
                    return Err(format!(
                        "register {reg} holds {value:#x}, which is not an i{bits} extended as the convention asks"
                    ));
                }
            }
        }
        Ok(())
    }

    // An integer that a float cannot hold exactly is rounded to nearest,
    // which is what `as` does and what the instructions are defined to do.
    #[allow(clippy::cast_precision_loss)]
    fn cast(&self, op: Opcode, from: u8, to: u8, value: u128) -> Result<u128, String> {
        let float = |bits: f64| if to == 32 { u128::from((bits as f32).to_bits()) } else { u128::from(bits.to_bits()) };
        Ok(match op {
            Opcode::Trunc
            | Opcode::ZExt
            | Opcode::PtrToInt
            | Opcode::PtrToAddr
            | Opcode::IntToPtr
            | Opcode::BitCast => value,
            Opcode::SExt => sext(from, value) as u128,
            Opcode::FPTrunc => u128::from((f64::from_bits(value as u64) as f32).to_bits()),
            Opcode::FPExt => u128::from(f64::from(f32::from_bits(value as u32)).to_bits()),
            Opcode::FPToUI | Opcode::FPToSI => {
                let x = if from == 32 {
                    f64::from(f32::from_bits(value as u32))
                } else {
                    f64::from_bits(value as u64)
                };
                if op == Opcode::FPToSI { x as i128 as u128 } else { x as u128 }
            }
            Opcode::UIToFP if to == 32 => u128::from((value as f32).to_bits()),
            Opcode::UIToFP => float(value as f64),
            Opcode::SIToFP if to == 32 => u128::from((sext(from, value) as f32).to_bits()),
            Opcode::SIToFP => float(sext(from, value) as f64),
            other => return Err(format!("{} is no cast of this machine", other.name())),
        })
    }
}

/// Runs `entry` of the compiled `chunk` with `args` in the general
/// argument registers.
///
/// # Errors
/// What went wrong, with where: a trap, an access outside memory, a
/// broken convention, the step limit.
pub fn run(module: &Module, chunk: &Chunk, entry: SymId, args: &[u64]) -> Result<Ran, String> {
    let mut machine = Machine::new(module, chunk)?;
    let Some(start) = machine.entry_of.get(entry.index()).copied().flatten() else {
        return Err(format!("{entry:?} was not compiled"));
    };
    if args.len() > 3 {
        return Err("more arguments than argument registers".to_owned());
    }
    machine.regs[..args.len()].copy_from_slice(args);
    machine.pc = start;
    let top = machine.sp;
    let mut steps = 0u64;
    while !machine.done {
        steps += 1;
        let pc = machine.pc;
        let stepped = if steps > MAX_STEPS { Err("the step limit".to_owned()) } else { machine.step() };
        if let Err(what) = stepped {
            let name = machine
                .func_at(pc)
                .map(|func| String::from_utf8_lossy(module.symbol(func.sym).name()).into_owned());
            let func = machine.func_at(pc).map_or(0, |func| func.code.start as usize);
            return Err(format!(
                "{what} (at {:#x} of {}, step {steps})",
                pc - func,
                name.as_deref().unwrap_or("no function")
            ));
        }
    }
    if machine.sp != top {
        return Err("the entry function returned with the stack pointer moved".to_owned());
    }
    Ok(Ran {
        ret: [machine.regs[0], machine.regs[1]],
        fret: machine.regs[usize::from(isa::FPR0)],
        trace: machine.trace,
        steps,
    })
}
