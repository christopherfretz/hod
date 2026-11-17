//! The forms that exist only in 32-bit mode, `spec/mc-x64.md` "32-bit
//! mode" (WP-285): what [`Mode::X86`] rewrites (the absolute `disp32`
//! without `SIB`, the `moffs` moves, `inc`/`dec` `40+r`/`48+r`, `xchg eax,
//! eax`, `ud1` without `67`), the 32-bit default operand size of the stack
//! and indirect branches, and the absolute fields an `i386` relocation
//! fills. Checked against `llvm-mc -triple=i686` 19 and 22; the
//! translated 64-bit sweeps are `main.rs`'s `i386::` tests.

use std::collections::BTreeMap;
use std::fmt::Write as _;

use hod_mc_x64::*;
use hod_testutil::run;

use crate::harness::{Batch, Tools};
use crate::text::{M, ptr, r, reg};

const SIZES: [Size; 3] = [Size::B, Size::W, Size::D];
const DISPS: [i32; 4] = [0, 16, -16, 0x1234_5678];

fn x86(inst: Inst) -> Inst {
    Mode::X86.admit(inst).expect("32-bit mode admits it")
}

/// The 32-bit-only forms, every one admitted.
fn forms(b: &mut Batch) {
    // inc/dec r16/r32: 40+r, 48+r. The byte forms keep FE.
    for n in 0..8 {
        for size in SIZES {
            emit!(b, inc_r(size, r(n)), "inc {}", reg(size, n));
            emit!(b, dec_r(size, r(n)), "dec {}", reg(size, n));
        }
    }
    emit!(b, xchg_rr(Size::D, Reg::Rax, Reg::Rax), "xchg eax, eax");
    emit!(b, xchg_rr(Size::W, Reg::Rax, Reg::Rax), "xchg ax, ax");
    // Absolute addresses: ModRM r/m=101, no SIB; the moffs moves for
    // al/ax/eax, with and without a segment.
    for disp in DISPS {
        for seg in [None, Some(Seg::Fs), Some(Seg::Gs)] {
            let m = M { seg, ..M::abs(disp) };
            for size in SIZES {
                for n in 0..8 {
                    let sized = format!("{} {}", ptr(size), m.text());
                    let rg = reg(size, n);
                    emit!(b, mov_rm(size, r(n), m.mem()), "mov {rg}, {sized}");
                    emit!(b, mov_mr(size, m.mem(), r(n)), "mov {sized}, {rg}");
                    emit!(b, add_rm(size, r(n), m.mem()), "add {rg}, {sized}");
                }
                let sized = format!("{} {}", ptr(size), m.text());
                emit!(b, try_cmp_mi(size, m.mem(), 100), "cmp {sized}, 100");
                emit_as!(b, "lock add", lock(add_mi(size, m.mem(), 1)), "lock add {sized}, 1");
                emit!(b, inc_m(size, m.mem()), "inc {sized}");
            }
            let dword = format!("dword ptr {}", m.text());
            emit!(b, push_m(m.mem()), "push {dword}");
            emit!(b, pop_m(m.mem()), "pop {dword}");
            emit!(b, call_m(m.mem()), "call {dword}");
            emit!(b, jmp_m(m.mem()), "jmp {dword}");
            emit!(b, try_lea(Size::D, Reg::Rcx, m.mem()), "lea ecx, {}", m.text());
            emit_as!(b, "movss", movs_rm(FpSize::S, Xmm::ALL[3], m.mem()), "movss xmm3, {dword}");
            emit!(b, fld_m(X87Size::D, m.mem()), "fld qword ptr {}", m.text());
            emit!(
                b,
                try_movzx_rm(Size::D, Reg::Rdx, Size::B, m.mem()),
                "movzx edx, byte ptr {}",
                m.text()
            );
        }
    }
    // The stack and the indirect branches: 32-bit operands.
    for n in 0..8 {
        let e = reg(Size::D, n);
        emit!(b, push_r(r(n)), "push {e}");
        emit!(b, pop_r(r(n)), "pop {e}");
        emit!(b, call_r(r(n)), "call {e}");
        emit!(b, jmp_r(r(n)), "jmp {e}");
        let m = M::base(n, 8);
        emit!(b, push_m(m.mem()), "push dword ptr {}", m.text());
        emit!(b, call_m(m.mem()), "call dword ptr {}", m.text());
    }
    emit!(b, push_i(-1), "push -1");
    emit!(b, push_i32(0x1234_5678), "push 0x12345678");
    emit!(b, pushfq(), "pushfd");
    emit!(b, popfq(), "popfd");
    emit!(b, leave(), "leave");
    emit!(b, endbr32(), "endbr32");
    emit!(b, ret(), "ret");
    emit!(b, ret_imm(4), "ret 4");
    // ud1 without the 67 of 64-bit mode's 32-bit addressing.
    for disp in [0u8, 5, 127, 128, 255] {
        let at = if disp == 0 { String::new() } else { format!(" + {disp}") };
        emit!(b, ud1_eax(disp), "ud1 eax, dword ptr [eax{at}]");
    }
    // Absolute fields (`with_abs32`) of every 32-bit-addressable form: the
    // disp32 an R_386_32 or R_386_GOTOFF fills.
    for m in crate::text::all_mems().into_iter().filter(|m| !m.is_rip()) {
        let field = m.mem().with_abs32().expect("only RIP-relative operands refuse");
        let text = crate::text::to_i386(&m.text());
        for n in [0, 3, 7] {
            emit!(
                b,
                mov_rm(Size::D, r(n), field),
                "{{disp32}} mov {}, dword ptr {text}",
                reg(Size::D, n)
            );
        }
    }
}

#[test]
fn forms_match_llvm() {
    let mut batch = Batch::for_mode(Mode::X86);
    forms(&mut batch);
    batch.check("i386-forms");
}

/// The relocations of an ELF32 object: offset and type, without the
/// `R_386_` prefix (`SHT_REL`: the addend is in the section bytes).
fn relocations(tools: &Tools, obj: &std::path::Path) -> BTreeMap<u64, String> {
    let outcome = run(tools.objdump(), ["-r".as_ref(), obj.as_os_str()]);
    assert!(outcome.success(), "llvm-objdump -r failed: {}", outcome.stderr_text());
    outcome
        .stdout_text()
        .lines()
        .filter_map(|line| {
            let mut parts = line.split_whitespace();
            let offset = u64::from_str_radix(parts.next()?, 16).ok()?;
            let kind = parts.next()?.strip_prefix("R_386_")?.to_string();
            Some((offset, kind))
        })
        .collect()
}

/// `(instruction with a zero field, text against `foo`, the relocation
/// llvm-mc writes)`: the references an `i386` back end writes.
fn references() -> Vec<(Inst, &'static str, &'static str)> {
    let abs = Mem::abs(0).with_abs32().expect("absolute");
    let ebx = Mem::base(Reg::Rbx, 0).with_abs32().expect("absolute");
    let gs = abs.with_seg(Seg::Gs);
    vec![
        (call(0), "call foo", "PC32"),
        (call(0), "call foo@PLT", "PLT32"),
        (jmp_near(0), "{disp32} jmp foo", "PC32"),
        (jcc_near(Cond::Ne, 0), "{disp32} jne foo", "PC32"),
        (mov_rm(Size::D, Reg::Rax, abs), "mov eax, dword ptr [foo]", "32"),
        (mov_rm(Size::D, Reg::Rcx, abs), "mov ecx, dword ptr [foo]", "32"),
        (mov_mr(Size::B, abs, Reg::Rax), "mov byte ptr [foo], al", "32"),
        (mov_mi(Size::D, abs, 7), "mov dword ptr [foo], 7", "32"),
        (lea(Size::D, Reg::Rax, abs), "lea eax, [foo]", "32"),
        (call_m(abs), "call dword ptr [foo]", "32"),
        (mov_rm(Size::D, Reg::Rax, ebx), "mov eax, dword ptr [ebx + foo@GOTOFF]", "GOTOFF"),
        (mov_rm(Size::D, Reg::Rcx, ebx), "mov ecx, dword ptr [ebx + foo@GOT]", "GOT32X"),
        (mov_rm(Size::D, Reg::Rax, gs), "mov eax, dword ptr gs:[foo@NTPOFF]", "TLS_LE"),
        (mov_rm(Size::D, Reg::Rdx, gs), "mov edx, dword ptr gs:[foo@NTPOFF]", "TLS_LE"),
        (mov_ri32(Reg::Rax, 0), "mov eax, offset foo", "32"),
        (push_i32(0), "att: pushl $foo", "32"),
        (movs_rm(FpSize::D, Xmm::ALL[1], abs), "movsd xmm1, qword ptr [foo]", "32"),
        (fld_m(X87Size::D, abs), "fld qword ptr [foo]", "32"),
    ]
}

/// The site [`Mode::X86`] reports is where `llvm-mc -triple=i686 -filetype=obj`
/// puts the relocation, and its addend is the implicit addend llvm-mc
/// writes into the field (`-4` from the end for `PC32`/`PLT32`, 0 for
/// the absolute fields). The relocation type is the back end's choice;
/// the encoder's [`Fixup`] says only `Rel32` or `Abs32`/`Abs32U`.
#[test]
fn relocation_sites_match_llvm() {
    let cases: Vec<(Inst, &str, &str)> =
        references().into_iter().map(|(inst, asm, kind)| (x86(inst), asm, kind)).collect();
    for tools in Tools::all_in(Mode::X86) {
        let mut source = String::new();
        for (i, (_, asm, _)) in cases.iter().enumerate() {
            // `push offset foo` is `pushw` to llvm-mc's Intel syntax at
            // i686: the AT&T `pushl $foo` says what is meant.
            match asm.strip_prefix("att: ") {
                Some(att) => {
                    writeln!(source, ".att_syntax\nh{i}: {att}\n.intel_syntax noprefix").unwrap();
                }
                None => writeln!(source, "h{i}: {asm}").unwrap(),
            }
        }
        let obj = tools.assemble_object("relocations", &source).expect("llvm-mc assembles it");
        let text = tools.text_bytes(&obj);
        let symbols = tools.symbols(&obj);
        let relocs = relocations(&tools, &obj);
        assert_eq!(relocs.len(), cases.len(), "one relocation per instruction");
        for (i, (inst, asm, kind)) in cases.iter().enumerate() {
            let start = symbols[&format!("h{i}")] as usize;
            let site = inst.site().expect("a referencing instruction has a site");
            let llvm = &text[start..start + inst.len()];
            // The field holds the implicit addend, as an `SHT_REL` object
            // must.
            let written = inst.with_disp(site.addend).expect("the addend fits");
            assert_eq!(written.bytes(), llvm, "LLVM {}: `{asm}`: bytes", tools.major);
            let at = (start + site.offset) as u64;
            assert_eq!(
                relocs.get(&at).map(String::as_str),
                Some(*kind),
                "LLVM {}: `{asm}`",
                tools.major
            );
            let implicit = site.fixup.extract(llvm, site.offset).expect("the field");
            assert_eq!(implicit, site.addend, "LLVM {}: `{asm}`: implicit addend", tools.major);
            let expected = if site.fixup.is_pc_relative() { Fixup::Rel32 } else { site.fixup };
            assert!(matches!(expected, Fixup::Rel32 | Fixup::Abs32 | Fixup::Abs32U), "{asm}");
        }
        println!("l1 i386 relocations (LLVM {}): {} sites agree", tools.major, cases.len());
    }
}
