//! Sweeps for `spec/mc-x64.md` "x87": every stack register in every
//! register form, every condition of `fcmov` (and the eight it lacks),
//! every operation and width, every memory form of "Memory operands"
//! through every memory constructor (`x87-memory-operands`, which also
//! takes the absolute fields), and the instructions `llc` writes for
//! `x86_fp80`, in its own AT&T text (`x87-llc-text`).

use hod_mc_x64::*;

use crate::harness::Batch;
use crate::text::{CONDS, FPS, M, all_mems, fp_ptr, ptr};

fn st(n: u8) -> St {
    St::new(n).expect("stack register 0..=7")
}

/// `dword ptr`, `qword ptr`, `tbyte ptr`.
fn x87_ptr(w: X87Size) -> &'static str {
    match w {
        X87Size::S => "dword ptr",
        X87Size::D => "qword ptr",
        X87Size::T => "tbyte ptr",
    }
}

const WIDTHS: [X87Size; 3] = [X87Size::S, X87Size::D, X87Size::T];

/// The conditions `fcmov` has, as LLVM names them.
const FCMOVS: [(Cond, &str); 8] = [
    (Cond::B, "fcmovb"),
    (Cond::E, "fcmove"),
    (Cond::Be, "fcmovbe"),
    (Cond::P, "fcmovu"),
    (Cond::Ae, "fcmovnb"),
    (Cond::Ne, "fcmovne"),
    (Cond::A, "fcmovnbe"),
    (Cond::Np, "fcmovnu"),
];

pub fn build(b: &mut Batch) {
    registers(b);
    fixed(b);
    // A sample of memory forms through the constructors that
    // `memory` takes over every form.
    for m in crate::text::some_mems() {
        memory_forms(b, m);
    }
    rejections(b);
}

fn registers(b: &mut Batch) {
    for i in 0..8 {
        let s = st(i);
        emit!(b, fld_st(s), "fld st({i})");
        emit!(b, fst_st(s), "fst st({i})");
        emit!(b, fstp_st(s), "fstp st({i})");
        emit!(b, fxch(s), "fxch st({i})");
        macro_rules! forms {
            ($($zero_i:ident $i_zero:ident $pop:ident $mn:literal;)*) => {$(
                emit!(b, $zero_i(s), concat!($mn, " st, st({})"), i);
                emit!(b, $i_zero(s), concat!($mn, " st({}), st"), i);
                emit!(b, $pop(s), concat!($mn, "p st({}), st"), i);
            )*};
        }
        forms! {
            fadd_0i fadd_i0 faddp "fadd";
            fmul_0i fmul_i0 fmulp "fmul";
            fsub_0i fsub_i0 fsubp "fsub";
            fsubr_0i fsubr_i0 fsubrp "fsubr";
            fdiv_0i fdiv_i0 fdivp "fdiv";
            fdivr_0i fdivr_i0 fdivrp "fdivr";
        }
        emit!(b, fucomi(s), "fucomi st, st({i})");
        emit!(b, fucomip(s), "fucomip st, st({i})");
        emit!(b, fcomi(s), "fcomi st, st({i})");
        emit!(b, fcomip(s), "fcomip st, st({i})");
        emit!(b, fucom(s), "fucom st({i})");
        emit!(b, fucomp(s), "fucomp st({i})");
        for (cond, mn) in FCMOVS {
            emit_as!(b, mn, try_fcmov(cond, s), "{mn} st, st({i})");
        }
    }
}

fn fixed(b: &mut Batch) {
    emit!(b, fucompp(), "fucompp");
    emit!(b, fnstsw_ax(), "fnstsw ax");
    emit!(b, fchs(), "fchs");
    emit!(b, fabs(), "fabs");
    emit!(b, fldz(), "fldz");
    emit!(b, fld1(), "fld1");
    emit!(b, fprem(), "fprem");
    emit!(b, fprem1(), "fprem1");
    emit!(b, fsqrt(), "fsqrt");
    emit!(b, frndint(), "frndint");
    emit!(b, fnclex(), "fnclex");
    emit!(b, fninit(), "fninit");
    emit!(b, fnop(), "fnop");
    emit!(b, ftst(), "ftst");
    emit!(b, fxam(), "fxam");
    emit!(b, fldl2t(), "fldl2t");
    emit!(b, fldl2e(), "fldl2e");
    emit!(b, fldpi(), "fldpi");
    emit!(b, fldlg2(), "fldlg2");
    emit!(b, fldln2(), "fldln2");
    emit!(b, f2xm1(), "f2xm1");
    emit!(b, fyl2x(), "fyl2x");
    emit!(b, fptan(), "fptan");
    emit!(b, fpatan(), "fpatan");
    emit!(b, fxtract(), "fxtract");
    emit!(b, fdecstp(), "fdecstp");
    emit!(b, fincstp(), "fincstp");
    emit!(b, fyl2xp1(), "fyl2xp1");
    emit!(b, fsincos(), "fsincos");
    emit!(b, fscale(), "fscale");
    emit!(b, fsin(), "fsin");
    emit!(b, fcos(), "fcos");
}

/// Every memory constructor at every width it has, on `m`.
fn memory_forms(b: &mut Batch, m: M) {
    let t = m.text();
    for w in WIDTHS {
        let p = x87_ptr(w);
        emit!(b, fld_m(w, m.mem()), "fld {p} {t}");
        emit!(b, fstp_m(w, m.mem()), "fstp {p} {t}");
    }
    for w in [X87Size::S, X87Size::D] {
        emit!(b, try_fst_m(w, m.mem()), "fst {} {t}", x87_ptr(w));
    }
    for size in [Size::W, Size::D, Size::Q] {
        let p = ptr(size);
        emit!(b, try_fild_m(size, m.mem()), "fild {p} {t}");
        emit!(b, try_fistp_m(size, m.mem()), "fistp {p} {t}");
    }
    for size in [Size::W, Size::D] {
        emit!(b, try_fist_m(size, m.mem()), "fist {} {t}", ptr(size));
    }
    for (fp, _) in FPS {
        let p = fp_ptr(fp);
        emit!(b, fadd_m(fp, m.mem()), "fadd {p} {t}");
        emit!(b, fmul_m(fp, m.mem()), "fmul {p} {t}");
        emit!(b, fsub_m(fp, m.mem()), "fsub {p} {t}");
        emit!(b, fsubr_m(fp, m.mem()), "fsubr {p} {t}");
        emit!(b, fdiv_m(fp, m.mem()), "fdiv {p} {t}");
        emit!(b, fdivr_m(fp, m.mem()), "fdivr {p} {t}");
    }
    emit!(b, fnstcw_m(m.mem()), "fnstcw word ptr {t}");
    emit!(b, fldcw_m(m.mem()), "fldcw word ptr {t}");
}

fn rejections(b: &mut Batch) {
    for m in [M::base(0, 0), M::bi(12, 9, 8, -4), M::rip(16)] {
        let t = m.text();
        reject!(b, try_fst_m(X87Size::T, m.mem()), "fst tbyte ptr {t}");
        reject!(b, try_fild_m(Size::B, m.mem()), "fild byte ptr {t}");
        reject!(b, try_fist_m(Size::B, m.mem()), "fist byte ptr {t}");
        reject!(b, try_fist_m(Size::Q, m.mem()), "fist qword ptr {t}");
        reject!(b, try_fistp_m(Size::B, m.mem()), "fistp byte ptr {t}");
    }
    for (cond, cc) in CONDS {
        if FCMOVS.iter().any(|(c, _)| *c == cond) {
            continue;
        }
        for i in [0, 1, 7] {
            reject!(b, try_fcmov(cond, st(i)), "fcmov{cc} st, st({i})");
        }
    }
}

/// Every form of "Memory operands" through every memory constructor, and
/// every form but RIP-relative as an absolute field (`{disp32}`) through
/// the loads, stores and arithmetic `llc` uses with a constant-pool or
/// TLS address.
pub fn memory(b: &mut Batch) {
    for m in all_mems() {
        memory_forms(b, m);
        if m.is_rip() {
            continue;
        }
        let field = m.mem().with_abs32().expect("only RIP-relative operands refuse");
        let t = m.text();
        for w in WIDTHS {
            let p = x87_ptr(w);
            let load = fld_m(w, field);
            let site = load.site().expect("an absolute field is a site");
            assert_eq!((site.offset, site.fixup, site.addend), (load.len() - 4, Fixup::Abs32, 0));
            emit!(b, fld_m(w, field), "{{disp32}} fld {p} {t}");
            emit!(b, fstp_m(w, field), "{{disp32}} fstp {p} {t}");
        }
        emit!(b, try_fild_m(Size::Q, field), "{{disp32}} fild qword ptr {t}");
        emit!(b, fadd_m(FpSize::S, field), "{{disp32}} fadd dword ptr {t}");
    }
}

/// One case in AT&T syntax, as `llc` writes it.
fn att(b: &mut Batch, name: &'static str, inst: Inst, mnemonic: Option<&str>, text: &str) {
    b.push(
        name,
        inst,
        mnemonic.map(String::from),
        format!(".att_syntax; {text}; .intel_syntax noprefix"),
    );
}

/// Every x87 instruction in `docs/design/0006-data/abi/03-x87.llc*.s` (the
/// same at LLVM 19 and 22), and those `llc -O0` writes for `fsub`, `fmul`,
/// `fdiv`, `fneg`, `fabs`, `sqrt`, `select` after `fcmp`, `sitofp` from
/// 16, 32 and 64 bits, `uitofp`, `fpext`, `fptrunc`, `fptosi` to 16 and
/// 32 bits and constants, in `llc`'s own AT&T text: AT&T swaps `fsub` and
/// `fsubr` (and `fdiv`, `fdivr`) where the destination is not `st(0)`.
pub fn llc_text(b: &mut Batch) {
    let sp = |d: i32| Mem::base(Reg::Rsp, d);
    let rdi = Mem::base(Reg::Rdi, 0);
    att(b, "fld_m", fld_m(X87Size::T, sp(64)), None, "fldt 64(%rsp)");
    att(b, "fld_st", fld_st(St::St0), None, "fld %st(0)");
    att(b, "fstp_m", fstp_m(X87Size::T, sp(44)), None, "fstpt 44(%rsp)");
    att(b, "fstp_m", fstp_m(X87Size::T, Mem::base(Reg::Rax, 16)), None, "fstpt 16(%rax)");
    att(b, "faddp", faddp(St::St1), None, "faddp %st, %st(1)");
    att(b, "fstp_m", fstp_m(X87Size::D, sp(-8)), None, "fstpl -8(%rsp)");
    att(b, "fnstcw_m", fnstcw_m(sp(-10)), None, "fnstcw -10(%rsp)");
    att(b, "fldcw_m", fldcw_m(sp(-12)), None, "fldcw -12(%rsp)");
    att(b, "fistp_m", try_fistp_m(Size::Q, sp(-8)).unwrap(), None, "fistpll -8(%rsp)");
    att(b, "fld_m", fld_m(X87Size::S, Mem::rip(0)), None, "flds 0(%rip)");
    att(b, "fucomi", fucomi(St::St1), None, "fucomi %st(1), %st");
    att(b, "fldz", fldz(), None, "fldz");
    let fcmovbe = try_fcmov(Cond::Be, St::St1).unwrap();
    att(b, "fcmov", fcmovbe, Some("fcmovbe"), "fcmovbe %st(1), %st");
    att(b, "fstp_st", fstp_st(St::St1), None, "fstp %st(1)");
    // `x - c` with `x` in st(1), `c` in st(0): st(1) = st(1) - st(0).
    att(b, "fsubp", fsubp(St::St1), None, "fsubrp %st, %st(1)");
    att(b, "fucomip", fucomip(St::St1), None, "fucompi %st(1), %st");
    att(b, "fstp_st", fstp_st(St::St0), None, "fstp %st(0)");
    // The wider sample.
    att(b, "fsub_0i", fsub_0i(St::St1), None, "fsub %st(1), %st");
    att(b, "fmul_0i", fmul_0i(St::St1), None, "fmul %st(1), %st");
    att(b, "fdiv_0i", fdiv_0i(St::St1), None, "fdiv %st(1), %st");
    att(b, "fchs", fchs(), None, "fchs");
    att(b, "fabs", fabs(), None, "fabs");
    att(b, "fsqrt", fsqrt(), None, "fsqrt");
    att(b, "fxch", fxch(St::St2), None, "fxch %st(2)");
    for (cond, mn, i) in [(Cond::B, "fcmovb", 1), (Cond::Ne, "fcmovne", 2), (Cond::P, "fcmovu", 2)]
    {
        att(b, "fcmov", try_fcmov(cond, st(i)).unwrap(), Some(mn), &format!("{mn} %st({i}), %st"));
    }
    att(b, "fild_m", try_fild_m(Size::W, sp(-38)).unwrap(), None, "filds -38(%rsp)");
    att(b, "fild_m", try_fild_m(Size::D, sp(-36)).unwrap(), None, "fildl -36(%rsp)");
    att(b, "fild_m", try_fild_m(Size::Q, sp(-24)).unwrap(), None, "fildll -24(%rsp)");
    let table = Mem::index(Reg::Rax.index().unwrap(), Scale::S4, 0).with_abs32().unwrap();
    att(b, "fld_m", fld_m(X87Size::S, table), None, "{disp32} flds 0(,%rax,4)");
    att(b, "fld_m", fld_m(X87Size::D, sp(-64)), None, "fldl -64(%rsp)");
    att(b, "fst_m", try_fst_m(X87Size::S, rdi).unwrap(), None, "fsts (%rdi)");
    att(b, "fist_m", try_fist_m(Size::W, sp(-2)).unwrap(), None, "fists -2(%rsp)");
    att(b, "fist_m", try_fist_m(Size::D, sp(-8)).unwrap(), None, "fistl -8(%rsp)");
    att(b, "fld1", fld1(), None, "fld1");
    att(b, "fstp_m", fstp_m(X87Size::T, rdi), None, "fstpt (%rdi)");
    // The AT&T spellings of the forms with st(i) the destination.
    att(b, "fsub_i0", fsub_i0(St::St3), None, "fsubr %st, %st(3)");
    att(b, "fsubr_i0", fsubr_i0(St::St3), None, "fsub %st, %st(3)");
    att(b, "fsubrp", fsubrp(St::St1), None, "fsubp %st, %st(1)");
    att(b, "fdiv_i0", fdiv_i0(St::St3), None, "fdivr %st, %st(3)");
    att(b, "fdivr_i0", fdivr_i0(St::St3), None, "fdiv %st, %st(3)");
    att(b, "fdivp", fdivp(St::St1), None, "fdivrp %st, %st(1)");
    att(b, "fdivrp", fdivrp(St::St1), None, "fdivp %st, %st(1)");
}

/// A RIP-relative operand's site is the `disp32`, the last four bytes
/// (no immediate follows in any x87 instruction).
#[test]
fn rip_relative_x87_operands_have_their_site_last() {
    for inst in [
        fld_m(X87Size::S, Mem::rip(0)),
        fld_m(X87Size::T, Mem::rip(0)),
        fstp_m(X87Size::D, Mem::rip(0)),
        fadd_m(FpSize::D, Mem::rip(0)),
        fldcw_m(Mem::rip(0)),
        try_fild_m(Size::Q, Mem::rip(0)).unwrap(),
    ] {
        let site = inst.site().expect("RIP-relative is a site");
        assert_eq!(inst.len(), 6, "{:02x?}", inst.bytes());
        assert_eq!((site.offset, site.fixup, site.addend), (2, Fixup::Rel32, -4));
    }
    // A REX.B base adds one byte before the escape; the site moves with it.
    let inst = fld_m(X87Size::T, Mem::base(Reg::R13, 0));
    assert_eq!(inst.bytes(), [0x41, 0xdb, 0x6d, 0x00]);
    assert!(fld_m(X87Size::T, Mem::base(Reg::R13, 0)).has_rex());
    assert!(!fld_m(X87Size::T, Mem::base(Reg::Rbp, 0)).has_rex());
}

/// The stack registers are numbered 0 to 7; `St::new` refuses 8 and more.
#[test]
fn stack_registers_are_numbered_0_to_7() {
    for n in 0..8 {
        assert_eq!(st(n).code(), n);
    }
    assert_eq!(St::new(8), None);
    assert_eq!(St::new(255), None);
}
