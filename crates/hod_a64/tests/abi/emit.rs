//! A test-only assembly emitter that knows nothing about the calling
//! convention except an [`Assignment`]: it moves bytes between memory and
//! the places the assignment names (`spec/abi-a64.md` §8).
//!
//! What it does know is the machine: which registers it may use as
//! temporaries (it refuses an assignment that names one of them), how to
//! reach the caller's stack-argument area from `sp`, and how to call.

use std::fmt::Write as _;

use hod_a64::reg;
use hod_cg::PReg;
use hod_cg::abi::{Assignment, Part, Place, VaInfo};

/// The bytes of one argument or return value: where the producer reads
/// them, where the consumer writes them.
#[derive(Clone, Copy, Debug)]
pub struct Item {
    /// The offset in `hodabi_in` of the image to pass.
    pub src: u64,
    /// The offset in the output blob of the image received.
    pub dst: u64,
    /// The alloc size of the value: what a return in memory copies.
    pub size: u64,
    /// The bytes of the value proper.
    pub bytes: u32,
    /// The value is one integer: the consumer records its whole 8-byte
    /// units, to show what the producer put above it.
    pub int: bool,
    /// What the producer owes above the value (§7).
    pub ext: Option<Ext>,
}

/// The low `bits` bits of a value are extended, by sign if `signed`, to
/// `to` bits. What lies above stays as it is.
#[derive(Clone, Copy, Debug)]
pub struct Ext {
    pub bits: u32,
    pub signed: bool,
    pub to: u32,
}

/// One function to emit.
#[derive(Clone, Copy, Debug)]
pub struct Func<'a> {
    pub name: &'a str,
    /// The function a caller calls.
    pub peer: &'a str,
    /// The output blob's symbol.
    pub out: &'a str,
    pub asg: &'a Assignment,
    pub args: &'a [Item],
    pub ret: Option<Item>,
}

/// The bits a producer leaves in every byte it does not owe.
const JUNK: u64 = 0xe7e7_e7e7_e7e7_e7e7;

/// The size of the frame record, the 16-byte scratch area at `x29 + 16`
/// and padding.
const FRAME: u64 = 48;

/// The registers the emitter computes with.
const TEMPS: [u8; 10] = [9, 10, 11, 12, 13, 14, 16, 17, 29, 30];

/// The helper routines every file needs once.
pub fn prelude(out: &mut String) {
    out.push_str(
        "\t.text\n\
         \t.p2align 2\n\
         hodabi_copy:\n\
         \tldrb w16, [x12], #1\n\
         \tstrb w16, [x11], #1\n\
         \tsubs x13, x13, #1\n\
         \tb.ne hodabi_copy\n\
         \tret\n\
         hodabi_fill:\n\
         \tstp x14, x14, [x11], #16\n\
         \tsubs x13, x13, #16\n\
         \tb.ne hodabi_fill\n\
         \tret\n",
    );
}

/// The line that marks the stack non-executable.
pub fn epilogue(out: &mut String) {
    out.push_str("\t.section .note.GNU-stack,\"\",%progbits\n");
}

enum Reg {
    General(u8),
    Vector(u8),
}

fn classify(reg: PReg) -> Reg {
    let number = reg::number(reg);
    if reg::is_gpr(reg) {
        assert!(!TEMPS.contains(&number), "x{number} is one of the emitter's temporaries");
        Reg::General(number)
    } else {
        assert!(reg::is_fpr(reg), "sp holds no argument");
        assert!(number < 16, "v{number} is beyond the registers the emitter poisons");
        Reg::Vector(number)
    }
}

struct Asm<'a> {
    text: &'a mut String,
    func: Func<'a>,
    /// The bytes between the frame record's address and the caller's
    /// stack-argument area.
    frame: u64,
}

macro_rules! ins {
    ($asm:expr, $($arg:tt)*) => {{
        $asm.text.push('\t');
        let _ = writeln!($asm.text, $($arg)*);
    }};
}

impl Asm<'_> {
    /// `dst = base + offset`.
    fn add(&mut self, dst: &str, base: &str, offset: u64) {
        assert!(offset < 1 << 24, "an offset the emitter cannot add");
        let (low, high) = (offset & 0xfff, offset >> 12);
        ins!(self, "add {dst}, {base}, #{low}");
        if high != 0 {
            ins!(self, "add {dst}, {dst}, #{high}, lsl #12");
        }
    }

    fn mov(&mut self, dst: &str, value: u64) {
        assert!(value < 1 << 32, "a size the emitter cannot load");
        ins!(self, "movz {dst}, #{}", value & 0xffff);
        if value >> 16 != 0 {
            ins!(self, "movk {dst}, #{}, lsl #16", value >> 16);
        }
    }

    /// Copies `size` bytes from `src` to `dst`, each a register and an
    /// offset.
    fn copy(&mut self, size: u64, src: (&str, u64), dst: (&str, u64)) {
        if size == 0 {
            return;
        }
        self.add("x12", src.0, src.1);
        self.add("x11", dst.0, dst.1);
        if size > 32 {
            self.mov("x13", size);
            ins!(self, "bl hodabi_copy");
            return;
        }
        let mut at = 0;
        for (chunk, reg) in [(8, "x16"), (4, "w16"), (2, "w16"), (1, "w16")] {
            while size - at >= chunk {
                let (load, store) = match chunk {
                    2 => ("ldurh", "sturh"),
                    1 => ("ldurb", "sturb"),
                    _ => ("ldur", "stur"),
                };
                ins!(self, "{load} {reg}, [x12, #{at}]");
                ins!(self, "{store} {reg}, [x11, #{at}]");
                at += chunk;
            }
        }
    }

    fn prologue(&mut self) {
        let name = self.func.name;
        let _ = writeln!(self.text, "\t.globl {name}\n\t.type {name}, %function\n{name}:");
        ins!(self, "stp x29, x30, [sp, #-{}]!", self.frame);
        ins!(self, "mov x29, sp");
        ins!(self, "mov x14, #{JUNK:#x}");
        ins!(self, "adrp x9, hodabi_in");
        ins!(self, "add x9, x9, :lo12:hodabi_in");
        self.out_base();
    }

    fn out_base(&mut self) {
        let out = self.func.out;
        ins!(self, "adrp x10, {out}");
        ins!(self, "add x10, x10, :lo12:{out}");
    }

    fn finish(&mut self) {
        let name = self.func.name;
        ins!(self, "mov sp, x29");
        ins!(self, "ldp x29, x30, [sp], #{}", self.frame);
        ins!(self, "ret");
        ins!(self, ".size {name}, .-{name}");
    }

    /// Fills the registers a producer has not written with junk, so that a
    /// consumer reading the wrong one cannot find the right value there
    /// by chance.
    fn poison(&mut self) {
        for number in 0..8 {
            ins!(self, "mov x{number}, x14");
            ins!(self, "dup v{number}.2d, x14");
        }
    }

    /// The extension the producer owes in `part`'s last 8-byte unit, in
    /// bits of that unit, and the offset of the unit in the part.
    fn extension(item: &Item, part: &Part) -> Option<(Ext, u32)> {
        let ext = item.ext?;
        if part.offset + part.size != item.bytes {
            return None;
        }
        let unit = (part.size - 1) / 8 * 8;
        let below = 8 * (part.offset + unit);
        let within = Ext { bits: ext.bits - below, signed: ext.signed, to: ext.to - below };
        assert!(within.bits < within.to && within.to <= 64, "an extension within one unit");
        Some((within, unit))
    }

    /// Extends the value in the 64-bit register `reg` as `ext` says,
    /// leaving the bits above `ext.to` alone.
    fn extend(&mut self, reg: &str, ext: Ext) {
        let op = if ext.signed { "sbfx" } else { "ubfx" };
        ins!(self, "{op} x17, {reg}, #0, #{}", ext.bits);
        if ext.to == 64 {
            ins!(self, "mov {reg}, x17");
        } else {
            ins!(self, "bfxil {reg}, x17, #0, #{}", ext.to);
        }
    }

    /// Moves the bytes of `part` from `item`'s image to the part's place;
    /// a stack place is `stack` plus the part's offset.
    fn produce(&mut self, item: &Item, part: &Part, stack: &str) {
        let src = ("x9", item.src + u64::from(part.offset));
        let size = u64::from(part.size);
        let extension = Self::extension(item, part);
        match part.place {
            Place::Reg(reg) => match classify(reg) {
                Reg::General(number) => {
                    assert!(size <= 8, "a general register holds 8 bytes");
                    ins!(self, "str x14, [x29, #16]");
                    self.copy(size, src, ("x29", 16));
                    ins!(self, "ldr x{number}, [x29, #16]");
                    if let Some((ext, _)) = extension {
                        self.extend(&format!("x{number}"), ext);
                    }
                }
                Reg::Vector(number) => {
                    assert!(size <= 16, "a vector register holds 16 bytes");
                    ins!(self, "stp x14, x14, [x29, #16]");
                    self.copy(size, src, ("x29", 16));
                    ins!(self, "ldr q{number}, [x29, #16]");
                }
            },
            Place::Stack(offset) => {
                self.copy(size, src, (stack, u64::from(offset)));
                if let Some((ext, unit)) = extension {
                    self.add("x11", stack, u64::from(offset + unit));
                    ins!(self, "ldr x16, [x11]");
                    self.extend("x16", ext);
                    ins!(self, "str x16, [x11]");
                }
            }
        }
    }

    /// Moves the bytes of `part` from its place to `item`'s output; a
    /// stack place is `stack` plus the part's offset.
    fn consume(&mut self, item: &Item, part: &Part, stack: (&str, u64)) {
        let dst = ("x10", item.dst + u64::from(part.offset));
        let mut size = u64::from(part.size);
        if item.int {
            // The whole unit, to see what the producer extended.
            size = size.next_multiple_of(8);
        }
        match part.place {
            Place::Reg(reg) => {
                match classify(reg) {
                    Reg::General(number) => {
                        assert!(size <= 8, "a general register holds 8 bytes");
                        ins!(self, "str x{number}, [x29, #16]");
                    }
                    Reg::Vector(number) => {
                        assert!(size <= 16, "a vector register holds 16 bytes");
                        ins!(self, "str q{number}, [x29, #16]");
                    }
                }
                self.copy(size, ("x29", 16), dst);
            }
            Place::Stack(offset) => self.copy(size, (stack.0, stack.1 + u64::from(offset)), dst),
        }
    }
}

/// A function that passes each argument's image as `asg` says, calls
/// `peer`, and records the return value.
pub fn caller(text: &mut String, func: Func<'_>) {
    let mut asm = Asm { text, func, frame: FRAME };
    asm.prologue();
    let area = u64::from(func.asg.stack_size).next_multiple_of(16);
    assert!(area < 1 << 24, "a stack-argument area the emitter cannot make");
    if area != 0 {
        if area & 0xfff != 0 {
            ins!(asm, "sub sp, sp, #{}", area & 0xfff);
        }
        if area >> 12 != 0 {
            ins!(asm, "sub sp, sp, #{}, lsl #12", area >> 12);
        }
        ins!(asm, "mov x11, sp");
        asm.mov("x13", area);
        ins!(asm, "bl hodabi_fill");
    }
    asm.poison();
    for part in &func.asg.args {
        asm.produce(&func.args[part.arg as usize], part, "sp");
    }
    if let (Some(memory), Some(ret)) = (func.asg.ret_memory, func.ret) {
        match memory.address {
            Place::Reg(reg) => {
                let Reg::General(number) = classify(reg) else {
                    panic!("an address in a vector register")
                };
                asm.add(&format!("x{number}"), "x10", ret.dst);
            }
            Place::Stack(offset) => {
                asm.add("x16", "x10", ret.dst);
                asm.add("x11", "sp", u64::from(offset));
                ins!(asm, "str x16, [x11]");
            }
        }
    }
    ins!(asm, "bl {}", func.peer);
    asm.out_base();
    if let Some(ret) = func.ret {
        for part in &func.asg.ret {
            asm.consume(&ret, part, ("sp", 0));
        }
    }
    asm.finish();
}

/// A function that records what arrives at each place `asg` names and
/// returns its return value's image.
pub fn callee(text: &mut String, func: Func<'_>) {
    let mut asm = Asm { text, func, frame: FRAME };
    asm.prologue();
    asm.record_arguments(usize::MAX);
    asm.produce_return();
    asm.finish();
}

/// The bytes of the general and of the vector register save area, and of
/// a `va_list` (`spec/abi-a64.md` §9).
const GR_SAVE: u64 = 64;
const VR_SAVE: u64 = 128;
const VA_LIST: u64 = 32;

/// A variadic function that records its `named` named arguments, then
/// does what §9.3 says `va_start` does — from `va` and nothing else — and
/// hands the `va_list` to `peer`, a `void peer(va_list)`.
pub fn variadic_callee(text: &mut String, func: Func<'_>, named: usize, va: VaInfo) {
    let (list, general, vector) = (FRAME, FRAME + VA_LIST, FRAME + VA_LIST + GR_SAVE);
    let mut asm = Asm { text, func, frame: vector + VR_SAVE };
    asm.prologue();
    // §9.3: the register save areas. Saving every argument register is
    // allowed; the offsets below skip the named ones.
    for number in 0..8 {
        ins!(asm, "str x{number}, [x29, #{}]", general + 8 * number);
        ins!(asm, "str q{number}, [x29, #{}]", vector + 16 * number);
    }
    asm.record_arguments(named);
    // §9.2: `__stack`, `__gr_top`, `__vr_top`, `__gr_offs`, `__vr_offs`.
    let frame = asm.frame;
    asm.add("x16", "x29", frame + u64::from(va.stack_offset));
    ins!(asm, "str x16, [x29, #{list}]");
    asm.add("x16", "x29", general + GR_SAVE);
    ins!(asm, "str x16, [x29, #{}]", list + 8);
    asm.add("x16", "x29", vector + VR_SAVE);
    ins!(asm, "str x16, [x29, #{}]", list + 16);
    ins!(asm, "mov w16, #{}", -8 * (8 - i32::from(va.gpr_used)));
    ins!(asm, "str w16, [x29, #{}]", list + 24);
    ins!(asm, "mov w16, #{}", -16 * (8 - i32::from(va.fpr_used)));
    ins!(asm, "str w16, [x29, #{}]", list + 28);
    // A `va_list` is passed by reference (AAPCS64 B.4: larger than 16
    // bytes).
    asm.add("x0", "x29", list);
    ins!(asm, "bl {}", func.peer);
    asm.finish();
}

impl Asm<'_> {
    /// Records the parts of the arguments below `limit`.
    fn record_arguments(&mut self, limit: usize) {
        let frame = self.frame;
        for part in self.func.asg.args.iter().filter(|part| (part.arg as usize) < limit) {
            let item = self.func.args[part.arg as usize];
            self.consume(&item, part, ("x29", frame));
        }
    }

    fn produce_return(&mut self) {
        let Some(ret) = self.func.ret else { return };
        if let Some(memory) = self.func.asg.ret_memory {
            let address = match memory.address {
                Place::Reg(reg) => {
                    let Reg::General(number) = classify(reg) else {
                        panic!("an address in a vector register")
                    };
                    format!("x{number}")
                }
                Place::Stack(offset) => {
                    let frame = self.frame;
                    self.add("x11", "x29", frame + u64::from(offset));
                    ins!(self, "ldr x17, [x11]");
                    "x17".to_string()
                }
            };
            self.copy(ret.size, ("x9", ret.src), (&address, 0));
            self.poison();
            if let Some(reg) = memory.returned_in {
                let Reg::General(number) = classify(reg) else {
                    panic!("an address in a vector register")
                };
                ins!(self, "mov x{number}, {address}");
            }
        } else {
            self.poison();
        }
        for part in &self.func.asg.ret {
            self.produce(&ret, part, "sp");
        }
    }
}
