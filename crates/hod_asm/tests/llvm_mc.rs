//! `spec/asm.md` §7: every instruction form the assembler accepts
//! encodes to the words LLVM 22's and LLVM 19's `llvm-mc` produce for
//! the same text, and the forms it refuses are refused by name.

use std::fmt::Write as _;

use hod_asm::Refusal;
use hod_asm::a64::{Code, Features, RefKind, assemble, assemble_at};
use hod_testutil::{Scratch, run, tool};

const ALL: Features = Features::ALL;

/// The `-mattr` of `llvm-mc` for [`ALL`] (`neon` is the default).
const MATTR_ALL: &str = "-mattr=+lse,+crc,+fp-armv8,+aes,+fullfp16";

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
    "bic x8, x8, #7",
    "bic sp, x21, #15",
    "bic w0, w1, #-8",
    "bic w0, w1, #0xffff0000",
    "orn x0, x1, #0xff",
    "orn sp, x1, #1",
    "eon w0, w1, #1",
    "bics x0, x1, #0xf0",
    "bics xzr, x1, #1",
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
    "hlt #0",
    "hlt #0xffff",
    "HLT #0x3e8",
    "xpaclri",
    // Prefetch (§16): every named operation, `#imm5`, every addressing form.
    "prfm pldl1keep, [x0]",
    "prfm pldl1strm, [x1]",
    "prfm pldl2keep, [x2]",
    "prfm pldl2strm, [x3]",
    "prfm pldl3keep, [x4]",
    "prfm pldl3strm, [x5]",
    "prfm pldslckeep, [x6]",
    "prfm pldslcstrm, [x7]",
    "prfm plil1keep, [x8]",
    "prfm plil1strm, [x9]",
    "prfm plil2keep, [x10]",
    "prfm plil2strm, [x11]",
    "prfm plil3keep, [x12]",
    "prfm plil3strm, [x13]",
    "prfm plislckeep, [x14]",
    "prfm plislcstrm, [x15]",
    "prfm pstl1keep, [x16]",
    "prfm pstl1strm, [x17]",
    "prfm pstl2keep, [x18]",
    "prfm pstl2strm, [x19]",
    "prfm pstl3keep, [x20]",
    "prfm pstl3strm, [x21]",
    "prfm pstslckeep, [x22]",
    "prfm pstslcstrm, [x23]",
    "prfm #0, [x0]",
    "prfm #6, [x0]",
    "prfm #24, [x0]",
    "prfm #31, [x30]",
    "prfm 5, [x0]",
    "prfm #0x1f, [x0]",
    "prfm #(2 + 3), [x0]",
    "PRFM PLDL1KEEP, [x8,1024]",
    "Prfm PldL1Keep, [X0]",
    "prfm pldl1keep, [x0, #0]",
    "prfm pldl1keep, [x0, #8]",
    "prfm pldl1keep, [x0, 8]",
    "prfm pldl1keep,[x0,#16]",
    "prfm pstl2strm, [sp, #8]",
    "prfm pldl1keep, [x0, #32760]",
    "prfm pldl1keep, [x0, x1]",
    "prfm pldl1keep, [x0, x1, lsl #3]",
    "prfm pldl1keep, [x0, x1, lsl #0]",
    "prfm pldl1keep, [x0, xzr]",
    "prfm pldl1keep, [sp, x30, lsl #3]",
    "prfm pldl1keep, [x0, w1, uxtw]",
    "prfm pldl1keep, [x0, w1, uxtw #0]",
    "prfm pldl1keep, [x0, w1, uxtw #3]",
    "prfm pldl1keep, [x0, w1, sxtw]",
    "prfm pldl1keep, [x0, w1, sxtw #0]",
    "prfm pldl1keep, [x0, w1, sxtw #3]",
    "prfm pldl1keep, [x0, x1, sxtx]",
    "prfm pldl1keep, [x0, x1, sxtx #0]",
    "prfm pldl1keep, [x0, x1, sxtx #3]",
    "prfm pldl1keep, [x0, wzr, uxtw #3]",
    "1: prfm pldl1keep, 1b",
    "prfm pstl3strm, 1f\nnop\n1:",
    "prfum pldl1keep, [x0]",
    "prfum pldl1keep, [x0, #-256]",
    "prfum pldl1keep, [x0, #255]",
    "prfum pldl1keep, [x0, #8]",
    "prfum #7, [sp, #-1]",
    "PRFUM PSTL1STRM, [x3, #-8]",
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
    // Scalar SIMD forms (§17).
    "fcvtzs d8, d9",
    "fcvtzu d8, d9",
    "fcmeq d8, d9, d9",
    "fcvtzs s0, s31",
    "FCVTZU s31, s0",
    "fcmeq s1, s2, s3",
    "fcmeq d31, d0, d17",
    "fcmeq d0, d1, #0.0",
    "fcmeq s5, s6, #0",
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

/// §20: the Advanced SIMD bodies, the census's forms first, then the
/// forms that share their encoding class.
const NEON: &[&str] = &[
    // The census (Eigen, highway, OpenCV, xxhash, abseil).
    "fmla v8.4s, v9.4s, v10.s[0]",
    "fcvtzs v8.4s, v9.4s",
    "fcvtzs v8.2d, v9.2d",
    "fcvtzs v8.4h, v9.4h",
    "fcvtzs v8.8h, v9.8h",
    "fcvtzu v8.4s, v9.4s",
    "fcvtzu v8.2d, v9.2d",
    "fcvtzu v8.4h, v9.4h",
    "fcvtzu v8.8h, v9.8h",
    "fcmeq v8.8h, v9.8h, v9.8h",
    "fcmeq v8.4s, v9.4s, v9.4s",
    "fcmeq v8.2d, v9.2d, v9.2d",
    "fcvtns v8.4s, v9.4s",
    "umlal v8.2d, v9.2s, v10.2s",
    "pmull v8.1q, v9.1d, v10.1d",
    "pmull2 v8.1q, v9.2d, v10.2d",
    "FMLA V8.4S, V9.4S, V10.S[0]",
    // By element: `fmla`, `fmls`, `fmul`, `fmulx`.
    "fmla v0.2s, v1.2s, v31.s[3]",
    "fmla v0.4s, v1.4s, v17.s[1]",
    "fmla v0.4s, v1.4s, v2.s[2]",
    "fmla v31.2d, v30.2d, v29.d[1]",
    "fmla v0.2d, v1.2d, v2.d[0]",
    "fmla v0.4h, v1.4h, v15.h[7]",
    "fmla v0.8h, v1.8h, v2.h[0]",
    "fmla v0.8h, v1.8h, v3.h[5]",
    "fmla v0.8h, v1.8h, v3.h[2]",
    "fmls v0.4s, v1.4s, v2.s[3]",
    "fmls v0.2d, v1.2d, v2.d[1]",
    "fmls v0.8h, v1.8h, v2.h[6]",
    "fmul v0.4s, v1.4s, v2.s[1]",
    "fmul v0.2d, v1.2d, v18.d[1]",
    "fmul v0.4h, v1.4h, v2.h[3]",
    "fmulx v0.2s, v1.2s, v2.s[2]",
    "fmulx v0.2d, v1.2d, v2.d[1]",
    "fmulx v0.8h, v1.8h, v2.h[1]",
    // Three same: the multiply-adds and compares.
    "fmla v0.2s, v1.2s, v2.2s",
    "fmla v0.4s, v1.4s, v2.4s",
    "fmla v0.2d, v1.2d, v2.2d",
    "fmla v0.4h, v1.4h, v2.4h",
    "fmla v0.8h, v1.8h, v2.8h",
    "fmls v0.4s, v1.4s, v2.4s",
    "fmls v0.8h, v1.8h, v2.8h",
    "fcmeq v0.2s, v1.2s, v2.2s",
    "fcmeq v0.4h, v1.4h, v2.4h",
    "fcmge v0.4s, v1.4s, v2.4s",
    "fcmge v0.8h, v1.8h, v2.8h",
    "fcmgt v0.2d, v1.2d, v2.2d",
    "fcmgt v0.4h, v1.4h, v2.4h",
    "fcmle v0.4s, v1.4s, v2.4s",
    "fcmlt v0.2d, v1.2d, v2.2d",
    "fcmlt v0.8h, v1.8h, v2.8h",
    // Compares with zero.
    "fcmeq v0.4s, v1.4s, #0.0",
    "fcmeq v0.2d, v1.2d, #0",
    "fcmeq v0.8h, v1.8h, #0.0",
    "fcmge v0.2s, v1.2s, #0.0",
    "fcmgt v0.4s, v1.4s, #0.0",
    "fcmle v0.2d, v1.2d, #0.0",
    "fcmlt v0.4h, v1.4h, #0.0",
    // Three same, `hod_mc_a64::neon::Fp3`.
    "fadd v0.4s, v1.4s, v2.4s",
    "fadd v0.8h, v1.8h, v2.8h",
    "fsub v0.2d, v1.2d, v2.2d",
    "fmul v0.2s, v1.2s, v2.2s",
    "fdiv v0.4h, v1.4h, v2.4h",
    "fmax v0.4s, v1.4s, v2.4s",
    "fmin v0.2d, v1.2d, v2.2d",
    "fmaxnm v0.8h, v1.8h, v2.8h",
    "fminnm v0.2s, v1.2s, v2.2s",
    "fmulx v0.4s, v1.4s, v2.4s",
    "frecps v0.2d, v1.2d, v2.2d",
    "frsqrts v0.4h, v1.4h, v2.4h",
    "fabd v0.4s, v1.4s, v2.4s",
    "facge v0.2d, v1.2d, v2.2d",
    "facgt v0.8h, v1.8h, v2.8h",
    "faddp v0.4s, v1.4s, v2.4s",
    "fmaxp v0.2s, v1.2s, v2.2s",
    "fminp v0.2d, v1.2d, v2.2d",
    "fmaxnmp v0.4h, v1.4h, v2.4h",
    "fminnmp v0.4s, v1.4s, v2.4s",
    // Two-register miscellaneous, `hod_mc_a64::neon::Fp2`.
    "fcvtns v0.2d, v1.2d",
    "fcvtns v0.8h, v1.8h",
    "fcvtnu v0.4s, v1.4s",
    "fcvtms v0.2s, v1.2s",
    "fcvtmu v0.2d, v1.2d",
    "fcvtas v0.4h, v1.4h",
    "fcvtau v0.4s, v1.4s",
    "fcvtps v0.2d, v1.2d",
    "fcvtpu v0.8h, v1.8h",
    "fcvtzs v0.2s, v1.2s",
    "fcvtzu v31.2s, v0.2s",
    "scvtf v0.4s, v1.4s",
    "ucvtf v0.2d, v1.2d",
    "scvtf v0.8h, v1.8h",
    "frecpe v0.4s, v1.4s",
    "frsqrte v0.2d, v1.2d",
    "fabs v0.4s, v1.4s",
    "fneg v0.8h, v1.8h",
    "fsqrt v0.2d, v1.2d",
    "frintn v0.4s, v1.4s",
    "frintm v0.2d, v1.2d",
    "frintp v0.4h, v1.4h",
    "frintz v0.2s, v1.2s",
    "frinta v0.4s, v1.4s",
    "frintx v0.2d, v1.2d",
    "frinti v0.8h, v1.8h",
    // Their scalar forms.
    "fcvtzs h0, h1",
    "fcvtzu h31, h30",
    "fcvtns s0, s1",
    "fcvtnu d0, d1",
    "fcvtms s0, s1",
    "fcvtmu h0, h1",
    "fcvtas d0, d1",
    "fcvtau s0, s1",
    "fcvtps d0, d1",
    "fcvtpu s0, s1",
    "scvtf s0, s1",
    "ucvtf d0, d1",
    "scvtf h0, h1",
    "frecpe s0, s1",
    "frsqrte d0, d1",
    // Three different, and the `2` forms.
    "umlal2 v8.2d, v9.4s, v10.4s",
    "umlal v0.8h, v1.8b, v2.8b",
    "umlal2 v0.4s, v1.8h, v2.8h",
    "smlal v0.4s, v1.4h, v2.4h",
    "smlal2 v0.8h, v1.16b, v2.16b",
    "umlsl v0.2d, v1.2s, v2.2s",
    "smlsl2 v0.2d, v1.4s, v2.4s",
    "umull v0.2d, v1.2s, v2.2s",
    "smull2 v0.8h, v1.16b, v2.16b",
    "saddl v0.4s, v1.4h, v2.4h",
    "uaddl2 v0.2d, v1.4s, v2.4s",
    "ssubl v0.8h, v1.8b, v2.8b",
    "usubl2 v0.4s, v1.8h, v2.8h",
    "sabal v0.2d, v1.2s, v2.2s",
    "uabal2 v0.8h, v1.16b, v2.16b",
    "sabdl v0.4s, v1.4h, v2.4h",
    "uabdl2 v0.2d, v1.4s, v2.4s",
    "sqdmlal v0.4s, v1.4h, v2.4h",
    "sqdmlsl2 v0.2d, v1.4s, v2.4s",
    "sqdmull v0.2d, v1.2s, v2.2s",
    "addhn v0.8b, v1.8h, v2.8h",
    "raddhn2 v0.16b, v1.8h, v2.8h",
    "subhn v0.4h, v1.4s, v2.4s",
    "rsubhn2 v0.4s, v1.2d, v2.2d",
    "pmull v0.8h, v1.8b, v2.8b",
    "pmull2 v0.8h, v1.16b, v2.16b",
    "pmull v31.1q, v0.1d, v31.1d",
    // Structure loads and stores.
    "ld1 {v0.16b}, [x0]",
    "ld1 {v0.8b}, [sp]",
    "ld1 {v0.1d}, [x0]",
    "ld1 {v0.4s, v1.4s}, [x1]",
    "ld1 {v0.2d, v1.2d, v2.2d}, [x2]",
    "ld1 {v28.8h, v29.8h, v30.8h, v31.8h}, [x3]",
    "ld1 {v30.4h-v1.4h}, [x3]",
    "ld1 { v0.2s - v3.2s }, [x0]",
    "ld1 {v0.16b}, [x0], #16",
    "ld1 {v0.8b, v1.8b}, [x0], #16",
    "ld1 {v0.4s-v3.4s}, [x0], #64",
    "ld1 {v0.16b}, [x0], x2",
    "ld2 {v0.4s, v1.4s}, [x0]",
    "ld3 {v0.8b, v1.8b, v2.8b}, [x0], #24",
    "ld4 {v0.2d, v1.2d, v2.2d, v3.2d}, [x0], x5",
    "st1 {v0.16b}, [x0]",
    "st1 {v0.1d, v1.1d}, [x0], #16",
    "st1 {v4.4s-v7.4s}, [sp]",
    "st2 {v0.8h, v1.8h}, [x0], #32",
    "st3 {v0.4h, v1.4h, v2.4h}, [x0]",
    "st4 {v0.16b, v1.16b, v2.16b, v3.16b}, [x0], x1",
    "ld1 {v0.b}[15], [x0]",
    "ld1 {v0.h}[7], [x0], #2",
    "ld1 {v0.s}[3], [x0], x9",
    "ld1 {v0.d}[1], [x0]",
    "ld2 {v0.s, v1.s}[1], [x0], #8",
    "ld3 {v0.h, v1.h, v2.h}[5], [x0]",
    "ld4 {v0.d, v1.d, v2.d, v3.d}[0], [x0], #32",
    "st1 {v0.s}[2], [x0]",
    "st2 {v0.b, v1.b}[9], [x0], #2",
    "st4 {v0.h, v1.h, v2.h, v3.h}[3], [sp]",
    "ld1r {v0.4s}, [x0]",
    "ld1r {v0.8b}, [x0], #1",
    "ld2r {v0.2d, v1.2d}, [x0], #16",
    "ld3r {v0.4h, v1.4h, v2.4h}, [x0], x3",
    "ld4r {v0.1d, v1.1d, v2.1d, v3.1d}, [x0]",
];

/// Bodies refused, with the refusal.
const REFUSED: &[(&str, &str)] = &[
    ("frob x0", "asm:mnemonic/frob"),
    ("prfm pldl4keep, [x0]", "asm:operand/option"),
    ("prfm x0, [x1]", "asm:operand/option"),
    ("prfm #32, [x0]", "asm:operand/range"),
    ("prfm #-1, [x0]", "asm:operand/range"),
    ("prfm pldl1keep, [x0, #32768]", "asm:operand/range"),
    ("prfm pldl1keep, [x0, #-257]", "asm:operand/range"),
    ("prfm pldl1keep, [x0, #257]", "asm:operand/range"),
    ("prfm pldl1keep, [x0, x1, lsl #2]", "asm:operand/range"),
    ("prfm pldl1keep, [x0, w1]", "asm:operand/register"),
    ("prfm pldl1keep, [x0, x1, uxtw]", "asm:operand/register"),
    ("prfm pldl1keep, [x0, sp]", "asm:operand/register"),
    ("prfm pldl1keep, [w0]", "asm:operand/register"),
    ("prfm pldl1keep, [x0, #8]!", "asm:operand/form"),
    ("prfm pldl1keep, [x0], #8", "asm:operand/form"),
    ("prfm pldl1keep", "asm:operand/form"),
    ("prfm pldl1keep, #8", "asm:operand/symbol"),
    ("prfm pldl1keep, =0x10", "asm:operand/literal-pool"),
    ("prfm pldl1keep, [x0, #:lo12:sym]", "asm:operand/specifier"),
    ("prfum pldl1keep, [x0, #256]", "asm:operand/range"),
    ("prfum pldl1keep, [x0, x1]", "asm:operand/form"),
    ("prfum pldl1keep, [x0, #-8]!", "asm:operand/form"),
    ("prfum pldl1keep, 1f\n1:", "asm:operand/form"),
    ("hlt #0x10000", "asm:operand/range"),
    ("bic x0, x1, #0", "asm:operand/range"),
    ("bic x0, x1, #5", "asm:operand/range"),
    ("bic w0, w1, #0xffffffff", "asm:operand/range"),
    ("bics sp, x1, #1", "asm:operand/register"),
    ("xpaclri x0", "asm:operand/form"),
    ("add v0.4s, v1.4s, v2.4s", "asm:operand/register"),
    ("fadd v0.4s, v1.4s, v2.2s", "asm:operand/vector"),
    ("ld1 {v0.16b}, [x0, #16]", "asm:operand/form"),
    ("ldr x0, =0x1234", "asm:operand/literal-pool"),
    ("add x0, x1, #:lo12:sym", "asm:operand/specifier"),
    ("adrp x0, sym", "asm:mnemonic/adrp"),
    ("mrs x0, midr_el2", "asm:operand/sysreg"),
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
    // §17.
    ("fcmeq d0, d1, #1.0", "asm:operand/fp-immediate"),
    ("fcmeq d0, d1, #1", "asm:operand/fp-immediate"),
    ("fcmeq d0, s1, s2", "asm:operand/register"),
    ("fcmeq h0, h1, h2", "asm:operand/register"),
    ("fcmeq x0, d1, d2", "asm:operand/register"),
    ("fcmeq v0.4s, v1.4s, v2.2d", "asm:operand/vector"),
    ("fcvtzs d0, s1", "asm:operand/register"),
    ("fcvtzs d0, d1, #3", "asm:operand/form"),
    ("fcvtzu v0.1d, v1.1d", "asm:operand/vector"),
    // §20.
    ("fmla v0.4s, v1.4s, v2.s[4]", "asm:operand/range"),
    ("fmla v0.2d, v1.2d, v2.d[2]", "asm:operand/range"),
    ("fmla v0.8h, v1.8h, v16.h[0]", "asm:operand/register"),
    ("fmla v0.4s, v1.4s, v2.d[0]", "asm:operand/vector"),
    ("fmla v0.4s, v1.2s, v2.4s", "asm:operand/vector"),
    ("fmla v0.1d, v1.1d, v2.1d", "asm:operand/vector"),
    ("fmla v0.16b, v1.16b, v2.16b", "asm:operand/vector"),
    ("fmla s0, s1, v2.s[1]", "asm:operand/register"),
    ("fmla v0.4s, v1.4s", "asm:operand/form"),
    ("fmla v0.4s, v1.4s, v2", "asm:operand/vector"),
    ("fmla v0.4s, v1.4s, v2.3s", "asm:operand/vector"),
    ("fcvtzs v0.4s, v1.4s, #3", "asm:operand/form"),
    ("fcvtzs v0.4s, v1.2d", "asm:operand/vector"),
    ("fcvtzs x0, v1.2d", "asm:operand/register"),
    ("fcmeq v0.4s, v1.4s, #1.0", "asm:operand/fp-immediate"),
    ("fcmeq v0.4s, v1.4s, #1", "asm:operand/fp-immediate"),
    ("umlal v0.2d, v1.4s, v2.4s", "asm:operand/vector"),
    ("umlal2 v0.2d, v1.2s, v2.2s", "asm:operand/vector"),
    ("umlal v0.4s, v1.2s, v2.2s", "asm:operand/vector"),
    ("umlal v0.2d, v1.2s, v2.4h", "asm:operand/vector"),
    ("umlal v0.1q, v1.1d, v2.1d", "asm:operand/vector"),
    ("pmull v0.4s, v1.4h, v2.4h", "asm:operand/vector"),
    ("addhn v0.8b, v1.8b, v2.8b", "asm:operand/vector"),
    ("umlal x0, x1, x2", "asm:operand/form"),
    ("ld1 {v0.16b}, [x0], #8", "asm:operand/range"),
    ("ld1 {v0.16b}, [x0], xzr", "asm:operand/register"),
    ("ld1 {v0.16b}, [x0], w1", "asm:operand/register"),
    ("ld1 {v0.16b}, [x0, #16]!", "asm:operand/form"),
    ("ld1 {v0.16b}, [w0]", "asm:operand/register"),
    ("ld1 {v0.16b}, x0", "asm:operand/form"),
    ("ld2 {v0.1d, v1.1d}, [x0]", "asm:operand/vector"),
    ("ld2 {v0.4s}, [x0]", "asm:operand/vector"),
    ("ld1 {v0.s}[4], [x0]", "asm:operand/range"),
    ("ld1 {v0.q}[0], [x0]", "asm:operand/vector"),
    ("ld1r {v0.s}[0], [x0]", "asm:operand/vector"),
    ("ld1 {v0.16b-v4.16b}, [x0]", "asm:operand/vector"),
    ("ld1 {v0.16b, v2.16b}, [x0]", "asm:operand/vector"),
    ("ld1 {v0.16b, v1.8b}, [x0]", "asm:operand/vector"),
    ("frint32z v0.4s, v1.4s", "asm:mnemonic/frint32z"),
    ("frecpx v0.4s, v1.4s", "asm:mnemonic/frecpx"),
    ("mov v0.16b, v1.16b", "asm:operand/register"),
    ("ld5 {v0.16b}, [x0]", "asm:mnemonic/ld5"),
    (".balign 3", "asm:operand/range"),
    (".p2align 17", "asm:operand/range"),
    (".p2align -1", "asm:operand/range"),
    (".p2align 4,,0", "asm:operand/range"),
    (".p2align 4,0x100", "asm:operand/range"),
    (".p2align 4,1,2,3", "asm:operand/form"),
    (".p2align 4,", "asm:operand/syntax"),
    (".p2align x", "asm:operand/symbol"),
    (".balignw 4", "asm:directive/balignw"),
    (".balignl 4", "asm:directive/balignl"),
    (".p2alignw 2", "asm:directive/p2alignw"),
];

/// Bodies with alignment directives (§17), each assembled at several
/// offsets from an origin aligned to 64: hod's words are `llvm-mc` 22's
/// and 19's for the body at that offset of its section. (A fill of 0 in
/// code is the one difference: LLVM 19 writes `nop`s, 22 zeros, and hod
/// follows 22: `fill_zero_is_llvm_22_s`.)
const ALIGNED: &[&str] = &[
    ".p2align 5",
    "nop\n.p2align 5\nnop",
    ".p2align 6\nnop\n.p2align 5\nnop\n.p2align 4",
    ".balign 64\nnop\n.balign 16\nnop",
    ".align 4\nnop\n.ALIGN 3\nnop",
    ".p2align 0\n.p2align 1\n.balign 1\n.balign 2\n.balign 0\nnop",
    "nop\n.p2align 4, 0xff\nnop",
    "nop\n.balign 32, -1\nnop",
    "nop\n.p2align 4, 0x5a\nnop",
    "nop\n.p2align 6,,8\nnop",
    "nop\n.p2align 6,,12\nnop",
    "nop\n.p2align 4,0xff,8\nnop",
    "nop\n.p2align 4,,16\nnop",
    "nop\n.balign 32,,28\nnop",
    "b 1f\n.p2align 5\n1: nop\ncbz x0, 1b",
    "adr x0, 2f\n.p2align 4\n2: nop\nadr x1, 2b",
    "1:\n.p2align 4\nb 1b",
    "adr x8, 0f\nadd x8, x8, x10, lsl #3\nmov w9, #0x80\nbr x8\n.p2align 3\n0:\n\
     orr x8, x9, x11, lsr #56\nstrb w8, [x12, #8]\norr w8, w11, #0x80\nstrb w8, [x12]",
    ".P2ALIGN 3\nnop",
];

#[test]
fn aligned_bodies_are_llvm_mc_words_at_each_offset() {
    let scratch = Scratch::new("hod-asm-mc-align");
    let mut source = String::new();
    let mut ours = Vec::new();
    for (i, body) in ALIGNED.iter().enumerate() {
        for start in (0..64).step_by(4) {
            let mut code = Code::default();
            assemble_at(body, ALL, start, &mut code, &mut String::new())
                .unwrap_or_else(|e| panic!("{body:?} at {start}: {e}"));
            assert!(code.align >= 1, "{body:?}: an alignment");
            ours.push((i, start, code.words));
            source.push_str(".p2align 6\n");
            for _ in 0..start / 4 {
                source.push_str("nop\n");
            }
            source.push_str(body);
            // The next case starts at the next 64-byte boundary: pad it
            // with something that is not a `nop` the oracle could write.
            source.push_str("\n.p2align 6, 0xee\n");
        }
    }
    for major in [22, 19] {
        let Some(theirs) = llvm(major, &source, &scratch) else { continue };
        let mut at = 0;
        let mut report = String::new();
        for (i, start, words) in &ours {
            at += (*start / 4) as usize;
            let got = &theirs[at..at + words.len()];
            if got != words.as_slice() {
                writeln!(
                    report,
                    "  {:?} at {start}: hod {words:08x?}, llvm {got:08x?}",
                    ALIGNED[*i]
                )
                .unwrap();
            }
            at += words.len();
            // Skip the 0xee padding to the next case.
            while at < theirs.len() && theirs[at] == 0xeeee_eeee {
                at += 1;
            }
        }
        assert!(report.is_empty(), "LLVM {major} disagrees:\n{report}");
        assert_eq!(at, theirs.len(), "LLVM {major}: the cases did not line up");
        println!("llvm_mc: {} aligned bodies at 16 offsets equal llvm-mc-{major}'s", ALIGNED.len());
    }
}

/// `.p2align` with a fill of 0 in code: `llvm-mc` 22 writes zeros, 19
/// `nop`s; hod writes 22's.
#[test]
fn fill_zero_is_llvm_22_s() {
    let body = "nop\n.balign 16, 0\nnop";
    let mut code = Code::default();
    assemble_at(body, ALL, 0, &mut code, &mut String::new()).unwrap();
    assert_eq!(code.words, [0xd503_201f, 0, 0, 0, 0xd503_201f]);
    let scratch = Scratch::new("hod-asm-mc-fill-zero");
    if let Some(theirs) = llvm(22, body, &scratch) {
        assert_eq!(theirs, code.words, "llvm-mc-22");
    }
    if let Some(theirs) = llvm(19, body, &scratch) {
        assert_eq!(theirs, [0xd503_201f; 5], "llvm-mc-19");
    }
}

fn hod(body: &str, features: Features) -> Result<Vec<u32>, Refusal> {
    let mut code = Code::default();
    assemble(body, features, &mut code, &mut String::new())?;
    assert!(code.refs.is_empty(), "{body}: no symbol references expected");
    Ok(code.words)
}

/// The words of `.text` of `source` as `llvm-mc-<major>` assembles it.
fn llvm(major: u32, source: &str, scratch: &Scratch) -> Option<Vec<u32>> {
    let words = llvm_try(major, source, scratch)?;
    Some(words.unwrap_or_else(|e| panic!("llvm-mc-{major} refused the accepted bodies:\n{e}")))
}

/// As [`llvm`], with `llvm-mc`'s error text when it refuses `source`.
fn llvm_try(major: u32, source: &str, scratch: &Scratch) -> Option<Result<Vec<u32>, String>> {
    llvm_with(major, source, MATTR_ALL, scratch)
}

/// As [`llvm_try`], under `mattr` (`-mattr=...`).
fn llvm_with(
    major: u32,
    source: &str,
    mattr: &str,
    scratch: &Scratch,
) -> Option<Result<Vec<u32>, String>> {
    let mc = tool(&format!("llvm-mc-{major}"))?;
    let objcopy = tool(&format!("llvm-objcopy-{major}"))?;
    let src = scratch.write(&format!("all-{major}.s"), source);
    let obj = scratch.join(format!("all-{major}.o"));
    let bin = scratch.join(format!("all-{major}.bin"));
    let out = run(
        &mc,
        [
            "--triple=aarch64-unknown-linux-gnu".as_ref(),
            mattr.as_ref(),
            "-filetype=obj".as_ref(),
            "-o".as_ref(),
            obj.as_os_str(),
            src.as_os_str(),
        ],
    );
    if !out.success() {
        return Some(Err(out.stderr_text()));
    }
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
    Some(Ok(bytes.chunks_exact(4).map(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]])).collect()))
}

/// `prfm` with an offset only the unscaled form takes (§16): LLVM 22's
/// assembler writes `prfum`, LLVM 19's rejects the text. hod writes
/// LLVM 22's word (kiln's LLVM).
const ACCEPTED_22: &[&str] = &[
    "prfm pldl1keep, [x0, #-8]",
    "prfm pldl1keep, [x0, #4]",
    "prfm pldl1keep, [x0, #255]",
    "prfm pldl1keep, [x0, #-256]",
    "prfm pstl1strm, [sp, #1]",
    "PRFM PLIL2KEEP, [x9,-128]",
];

#[test]
fn unscaled_prfm_is_llvm_22_s_prfum() {
    let scratch = Scratch::new("hod-asm-mc-prfum");
    for body in ACCEPTED_22 {
        let ours = hod(body, ALL).unwrap_or_else(|e| panic!("{body:?} refused: {e}"));
        let unscaled = body.to_ascii_lowercase().replacen("prfm", "prfum", 1);
        assert_eq!(hod(&unscaled, ALL), Ok(ours.clone()), "{body:?} is its prfum");
        if let Some(theirs) = llvm_try(22, body, &scratch) {
            assert_eq!(theirs.as_ref(), Ok(&ours), "{body:?}: llvm-mc-22");
        }
        if let Some(theirs) = llvm_try(19, body, &scratch) {
            assert!(theirs.is_err(), "{body:?}: llvm-mc-19 now accepts it: {theirs:08x?}");
        }
    }
}

/// `prfm <op>, <symbol>`: the literal form, a reference for the caller.
#[test]
fn prfm_of_a_symbol_is_a_literal_reference() {
    let mut code = Code::default();
    assemble("prfm pldl2keep, sym + 8", ALL, &mut code, &mut String::new()).expect("assembled");
    assert_eq!(code.words, [0xd800_0002]);
    let [r] = &code.refs[..] else { panic!("one reference: {:?}", code.refs) };
    assert_eq!((r.offset, r.kind, &*r.name, r.addend), (0, RefKind::Literal19, "sym", 8));
}

#[test]
fn accepted_bodies_match_llvm_mc() {
    bodies_match_llvm_mc(ACCEPTED, "hod-asm-mc");
}

/// §20: the Advanced SIMD forms on vector operands, and the scalar SIMD
/// forms of the two-register class.
#[test]
fn neon_bodies_match_llvm_mc() {
    bodies_match_llvm_mc(NEON, "hod-asm-mc-neon");
}

fn bodies_match_llvm_mc(bodies: &[&str], label: &str) {
    let mut ours = Vec::new();
    let mut spans = Vec::new();
    let mut source = String::new();
    for body in bodies {
        let words = hod(body, ALL).unwrap_or_else(|e| panic!("{body:?} refused: {e}"));
        spans.push((body, ours.len(), words.len()));
        ours.extend(words);
        source.push_str(body);
        source.push('\n');
    }
    let scratch = Scratch::new(label);
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
        println!("{}: {} bodies, {} words equal llvm-mc-{major}'s", label, spans.len(), ours.len());
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
    let base = Features {
        lse: false,
        crc: false,
        fp: false,
        rand: false,
        neon: false,
        aes: false,
        fp16: false,
    };
    for body in [
        "cas x0, x1, [x2]",
        "ldaddal w0, w1, [x2]",
        "crc32b w0, w1, w2",
        "fadd d0, d1, d2",
        // §20.
        "fmla v0.4s, v1.4s, v2.s[1]",
        "fcvtzs d0, d1",
        "fcmeq d0, d1, d2",
        "ld1 {v0.16b}, [x0]",
        "umlal v0.2d, v1.2s, v2.2s",
    ] {
        assert_eq!(hod(body, base).err().map(|r| r.0), Some("asm:feature"), "{body}");
    }
}

/// §20: an Advanced SIMD body is accepted under a `target-features`
/// value exactly when `llvm-mc` 22 and 19 accept it under the same
/// `-mattr` (the order of the items and LLVM's implications among
/// `fp-armv8`, `neon`, `aes`, `crypto`, `fullfp16`, `fp16fml`, `sve`);
/// hod's refusal is `asm:feature`.
#[test]
fn neon_features_are_llvm_s() {
    const BODIES: &[&str] = &[
        "pmull v0.1q, v1.1d, v2.1d",
        "pmull2 v0.1q, v1.2d, v2.2d",
        "pmull v0.8h, v1.8b, v2.8b",
        "fmla v0.4s, v1.4s, v2.s[1]",
        "fcmeq v0.8h, v1.8h, v2.8h",
        "fcvtzs v0.4h, v1.4h",
        "fmla v0.8h, v1.8h, v2.h[7]",
        "fcvtzs d0, d1",
        "fcvtzs h0, h1",
        "fcmeq s0, s1, #0.0",
        "fadd d0, d1, d2",
        "ld1 {v0.16b}, [x0]",
        "umlal v0.2d, v1.2s, v2.2s",
    ];
    const ATTRS: &[&str] = &[
        "",
        "+aes",
        "+crypto",
        "+aes,-neon",
        "-neon,+aes",
        "-neon",
        "-fp-armv8",
        "-fp-armv8,+neon",
        "+fullfp16",
        "+fullfp16,-neon",
        "-neon,+fullfp16",
        "+fullfp16,-fp-armv8",
        "+fp16fml",
        "+sve",
        "+v8.2a,+crypto",
        "+v8.2a",
        "+v9a",
        "+sha2",
    ];
    let scratch = Scratch::new("hod-asm-mc-features");
    let (mut cells, mut accepted) = (0, 0);
    let mut report = String::new();
    for attrs in ATTRS {
        let features = Features::from_attribute(attrs);
        for body in BODIES {
            let ours = hod(body, features);
            if let Err(e) = &ours {
                assert_eq!(e.0, "asm:feature", "{body:?} under {attrs:?}");
            }
            for major in [22, 19] {
                let Some(theirs) = llvm_with(major, body, &format!("-mattr={attrs}"), &scratch)
                else {
                    continue;
                };
                cells += 1;
                accepted += usize::from(theirs.is_ok());
                if theirs.is_ok() != ours.is_ok() {
                    writeln!(
                        report,
                        "  {body:?} [{attrs}]: hod {ours:?}, llvm-mc-{major} {theirs:?}"
                    )
                    .unwrap();
                }
            }
        }
    }
    assert!(report.is_empty(), "the features differ from llvm-mc's:\n{report}");
    println!("neon features: {cells} cells agree with llvm-mc ({accepted} accepted)");
}
