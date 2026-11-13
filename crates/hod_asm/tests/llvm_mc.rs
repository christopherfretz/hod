//! `spec/asm.md` §7: every instruction form the assembler accepts
//! encodes to the words LLVM 22's and LLVM 19's `llvm-mc` produce for
//! the same text, and the forms it refuses are refused by name.

use std::fmt::Write as _;

use hod_asm::Refusal;
use hod_asm::a64::{Code, Features, assemble};
use hod_testutil::{Scratch, run, tool};

const ALL: Features = Features { lse: true, crc: true, fp: true };

/// Bodies the assembler accepts. Each is assembled alone by hod; LLVM
/// assembles them all in one file. A body's numeric labels are its own.
const ACCEPTED: &[&str] = &[
    // Moves.
    "mov x0, x1",
    "mov w3, w30",
    "mov x0, sp",
    "mov sp, x9",
    "mov wsp, w2",
    "mov w0, wzr",
    "mov x0, #0",
    "mov w0, #1",
    "mov w0, #3",
    "mov x1, #7",
    "mov x0, #-1",
    "mov w0, #-1",
    "mov x0, #0x10000",
    "mov x0, #0xffff0000ffff0000",
    "mov w0, #0xffff0000",
    "mov w5, #0x80000000",
    "mov x29, sp",
    "mov fp, sp",
    "mov x0, lr",
    "movz x0, #1, lsl #16",
    "movz w1, #0xffff",
    "movn x2, #3, lsl #48",
    "movk x3, #0x1234, lsl #32",
    "mvn x0, x1",
    "mvn w0, w1, lsl #3",
    // Arithmetic.
    "add x10, x9, #1",
    "add w0, w0, #1",
    "add x0, x1, #4095",
    "add x0, x1, #1, lsl #12",
    "add x0, x1, #0x1000",
    "add x0, x1, #-8",
    "sub x0, x1, #-8",
    "add sp, sp, #16",
    "sub sp, sp, #32",
    "add x0, sp, #8",
    "adds x0, x1, #3",
    "subs w0, w1, #7",
    "add x0, x1, x2",
    "add x0, x1, x2, lsl #3",
    "sub w0, w1, w2, asr #5",
    "adds x0, x1, x2, lsr #63",
    "subs x0, x1, x2",
    "add x0, sp, x1",
    "add sp, sp, x1",
    "add x0, x1, w2, uxtw",
    "add x0, x1, w2, sxtw #2",
    "sub x0, sp, x1, lsl #2",
    "add w0, w1, w2, uxtb",
    "cmp x0, #1",
    "cmp w0, #-1",
    "cmn x0, #4",
    "cmp x0, x1",
    "cmp w1, w2",
    "cmp sp, x1",
    "cmp x0, w1, sxtw",
    "cmn w0, w1, lsl #4",
    "neg x0, x1",
    "negs w0, w1, lsl #2",
    "adc x0, x1, x2",
    "adcs w0, w1, w2",
    "sbc x0, x1, x2",
    "sbcs w0, w1, w2",
    "mul x0, x1, x2",
    "mneg w0, w1, w2",
    "madd x0, x1, x2, x3",
    "msub w0, w1, w2, w3",
    "smull x0, w1, w2",
    "umull x0, w1, w2",
    "smaddl x0, w1, w2, x3",
    "umsubl x0, w1, w2, x3",
    "smulh x0, x1, x2",
    "umulh x0, x1, x2",
    "sdiv x0, x1, x2",
    "udiv w0, w1, w2",
    "crc32b w0, w1, w2",
    "crc32cx w0, w1, x2",
    // Logical and bit fields.
    "and x0, x1, #0xff",
    "orr w0, w1, #1",
    "eor x0, x1, #0x5555555555555555",
    "ands w0, w1, #0xf0",
    "and w0, w1, #-2",
    "and sp, x1, #0xfffffffffffffff0",
    "and x0, x1, x2",
    "orr x0, x1, x2, lsl #4",
    "eor w0, w1, w2, ror #3",
    "ands x0, x1, x2",
    "bic x0, x1, x2",
    "bics w0, w1, w2, lsr #1",
    "orn x0, x1, x2",
    "eon w0, w1, w2",
    "tst x0, #1",
    "tst w0, w1",
    "tst x0, x1, lsl #2",
    "lsl x0, x1, #3",
    "lsr w0, w1, #31",
    "asr x0, x1, #63",
    "ror w0, w1, #7",
    "lsl x0, x1, x2",
    "asr w0, w1, w2",
    "lslv x0, x1, x2",
    "rorv w0, w1, w2",
    "ubfx x0, x1, #4, #8",
    "sbfx w0, w1, #0, #16",
    "bfxil x0, x1, #3, #5",
    "ubfiz w0, w1, #2, #6",
    "sbfiz x0, x1, #1, #31",
    "bfi x0, x1, #8, #8",
    "ubfm x0, x1, #3, #10",
    "sbfm w0, w1, #31, #31",
    "bfm x0, x1, #60, #3",
    "sxtb x0, w1",
    "sxth w0, w1",
    "sxtw x0, w1",
    "uxtb w0, w1",
    "uxth w0, w1",
    "extr x0, x1, x2, #13",
    "rbit x0, x1",
    "rev w0, w1",
    "rev16 x0, x1",
    "rev32 x0, x1",
    "clz x0, x1",
    "cls w0, w1",
    // Conditional.
    "csel x0, x1, x2, eq",
    "csinc w0, w1, w2, ne",
    "csinv x0, x1, x2, hs",
    "csneg w0, w1, w2, lo",
    "csel x0, x1, x2, cs",
    "cset w0, eq",
    "cset x9, lt",
    "csetm x0, hi",
    "cinc w9, w9, eq",
    "cinv x0, x1, ge",
    "cneg w0, w1, mi",
    "ccmp x0, x1, #4, ne",
    "ccmn w0, #3, #0, eq",
    "ccmp w0, #31, #15, gt",
    // Branches to local labels.
    "1: b 1b",
    "b 1f\n nop\n1:",
    "1: nop\n b.ne 1b",
    "b.eq 2f; nop; 2: nop",
    "1: cbz x0, 1b",
    "cbnz w3, 7f\n7:",
    "1: tbz x0, #63, 1b",
    "tbnz w1, #3, 1f\nnop\n1: ret",
    "0: adr x0, 0b",
    "1: ldr x0, 1b",
    "1: ldr w0, 1f\n1: nop",
    "1: ldrsw x2, 1b",
    "1: ldr d3, 1b",
    "1: ldr s3, 1b",
    "1: ldr q3, 1b",
    "1: b 1b\n1: b 1b",
    "br x16",
    "blr x1",
    "ret",
    "ret x1",
    // Loads and stores.
    "ldr x0, [x1]",
    "ldr x0, [x1, #8]",
    "ldr w0, [sp, #4]",
    "ldr x0, [x1, #-8]",
    "ldr x0, [x1, #3]",
    "ldr x0, [x1, #32760]",
    "ldr x0, [x1, #8]!",
    "ldr x0, [x1], #-16",
    "str x0, [x1]",
    "str w0, [x1, #252]",
    "str x0, [sp, #-16]!",
    "str x0, [x1], #8",
    "ldrb w0, [x1, #1]",
    "strb w0, [x1, #4095]",
    "ldrh w0, [x1, #2]",
    "strh w0, [x1, #-1]",
    "ldrsb x0, [x1]",
    "ldrsb w0, [x1, #3]",
    "ldrsh x0, [x1, #6]",
    "ldrsh w0, [x1]",
    "ldrsw x0, [x1, #4]",
    "ldur x0, [x1, #8]",
    "stur w0, [x1, #-4]",
    "ldurb w0, [x1, #1]",
    "sturh w0, [x1, #3]",
    "ldursw x0, [x1, #-4]",
    "ldursb w0, [x1, #-1]",
    "ldr x0, [x1, x2]",
    "ldr x0, [x1, x2, lsl #3]",
    "ldr w0, [x1, w2, uxtw]",
    "ldr w0, [x1, w2, sxtw #2]",
    "str x0, [x1, x2, sxtx]",
    "ldrb w0, [x1, x2]",
    "ldrb w0, [x1, x2, lsl #0]",
    "strh w0, [x1, x2, lsl #1]",
    "ldr d0, [x1]",
    "ldr s1, [x1, #4]",
    "str q2, [sp, #16]",
    "ldr b3, [x1, #1]",
    "str h4, [x1, #2]",
    "ldr d0, [x1, #8]!",
    "str d0, [x1], #8",
    "ldur d1, [x1, #-8]",
    "ldr d0, [x1, x2, lsl #3]",
    "ldp x29, x30, [sp], #16",
    "stp x29, x30, [sp, #-16]!",
    "ldp x0, x1, [x2]",
    "stp w0, w1, [x2, #8]",
    "ldp x0, x1, [x2, #-512]",
    "ldpsw x0, x1, [x2, #4]",
    "ldpsw x0, x1, [x2], #8",
    "ldp d0, d1, [sp, #16]",
    "stp q0, q1, [sp, #-32]!",
    "ldp s0, s1, [x0], #8",
    // Exclusives and atomics.
    "ldxr x0, [x1]",
    "ldxrb w0, [x1]",
    "ldaxrh w0, [x1]",
    "ldaxr w0, [x1, #0]",
    "ldar x0, [sp]",
    "ldarb w0, [x1]",
    "stlr x0, [x1]",
    "stlrh w0, [x1]",
    "stxr w2, x0, [x1]",
    "stlxr w2, w0, [x1]",
    "stxrb w2, w0, [x1]",
    "ldxp x0, x1, [x2]",
    "ldaxp w0, w1, [x2]",
    "stxp w3, x0, x1, [x2]",
    "stlxp w3, w0, w1, [x2]",
    "clrex",
    "cas x0, x1, [x2]",
    "casal w0, w1, [x2]",
    "casab w0, w1, [x2]",
    "swp x0, x1, [x2]",
    "swplh w0, w1, [x2]",
    "ldadd x0, x1, [x2]",
    "ldaddal w0, w1, [x2]",
    "ldclrb w0, w1, [x2]",
    "ldeora x0, x1, [x2]",
    "ldsetl w0, w1, [x2]",
    "ldsmax x0, x1, [x2]",
    "ldsminh w0, w1, [x2]",
    "ldumaxal x0, x1, [x2]",
    "lduminab w0, w1, [x2]",
    // System.
    "nop",
    "NOP",
    "yield",
    "wfe",
    "wfi",
    "sev",
    "sevl",
    "hint #34",
    "hint 127",
    "isb",
    "isb sy",
    "dmb ish",
    "dmb ishld",
    "dmb sy",
    "dsb sy",
    "dsb ishst",
    "dmb oshld",
    "svc #0",
    "svc 0x80",
    "brk #0x3e8",
    "udf #1",
    "mrs x0, tpidr_el0",
    "mrs x1, cntvct_el0",
    "mrs x0, nzcv",
    "mrs x0, fpcr",
    "mrs x0, fpsr",
    "msr fpcr, x0",
    "msr fpsr, x1",
    "msr nzcv, x2",
    "msr tpidr_el0, x3",
    "mrs x0, TPIDR_EL0",
    "bti c",
    "bti j",
    "bti jc",
    "bti",
    "paciasp",
    "autiasp",
    "pacibsp",
    "autibsp",
    // Floating point.
    "fmov d0, d1",
    "fmov s0, s1",
    "fmov d0, x1",
    "fmov x0, d1",
    "fmov s0, w1",
    "fmov w0, s1",
    "fmov d0, #1.0",
    "fmov s0, #-0.5",
    "fmov d1, #31.0",
    "fabs d0, d1",
    "fneg s0, s1",
    "fsqrt d0, d1",
    "frintz s0, s1",
    "frinta d0, d1",
    "fadd d0, d1, d2",
    "fsub s0, s1, s2",
    "fmul d0, d1, d2",
    "fdiv s0, s1, s2",
    "fnmul d0, d1, d2",
    "fmaxnm s0, s1, s2",
    "fmin d0, d1, d2",
    "fmadd d0, d1, d2, d3",
    "fnmsub s0, s1, s2, s3",
    "fcmp d0, d1",
    "fcmpe s0, s1",
    "fcmp d0, #0.0",
    "add x0, x1, #4096",
    "fcmpe d1, #0.0",
    "fccmp d0, d1, #4, ne",
    "fcsel s0, s1, s2, lt",
    "fcvt d0, s1",
    "fcvt s0, d1",
    "fcvt h0, s1",
    "fcvt d0, h1",
    "scvtf d0, x1",
    "ucvtf s0, w1",
    "scvtf s0, x1",
    "fcvtzs x0, d1",
    "fcvtzu w0, s1",
    "fcvtns x0, s1",
    "fcvtms w0, d1",
    "fcvtau x0, d1",
    // Expressions and statements.
    "add x0, x1, #(1 << 3) + 2",
    "mov x0, #0x10 * 4",
    "add x0, x1, #'a'",
    "and x0, x1, #~0xf",
    "mov w0, #(3 - 1)",
    "nop; nop\n\tnop",
    "nop // trailing; comment\nnop",
    "# a comment\n nop",
    "/* a\n block */ nop",
    "\tadd x0, x0, #1 ; sub x0, x0, #1",
];

/// Bodies refused, with the refusal.
const REFUSED: &[(&str, &str)] = &[
    ("frob x0", "asm:mnemonic/frob"),
    ("prfm pldl1keep, [x0]", "asm:mnemonic/prfm"),
    ("hlt #0", "asm:mnemonic/hlt"),
    ("add v0.4s, v1.4s, v2.4s", "asm:operand/register"),
    ("fadd v0.4s, v1.4s, v2.4s", "asm:operand/vector"),
    ("ld1 {v0.16b}, [x0]", "asm:mnemonic/ld1"),
    ("ldr x0, =0x1234", "asm:operand/literal-pool"),
    ("add x0, x1, #:lo12:sym", "asm:operand/specifier"),
    ("adrp x0, sym", "asm:mnemonic/adrp"),
    ("mrs x0, midr_el1", "asm:operand/sysreg"),
    ("msr cntvct_el0, x0", "asm:operand/sysreg"),
    ("dmb foo", "asm:operand/option"),
    ("mov x0, #0x12345", "asm:operand/range"),
    ("add x0, x1, #4097", "asm:operand/range"),
    ("fcmp s0, #0", "asm:operand/form"),
    ("add x0, x1, #0, lsl #12", "asm:operand/range"),
    ("ldr x0, [x1, #1000000]", "asm:operand/range"),
    ("add x0, w1, x2", "asm:operand/register"),
    ("ldr x0, [w1]", "asm:operand/register"),
    ("b 1b", "asm:operand/label"),
    ("1: b 1b + 4", "asm:operand/expression"),
    ("lbl: nop", "asm:label"),
    (".word 0", "asm:directive/word"),
    (".inst 0xd503201f", "asm:directive/inst"),
    ("mov x0, #1 +", "asm:operand/syntax"),
    ("ldr x0, [x1", "asm:operand/syntax"),
    ("fmov d0, #0.1", "asm:operand/fp-immediate"),
    ("lsl x0, x1, #64", "asm:operand/range"),
    ("svc #0x10000", "asm:operand/range"),
    ("\"unterminated", "asm:lexical"),
];

fn hod(body: &str, features: Features) -> Result<Vec<u32>, Refusal> {
    let mut code = Code::default();
    assemble(body, features, &mut code, &mut String::new())?;
    assert!(code.refs.is_empty(), "{body}: no symbol references expected");
    Ok(code.words)
}

/// The words of `.text` of `source` as `llvm-mc-<major>` assembles it.
fn llvm(major: u32, source: &str, scratch: &Scratch) -> Option<Vec<u32>> {
    let mc = tool(&format!("llvm-mc-{major}"))?;
    let objcopy = tool(&format!("llvm-objcopy-{major}"))?;
    let src = scratch.write(&format!("all-{major}.s"), source);
    let obj = scratch.join(format!("all-{major}.o"));
    let bin = scratch.join(format!("all-{major}.bin"));
    let out = run(
        &mc,
        [
            "--triple=aarch64-unknown-linux-gnu".as_ref(),
            "-mattr=+lse,+crc,+fp-armv8".as_ref(),
            "-filetype=obj".as_ref(),
            "-o".as_ref(),
            obj.as_os_str(),
            src.as_os_str(),
        ],
    );
    assert!(out.success(), "llvm-mc-{major} refused the accepted bodies:\n{}", out.stderr_text());
    let out = run(
        &objcopy,
        [
            "-O".as_ref(),
            "binary".as_ref(),
            "--only-section=.text".as_ref(),
            obj.as_os_str(),
            bin.as_os_str(),
        ],
    );
    assert!(out.success(), "llvm-objcopy failed: {}", out.stderr_text());
    let bytes = std::fs::read(&bin).expect("objcopy output");
    Some(bytes.chunks_exact(4).map(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]])).collect())
}

#[test]
fn accepted_bodies_match_llvm_mc() {
    let mut ours = Vec::new();
    let mut spans = Vec::new();
    let mut source = String::new();
    for body in ACCEPTED {
        let words = hod(body, ALL).unwrap_or_else(|e| panic!("{body:?} refused: {e}"));
        spans.push((body, ours.len(), words.len()));
        ours.extend(words);
        source.push_str(body);
        source.push('\n');
    }
    let scratch = Scratch::new("hod-asm-mc");
    for major in [22, 19] {
        let Some(theirs) = llvm(major, &source, &scratch) else { continue };
        let mut report = String::new();
        for &(body, at, len) in &spans {
            let (a, b) = (&ours[at..at + len], theirs.get(at..at + len));
            if Some(a) != b {
                writeln!(report, "  {body:?}: hod {a:08x?}, llvm {b:08x?}").unwrap();
            }
        }
        assert!(
            report.is_empty() && ours.len() == theirs.len(),
            "LLVM {major} disagrees ({} vs {} words):\n{report}",
            ours.len(),
            theirs.len()
        );
        println!("hod_asm: {} bodies, {} words equal llvm-mc-{major}'s", spans.len(), ours.len());
    }
}

#[test]
fn refused_bodies_are_named() {
    for &(body, name) in REFUSED {
        assert_eq!(hod(body, ALL).err().map(|r| r.0), Some(name), "{body:?}");
    }
}

/// The LSE atomics, `crc32` and floating point need their feature.
#[test]
fn features_gate_instructions() {
    let base = Features { lse: false, crc: false, fp: false };
    for body in ["cas x0, x1, [x2]", "ldaddal w0, w1, [x2]", "crc32b w0, w1, w2", "fadd d0, d1, d2"]
    {
        assert_eq!(hod(body, base).err().map(|r| r.0), Some("asm:feature"), "{body}");
    }
}
