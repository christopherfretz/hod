//! A test-only assembly emitter that knows nothing about the calling
//! convention except an [`Assignment`]: it moves bytes between memory and
//! the places the assignment names (`spec/abi-x64.md` §8).
//!
//! What it does know is the machine: which registers it may use as
//! temporaries (it refuses an assignment that names one of them), how to
//! reach the caller's stack-argument area from `rbp`, and how to call.
//! It writes AT&T syntax.

use std::fmt::Write as _;

use hod_cg::PReg;
use hod_cg::abi::{Assignment, Part, PartKind, Place, VaInfo};
use hod_x64::{abi, reg};

/// The bytes of one argument or return value: where the producer reads
/// them, where the consumer writes them.
#[derive(Clone, Copy, Debug)]
pub struct Item {
    /// The offset in `hodabi_in` of the image to pass.
    pub src: u64,
    /// The offset in `hodabi_in` of the image to leave in memory when the
    /// value is returned there.
    pub memory: u64,
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
    /// The argument is `sret`: the callee hands it back (§6.1).
    pub sret: bool,
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
    /// The call is to a variadic function: `al` carries the number of
    /// vector registers used (§9.1).
    pub variadic: bool,
}

/// The bits a producer leaves in every byte it does not owe.
const JUNK: u64 = 0xe7e7_e7e7_e7e7_e7e7;

/// The registers the emitter computes with: `rbx` (`hodabi_in`), `rsp`,
/// `rbp`, `r10`, `r11`, `r12` (the output blob), `r13` (junk), `r14` and
/// `r15`.
const TEMPS: [u8; 9] = [3, 4, 5, 10, 11, 12, 13, 14, 15];

/// Where the 32-byte scratch area is, below `rbp` and the five saved
/// registers.
const SCRATCH: i64 = -72;

/// The bytes below `rbp` a function without a register save area keeps:
/// the saved registers and the scratch area, aligned to 16.
const LOCALS: u64 = 80;

/// The helper routines every file needs once: `hodabi_copy` copies `r10`
/// bytes from `r14` to `r15`; `hodabi_fill` fills `r10` bytes (a multiple
/// of 8) at `r15` with `r13`.
pub fn prelude(out: &mut String) {
    out.push_str(
        "\t.text\n\
         \t.p2align 4\n\
         hodabi_copy:\n\
         \tmovb (%r14), %r11b\n\
         \tmovb %r11b, (%r15)\n\
         \tincq %r14\n\
         \tincq %r15\n\
         \tdecq %r10\n\
         \tjnz hodabi_copy\n\
         \tret\n\
         hodabi_fill:\n\
         \tmovq %r13, (%r15)\n\
         \taddq $8, %r15\n\
         \tsubq $8, %r10\n\
         \tjnz hodabi_fill\n\
         \tret\n",
    );
}

/// The line that marks the stack non-executable.
pub fn epilogue(out: &mut String) {
    out.push_str("\t.section .note.GNU-stack,\"\",@progbits\n");
}

const GPR64: [&str; 16] = [
    "rax", "rcx", "rdx", "rbx", "rsp", "rbp", "rsi", "rdi", "r8", "r9", "r10", "r11", "r12", "r13",
    "r14", "r15",
];

enum Reg {
    General(&'static str),
    Vector(u8),
    X87,
}

fn classify(reg: PReg) -> Reg {
    let number = reg::number(reg);
    if reg::is_gpr(reg) {
        assert!(
            !TEMPS.contains(&number),
            "%{} is one of the emitter's temporaries",
            GPR64[number as usize]
        );
        Reg::General(GPR64[number as usize])
    } else if reg::is_fpr(reg) {
        assert!(number < 8, "xmm{number} is beyond the registers the emitter poisons");
        Reg::Vector(number)
    } else {
        assert!(reg::is_x87(reg), "{reg:?} is no register");
        Reg::X87
    }
}

/// The x87 load or store (`fld`, `fstp`) of a part of `size` bytes: an
/// `x86_fp80`, or a `float` or `double` returned on the x87 stack (§5.1).
fn x87_op(op: &str, size: u32) -> String {
    let suffix = match size {
        4 => 's',
        8 => 'l',
        10 => 't',
        _ => panic!("an x87 part of {size} bytes"),
    };
    format!("{op}{suffix}")
}

fn general(reg: PReg) -> &'static str {
    match classify(reg) {
        Reg::General(name) => name,
        _ => panic!("an address in a register that is not a general one"),
    }
}

struct Asm<'a> {
    text: &'a mut String,
    func: Func<'a>,
    /// The bytes below `rbp` the function keeps.
    locals: u64,
}

macro_rules! ins {
    ($asm:expr, $($arg:tt)*) => {{
        $asm.text.push('\t');
        let _ = writeln!($asm.text, $($arg)*);
    }};
}

impl Asm<'_> {
    /// Copies `size` bytes from `src` to `dst`, each a register and an
    /// offset.
    fn copy(&mut self, size: u64, src: (&str, i64), dst: (&str, i64)) {
        if size == 0 {
            return;
        }
        ins!(self, "leaq {}(%{}), %r14", src.1, src.0);
        ins!(self, "leaq {}(%{}), %r15", dst.1, dst.0);
        if size > 32 {
            ins!(self, "movq ${size}, %r10");
            ins!(self, "callq hodabi_copy");
            return;
        }
        let mut at = 0;
        for (chunk, op, reg) in
            [(8, "movq", "r11"), (4, "movl", "r11d"), (2, "movw", "r11w"), (1, "movb", "r11b")]
        {
            while size - at >= chunk {
                ins!(self, "{op} {at}(%r14), %{reg}");
                ins!(self, "{op} %{reg}, {at}(%r15)");
                at += chunk;
            }
        }
    }

    fn prologue(&mut self) {
        let name = self.func.name;
        let _ = writeln!(self.text, "\t.globl {name}\n\t.type {name}, @function\n{name}:");
        ins!(self, "pushq %rbp");
        ins!(self, "movq %rsp, %rbp");
        for reg in ["rbx", "r12", "r13", "r14", "r15"] {
            ins!(self, "pushq %{reg}");
        }
        ins!(self, "subq ${}, %rsp", self.locals - 40);
        ins!(self, "movabsq ${JUNK:#x}, %r13");
        ins!(self, "leaq hodabi_in(%rip), %rbx");
        let out = self.func.out;
        ins!(self, "leaq {out}(%rip), %r12");
    }

    fn finish(&mut self) {
        let name = self.func.name;
        ins!(self, "leaq -40(%rbp), %rsp");
        for reg in ["r15", "r14", "r13", "r12", "rbx"] {
            ins!(self, "popq %{reg}");
        }
        ins!(self, "popq %rbp");
        ins!(self, "retq");
        ins!(self, ".size {name}, .-{name}");
    }

    /// Fills the registers a producer has not written with junk, so that a
    /// consumer reading the wrong one cannot find the right value there
    /// by chance.
    fn poison(&mut self) {
        for reg in ["rax", "rcx", "rdx", "rsi", "rdi", "r8", "r9"] {
            ins!(self, "movq %r13, %{reg}");
        }
        ins!(self, "movq %r13, {SCRATCH}(%rbp)");
        ins!(self, "movq %r13, {}(%rbp)", SCRATCH + 8);
        for number in 0..8 {
            ins!(self, "movdqu {SCRATCH}(%rbp), %xmm{number}");
        }
    }

    /// Fills the first 16 bytes of the scratch area with junk.
    fn junk_scratch(&mut self) {
        ins!(self, "movq %r13, {SCRATCH}(%rbp)");
        ins!(self, "movq %r13, {}(%rbp)", SCRATCH + 8);
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
        let shift = 64 - ext.bits;
        let op = if ext.signed { "sarq" } else { "shrq" };
        ins!(self, "movq %{reg}, %r11");
        if shift != 0 {
            ins!(self, "shlq ${shift}, %r11");
            ins!(self, "{op} ${shift}, %r11");
        }
        if ext.to == 64 {
            ins!(self, "movq %r11, %{reg}");
        } else {
            let mask = (1u64 << ext.to) - 1;
            ins!(self, "movabsq ${mask:#x}, %r10");
            ins!(self, "andq %r10, %r11");
            ins!(self, "notq %r10");
            ins!(self, "andq %r10, %{reg}");
            ins!(self, "orq %r11, %{reg}");
        }
    }

    /// Moves the bytes of `part` from `item`'s image to the part's place;
    /// a stack place is `stack` plus the part's offset. An x87 part is
    /// left for [`Asm::produce_x87`].
    fn produce(&mut self, item: &Item, part: &Part, stack: &str) {
        if let PartKind::Lanes { lane, len, .. } = part.kind {
            // §4.3: element `i` in the low bit of lane `i`; the other bits
            // of the lane are junk.
            self.junk_scratch();
            for index in 0..i64::from(len) {
                let at = (item.src + u64::from(part.offset)) as i64 + index / 8;
                ins!(self, "movzbl {at}(%rbx), %r11d");
                ins!(self, "shrl ${}, %r11d", index % 8);
                ins!(self, "xorb %r13b, %r11b");
                ins!(self, "andb $1, %r11b");
                ins!(self, "xorb %r13b, %r11b");
                ins!(self, "movb %r11b, {}(%rbp)", SCRATCH + index * i64::from(lane));
            }
            match part.place {
                Place::Reg(reg) => match classify(reg) {
                    Reg::Vector(number) => ins!(self, "movdqu {SCRATCH}(%rbp), %xmm{number}"),
                    _ => panic!("lanes outside a vector register"),
                },
                Place::Stack(offset) => self.copy(16, ("rbp", SCRATCH), (stack, i64::from(offset))),
            }
            return;
        }
        let src = ("rbx", (item.src + u64::from(part.offset)) as i64);
        let size = u64::from(part.size);
        let extension = Self::extension(item, part);
        match part.place {
            Place::Reg(reg) => match classify(reg) {
                Reg::General(name) => {
                    assert!(size <= 8, "a general register holds 8 bytes");
                    self.junk_scratch();
                    self.copy(size, src, ("rbp", SCRATCH));
                    ins!(self, "movq {SCRATCH}(%rbp), %{name}");
                    if let Some((ext, _)) = extension {
                        self.extend(name, ext);
                    }
                }
                Reg::Vector(number) => {
                    assert!(size <= 16, "a vector register holds 16 bytes");
                    self.junk_scratch();
                    self.copy(size, src, ("rbp", SCRATCH));
                    ins!(self, "movdqu {SCRATCH}(%rbp), %xmm{number}");
                }
                Reg::X87 => {}
            },
            Place::Stack(offset) => {
                self.copy(size, src, (stack, i64::from(offset)));
                if let Some((ext, unit)) = extension {
                    let at = i64::from(offset + unit);
                    ins!(self, "movq {at}(%{stack}), %rax");
                    self.extend("rax", ext);
                    ins!(self, "movq %rax, {at}(%{stack})");
                }
            }
        }
    }

    /// Loads the x87 parts of `item`, `st1` first, so that each ends at
    /// the place the assignment names.
    fn produce_x87(&mut self, item: &Item, parts: &[Part]) {
        let mut x87: Vec<(u8, &Part)> = parts
            .iter()
            .filter_map(|part| match part.place {
                Place::Reg(reg) if reg::is_x87(reg) => Some((reg::number(reg), part)),
                _ => None,
            })
            .collect();
        x87.sort_by_key(|&(number, _)| std::cmp::Reverse(number));
        for (index, (number, _)) in x87.iter().rev().enumerate() {
            assert_eq!(usize::from(*number), index, "x87 parts from st0 up");
        }
        for (_, part) in x87 {
            self.junk_scratch();
            let src = ("rbx", (item.src + u64::from(part.offset)) as i64);
            self.copy(u64::from(part.size), src, ("rbp", SCRATCH));
            ins!(self, "{} {SCRATCH}(%rbp)", x87_op("fld", part.size));
        }
    }

    /// Moves the bytes of `part` from its place to `item`'s output; a
    /// stack place is `stack` plus the part's offset. An x87 part pops the
    /// x87 stack: those go in order from `st0`.
    fn consume(&mut self, item: &Item, part: &Part, stack: (&str, i64)) {
        let dst = ("r12", (item.dst + u64::from(part.offset)) as i64);
        if let PartKind::Lanes { lane, len, .. } = part.kind {
            match part.place {
                Place::Reg(reg) => match classify(reg) {
                    Reg::Vector(number) => ins!(self, "movdqu %xmm{number}, {SCRATCH}(%rbp)"),
                    _ => panic!("lanes outside a vector register"),
                },
                Place::Stack(offset) => {
                    self.copy(16, (stack.0, stack.1 + i64::from(offset)), ("rbp", SCRATCH));
                }
            }
            // The low bit of each lane, packed.
            ins!(self, "xorl %r10d, %r10d");
            for index in 0..i64::from(len) {
                ins!(self, "movzbl {}(%rbp), %r11d", SCRATCH + index * i64::from(lane));
                ins!(self, "andl $1, %r11d");
                ins!(self, "shll ${index}, %r11d");
                ins!(self, "orl %r11d, %r10d");
            }
            if len > 8 {
                ins!(self, "movw %r10w, {}(%r12)", dst.1);
            } else {
                ins!(self, "movb %r10b, {}(%r12)", dst.1);
            }
            return;
        }
        let mut size = u64::from(part.size);
        if item.int {
            // The whole unit, to see what the producer extended.
            size = size.next_multiple_of(8);
        }
        match part.place {
            Place::Reg(reg) => {
                match classify(reg) {
                    Reg::General(name) => {
                        assert!(size <= 8, "a general register holds 8 bytes");
                        ins!(self, "movq %{name}, {SCRATCH}(%rbp)");
                    }
                    Reg::Vector(number) => {
                        assert!(size <= 16, "a vector register holds 16 bytes");
                        ins!(self, "movdqu %xmm{number}, {SCRATCH}(%rbp)");
                    }
                    Reg::X87 => {
                        assert!(size <= 10, "an x87 register holds 10 bytes");
                        ins!(self, "{} {SCRATCH}(%rbp)", x87_op("fstp", part.size));
                    }
                }
                self.copy(size, ("rbp", SCRATCH), dst);
            }
            Place::Stack(offset) => self.copy(size, (stack.0, stack.1 + i64::from(offset)), dst),
        }
    }

    /// Traps unless `rax` holds the address in `r11`.
    fn check_rax(&mut self) {
        ins!(self, "cmpq %r11, %rax");
        ins!(self, "je 1f");
        ins!(self, "ud2");
        self.text.push_str("1:\n");
    }
}

/// A function that passes each argument's image as `asg` says, calls
/// `peer`, and records the return value.
pub fn caller(text: &mut String, func: Func<'_>) {
    let mut asm = Asm { text, func, locals: LOCALS };
    asm.prologue();
    let area = u64::from(func.asg.stack_size).next_multiple_of(16);
    assert!(area < 1 << 30, "a stack-argument area the emitter cannot make");
    if area != 0 {
        ins!(asm, "subq ${area}, %rsp");
        ins!(asm, "movq %rsp, %r15");
        ins!(asm, "movq ${area}, %r10");
        ins!(asm, "callq hodabi_fill");
    }
    asm.poison();
    for part in &func.asg.args {
        asm.produce(&func.args[part.arg as usize], part, "rsp");
    }
    if let (Some(memory), Some(ret)) = (func.asg.ret_memory, func.ret) {
        match memory.address {
            Place::Reg(reg) => {
                ins!(asm, "leaq {}(%r12), %{}", ret.dst, general(reg));
            }
            Place::Stack(offset) => {
                ins!(asm, "leaq {}(%r12), %r11", ret.dst);
                ins!(asm, "movq %r11, {offset}(%rsp)");
            }
        }
    }
    if func.variadic {
        ins!(asm, "movl ${}, %eax", func.asg.vector_regs_used);
    }
    ins!(asm, "callq {}", func.peer);
    if let Some(ret) = func.ret {
        if let Some(memory) = func.asg.ret_memory
            && let Some(reg) = memory.returned_in
        {
            assert_eq!(reg, abi::RETURNS_ADDRESS, "the address comes back in rax");
            ins!(asm, "leaq {}(%r12), %r11", ret.dst);
            asm.check_rax();
        }
        // The x87 parts in the order they pop.
        let mut parts = func.asg.ret.clone();
        parts.sort_by_key(|part| match part.place {
            Place::Reg(reg) if reg::is_x87(reg) => i32::from(reg::number(reg)),
            _ => -1,
        });
        for part in &parts {
            asm.consume(&ret, part, ("rsp", 0));
        }
    }
    if func.asg.sret_returned_in.is_some()
        && let Some(sret) = func.args.iter().find(|item| item.sret)
    {
        ins!(asm, "movq {}(%rbx), %r11", sret.src);
        asm.check_rax();
    }
    asm.finish();
}

/// A function that records what arrives at each place `asg` names and
/// returns its return value's image.
pub fn callee(text: &mut String, func: Func<'_>) {
    let mut asm = Asm { text, func, locals: LOCALS };
    asm.prologue();
    asm.record_arguments(usize::MAX);
    asm.produce_return();
    asm.finish();
}

/// The bytes of the register save area and of a `va_list` (§9).
const REG_SAVE: u64 = 176;
const VA_LIST: u64 = 24;

/// A variadic function that records its `named` named arguments, then
/// does what §9.3 says `va_start` does — from `va` and nothing else — and
/// hands the `va_list` to `peer`, a `void peer(va_list)`.
pub fn variadic_callee(text: &mut String, func: Func<'_>, named: usize, va: VaInfo) {
    let save = -(LOCALS as i64) - REG_SAVE as i64;
    let list = save - VA_LIST as i64;
    let locals = (LOCALS + REG_SAVE + VA_LIST).next_multiple_of(16);
    let mut asm = Asm { text, func, locals };
    asm.prologue();
    // §9.3: the register save area. Saving every argument register is
    // allowed; the offsets below skip the named ones.
    for (index, reg) in ["rdi", "rsi", "rdx", "rcx", "r8", "r9"].iter().enumerate() {
        ins!(asm, "movq %{reg}, {}(%rbp)", save + 8 * index as i64);
    }
    for number in 0..8 {
        ins!(asm, "movdqu %xmm{number}, {}(%rbp)", save + 48 + 16 * number);
    }
    asm.record_arguments(named);
    // §9.2: `gp_offset`, `fp_offset`, `overflow_arg_area`,
    // `reg_save_area`.
    ins!(asm, "movl ${}, {list}(%rbp)", 8 * u32::from(va.gpr_used));
    ins!(asm, "movl ${}, {}(%rbp)", 48 + 16 * u32::from(va.fpr_used), list + 4);
    ins!(asm, "leaq {}(%rbp), %r11", 16 + u64::from(va.stack_offset));
    ins!(asm, "movq %r11, {}(%rbp)", list + 8);
    ins!(asm, "leaq {save}(%rbp), %r11");
    ins!(asm, "movq %r11, {}(%rbp)", list + 16);
    // A `va_list` is an array: passed as its address.
    ins!(asm, "leaq {list}(%rbp), %rdi");
    ins!(asm, "callq {}", func.peer);
    asm.finish();
}

impl Asm<'_> {
    /// Records the parts of the arguments below `limit`.
    fn record_arguments(&mut self, limit: usize) {
        for part in self.func.asg.args.iter().filter(|part| (part.arg as usize) < limit) {
            let item = self.func.args[part.arg as usize];
            self.consume(&item, part, ("rbp", 16));
        }
    }

    fn produce_return(&mut self) {
        if let Some(ret) = self.func.ret {
            if let Some(memory) = self.func.asg.ret_memory {
                match memory.address {
                    Place::Reg(reg) => ins!(self, "movq %{}, %r10", general(reg)),
                    Place::Stack(offset) => ins!(self, "movq {}(%rbp), %r10", 16 + offset),
                }
                ins!(self, "movq %r10, {}(%rbp)", SCRATCH + 16);
                // §5.2: what a `store` of the value leaves there.
                self.copy(ret.size, ("rbx", ret.memory as i64), ("r10", 0));
                self.poison();
                if let Some(reg) = memory.returned_in {
                    ins!(self, "movq {}(%rbp), %{}", SCRATCH + 16, general(reg));
                }
            } else {
                self.poison();
            }
            for part in &self.func.asg.ret {
                self.produce(&ret, part, "rsp");
            }
            self.produce_x87(&ret, &self.func.asg.ret.clone());
        }
        if self.func.asg.sret_returned_in.is_some()
            && let Some(sret) = self.func.args.iter().find(|item| item.sret)
        {
            ins!(self, "movq {}(%rbx), %rax", sret.src);
        }
    }
}
