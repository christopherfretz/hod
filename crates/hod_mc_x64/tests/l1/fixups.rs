//! Fixups and relocations, `spec/mc-x64.md` "Fixups and relocations":
//! patch and extract are inverses and touch only their field, every site
//! is where the spec says (tool-free); the sites and addends equal the
//! relocations LLVM writes for the same instructions against a symbol, and
//! a laid-out function patched through its sites equals LLVM's assembly.

use std::collections::BTreeMap;
use std::fmt::Write as _;

use hod_mc_x64::*;
use hod_testutil::run;

use crate::harness::{Batch, Tools};
use crate::text::{Rng, r};

#[test]
fn ranges_are_the_architectures() {
    assert_eq!(Fixup::ALL, [Fixup::Rel8, Fixup::Rel32]);
    assert_eq!((Fixup::Rel8.size(), Fixup::Rel8.min(), Fixup::Rel8.max()), (1, -128, 127));
    assert_eq!(
        (Fixup::Rel32.size(), Fixup::Rel32.min(), Fixup::Rel32.max()),
        (4, -(1 << 31), (1 << 31) - 1)
    );
}

fn displacements(kind: Fixup, rng: &mut Rng) -> Vec<i64> {
    match kind {
        Fixup::Rel8 => (-128..=127).collect(),
        Fixup::Rel32 => {
            let mut out = vec![0, 1, -1, 127, -128, 128, -129, i32::MAX.into(), i32::MIN.into()];
            out.extend((0..2000).map(|_| i64::from(rng.next() as i32)));
            out
        }
    }
}

#[test]
fn patch_and_extract_are_inverses() {
    let mut rng = Rng::new(0xf1);
    for kind in Fixup::ALL {
        for disp in displacements(kind, &mut rng) {
            let mut code: Vec<u8> = (0..12).map(|_| rng.next() as u8).collect();
            let before = code.clone();
            let at = (rng.next() % 8) as usize;
            assert!(kind.patch(&mut code, at, disp), "{kind:?} {disp}");
            assert_eq!(kind.extract(&code, at), Some(disp), "{kind:?} {disp}");
            for (i, (new, old)) in code.iter().zip(&before).enumerate() {
                if i < at || i >= at + kind.size() {
                    assert_eq!(new, old, "{kind:?} changed byte {i} outside its field");
                }
            }
            // The field is little-endian two's complement.
            let field = &code[at..at + kind.size()];
            let mut expected = disp.to_le_bytes().to_vec();
            expected.truncate(kind.size());
            assert_eq!(field, expected.as_slice());
        }
        for disp in [kind.min() - 1, kind.max() + 1, i64::MIN, i64::MAX] {
            let mut code = vec![0xaa; 8];
            assert!(!kind.patch(&mut code, 0, disp), "{kind:?} {disp}");
            assert_eq!(code, vec![0xaa; 8], "a refused patch left the code alone");
        }
        // A field that does not lie inside the code.
        let mut code = vec![0; 4];
        let at = 5 - kind.size();
        assert!(!kind.patch(&mut code, at, 0));
        assert_eq!(kind.extract(&code, at), None);
        assert!(!kind.patch(&mut code, usize::MAX, 0));
        assert_eq!(kind.extract(&code, usize::MAX), None);
    }
}

/// `(instruction with a zero field, its field offset, kind, bytes after the
/// field)`, stated independently of the crate.
fn sites() -> Vec<(Inst, usize, Fixup, usize)> {
    let rip = Mem::rip(0);
    vec![
        (jmp_short(0), 1, Fixup::Rel8, 0),
        (jcc_short(Cond::G, 0), 1, Fixup::Rel8, 0),
        (jmp_near(0), 1, Fixup::Rel32, 0),
        (jcc_near(Cond::E, 0), 2, Fixup::Rel32, 0),
        (call(0), 1, Fixup::Rel32, 0),
        (lea(Size::Q, Reg::Rax, rip), 3, Fixup::Rel32, 0),
        (mov_rm(Size::D, Reg::Rcx, rip), 2, Fixup::Rel32, 0),
        (mov_rm(Size::B, Reg::Rsi, rip), 3, Fixup::Rel32, 0),
        (mov_mi(Size::Q, rip, 5), 3, Fixup::Rel32, 4),
        (mov_mi(Size::W, rip, 5), 3, Fixup::Rel32, 2),
        (cmp_mi(Size::D, rip, 5), 2, Fixup::Rel32, 1),
        (cmp_mi(Size::B, rip, 5), 2, Fixup::Rel32, 1),
        (shld_mri(Size::Q, rip, Reg::R9, 3), 4, Fixup::Rel32, 1),
        (lock(add_mi(Size::W, rip, 300)), 4, Fixup::Rel32, 2),
        (mov_rm(Size::Q, Reg::Rax, rip.with_seg(Seg::Fs)), 4, Fixup::Rel32, 0),
        (call_m(rip), 2, Fixup::Rel32, 0),
        (popcnt_rm(Size::W, Reg::R10, rip), 6, Fixup::Rel32, 0),
    ]
}

#[test]
fn sites_are_where_the_fields_are() {
    for (inst, offset, fixup, after) in sites() {
        let site = inst.site().unwrap_or_else(|| panic!("{inst:02x?} has a site"));
        assert_eq!(site.offset, offset, "{:02x?}", inst.bytes());
        assert_eq!(site.fixup, fixup, "{:02x?}", inst.bytes());
        assert_eq!(inst.len(), offset + fixup.size() + after, "{:02x?}", inst.bytes());
        assert_eq!(site.addend, -((fixup.size() + after) as i64));
        assert_eq!(fixup.extract(inst.bytes(), offset), Some(0));
    }
    for inst in [
        ret(),
        mov_rm(Size::Q, Reg::Rax, Mem::abs(0)),
        lea(Size::Q, Reg::Rax, Mem::base(Reg::Rbp, 0x100)),
        jmp_r(Reg::Rax),
    ] {
        assert_eq!(inst.site(), None, "{:02x?}", inst.bytes());
        assert_eq!(inst.with_disp(0), None);
    }
}

#[test]
fn constructors_agree_with_patching() {
    let mut rng = Rng::new(0x5e);
    for kind in Fixup::ALL {
        for disp in displacements(kind, &mut rng) {
            let pairs: Vec<(Inst, Inst)> = match kind {
                Fixup::Rel8 => {
                    let d = disp as i8;
                    vec![
                        (jmp_short(d), jmp_short(0)),
                        (jcc_short(Cond::Le, d), jcc_short(Cond::Le, 0)),
                    ]
                }
                Fixup::Rel32 => {
                    let d = disp as i32;
                    vec![
                        (jmp_near(d), jmp_near(0)),
                        (jcc_near(Cond::B, d), jcc_near(Cond::B, 0)),
                        (call(d), call(0)),
                        (lea(Size::Q, Reg::R12, Mem::rip(d)), lea(Size::Q, Reg::R12, Mem::rip(0))),
                        (mov_mi(Size::D, Mem::rip(d), -7), mov_mi(Size::D, Mem::rip(0), -7)),
                    ]
                }
            };
            for (direct, zero) in pairs {
                assert_eq!(zero.with_disp(disp), Some(direct), "{kind:?} {disp}");
            }
        }
    }
    assert_eq!(jmp_short(0).with_disp(128), None);
    assert_eq!(call(0).with_disp(1 << 31), None);
}

/// The relocations of an object: offset, type name and addend.
fn relocations(tools: &Tools, obj: &std::path::Path) -> Vec<(u64, String, i64)> {
    let outcome = run(tools.objdump(), ["-r".as_ref(), obj.as_os_str()]);
    assert!(outcome.success(), "llvm-objdump -r failed: {}", outcome.stderr_text());
    outcome
        .stdout_text()
        .lines()
        .filter_map(|line| {
            let mut parts = line.split_whitespace();
            let offset = u64::from_str_radix(parts.next()?, 16).ok()?;
            let kind = parts.next()?.strip_prefix("R_X86_64_")?.to_string();
            let value = parts.next()?;
            let addend = match value.find(['+', '-']) {
                Some(at) => {
                    let magnitude =
                        i64::from_str_radix(value[at + 1..].trim_start_matches("0x"), 16).ok()?;
                    if value.as_bytes()[at] == b'-' { -magnitude } else { magnitude }
                }
                None => 0,
            };
            Some((offset, kind, addend))
        })
        .collect()
}

/// `(constructor, instruction with a zero field, text against `foo`, the
/// relocation LLVM writes)`: the small PIC code model's references
/// (`spec/mc-x64.md` "Fixups and relocations").
#[allow(clippy::too_many_lines, reason = "one row per reference")]
fn references() -> Vec<(&'static str, Inst, String, &'static str)> {
    let rip = Mem::rip(0);
    let mut out = vec![
        ("call", call(0), "call foo".to_string(), "PLT32"),
        ("jmp_near", jmp_near(0), "{disp32} jmp foo".to_string(), "PLT32"),
        ("jcc_near", jcc_near(Cond::Ne, 0), "{disp32} jne foo".to_string(), "PLT32"),
        ("mov_mi", mov_mi(Size::Q, rip, 5), "mov qword ptr [rip + foo], 5".to_string(), "PC32"),
        ("mov_mi", mov_mi(Size::W, rip, 5), "mov word ptr [rip + foo], 5".to_string(), "PC32"),
        ("cmp_mi", cmp_mi(Size::B, rip, 1), "cmp byte ptr [rip + foo], 1".to_string(), "PC32"),
        (
            "cmp_mi",
            cmp_mi(Size::D, rip, 1000),
            "cmp dword ptr [rip + foo], 1000".to_string(),
            "PC32",
        ),
        (
            "shld_mri",
            shld_mri(Size::Q, rip, Reg::R9, 3),
            "shld qword ptr [rip + foo], r9, 3".to_string(),
            "PC32",
        ),
        (
            "lock",
            lock(add_mi(Size::W, rip, 300)),
            "lock add word ptr [rip + foo], 300".to_string(),
            "PC32",
        ),
        (
            "mov_rm",
            mov_rm(Size::D, Reg::Rax, rip.with_seg(Seg::Fs)),
            "mov eax, dword ptr fs:[rip + foo]".to_string(),
            "PC32",
        ),
        ("call_m", call_m(rip), "call qword ptr [rip + foo@GOTPCREL]".to_string(), "GOTPCRELX"),
        ("jmp_m", jmp_m(rip), "jmp qword ptr [rip + foo@GOTPCREL]".to_string(), "GOTPCRELX"),
        (
            "mov_rm",
            mov_rm(Size::Q, Reg::Rax, rip),
            "mov rax, qword ptr [rip + foo@GOTPCREL]".to_string(),
            "REX_GOTPCRELX",
        ),
        (
            "mov_rm",
            mov_rm(Size::D, Reg::Rcx, rip),
            "mov ecx, dword ptr [rip + foo@GOTPCREL]".to_string(),
            "GOTPCRELX",
        ),
        (
            "mov_rm",
            mov_rm(Size::D, Reg::R8, rip),
            "mov r8d, dword ptr [rip + foo@GOTPCREL]".to_string(),
            "REX_GOTPCRELX",
        ),
        (
            "add_rm",
            add_rm(Size::D, Reg::Rax, rip),
            "add eax, dword ptr [rip + foo@GOTPCREL]".to_string(),
            "GOTPCRELX",
        ),
        (
            "cmp_rm",
            cmp_rm(Size::Q, Reg::Rdi, rip),
            "cmp rdi, qword ptr [rip + foo@GOTPCREL]".to_string(),
            "REX_GOTPCRELX",
        ),
        (
            "test_mr",
            test_mr(Size::Q, rip, Reg::Rax),
            "test qword ptr [rip + foo@GOTPCREL], rax".to_string(),
            "REX_GOTPCRELX",
        ),
        (
            "mov_rm",
            mov_rm(Size::Q, Reg::R11, rip),
            "mov r11, qword ptr [rip + foo@GOTTPOFF]".to_string(),
            "GOTTPOFF",
        ),
        (
            "add_rm",
            add_rm(Size::Q, Reg::Rax, rip),
            "add rax, qword ptr [rip + foo@GOTTPOFF]".to_string(),
            "GOTTPOFF",
        ),
    ];
    for n in 0..16 {
        let text = |size| crate::text::reg(size, n);
        out.push((
            "lea",
            lea(Size::Q, r(n), rip),
            format!("lea {}, [rip + foo]", text(Size::Q)),
            "PC32",
        ));
        out.push((
            "mov_rm",
            mov_rm(Size::B, r(n), rip),
            format!("mov {}, byte ptr [rip + foo]", text(Size::B)),
            "PC32",
        ));
        out.push((
            "mov_rm",
            mov_rm(Size::Q, r(n), rip),
            format!("mov {}, qword ptr [rip + foo@GOTPCREL]", text(Size::Q)),
            "REX_GOTPCRELX",
        ));
    }
    out
}

/// The `R_X86_64_*` type the spec's rule gives for a GOT load through
/// `inst`.
fn got_load(inst: Inst) -> &'static str {
    if inst.has_rex() { "REX_GOTPCRELX" } else { "GOTPCRELX" }
}

#[test]
fn relocation_sites_match_llvm() {
    let cases = references();
    for (name, inst, asm, kind) in &cases {
        if kind.ends_with("GOTPCRELX") {
            assert_eq!(got_load(*inst), *kind, "{name}: `{asm}`: has_rex");
        }
    }
    for tools in Tools::all() {
        let mut batch = Batch::default();
        let mut source = String::new();
        for (i, (name, inst, asm, _)) in cases.iter().enumerate() {
            let family = match *name {
                "jcc_near" => Some("jne".to_string()),
                "lock" => Some("lock add".to_string()),
                _ => None,
            };
            batch.push(name, *inst, family, asm.clone());
            writeln!(source, "h{i}: {asm}").unwrap();
        }
        let (mismatches, report) = batch.examine(&tools);
        assert_eq!(mismatches, 0, "LLVM {}: {report}", tools.major);
        let obj = tools.assemble_object("relocations", &source).expect("llvm-mc assembles it");
        let symbols = tools.symbols(&obj);
        let relocs: BTreeMap<u64, (String, i64)> =
            relocations(&tools, &obj).into_iter().map(|(o, k, a)| (o, (k, a))).collect();
        assert_eq!(relocs.len(), cases.len(), "one relocation per instruction");
        for (i, (name, inst, asm, kind)) in cases.iter().enumerate() {
            let site = inst.site().expect("a referencing instruction has a site");
            let at = symbols[&format!("h{i}")] + site.offset as u64;
            let found = relocs.get(&at);
            assert_eq!(
                found,
                Some(&((*kind).to_string(), site.addend)),
                "LLVM {}: {name}: `{asm}`: hod's site is offset {} addend {}",
                tools.major,
                site.offset,
                site.addend
            );
        }
        println!("l1 relocations (LLVM {}): {} sites agree", tools.major, cases.len());
    }
}

/// An instruction of the laid-out function, and the label it refers to.
type Item = (Inst, Option<usize>, String);

/// A function with forward and backward references of every kind, and
/// padding between them.
fn function() -> (Vec<Item>, Vec<usize>) {
    let rip = Mem::rip(0);
    // (instruction, target label, text); a `None` instruction places a
    // label.
    let items: Vec<(Option<Inst>, Option<usize>, &str)> = vec![
        (None, Some(0), ""),
        (Some(jmp_short(0)), Some(1), "jmp L1"),
        (Some(jcc_short(Cond::E, 0)), Some(0), "je L0"),
        (Some(nop(7)), None, ".nops 7, 7"),
        (Some(jmp_near(0)), Some(2), "{disp32} jmp L2"),
        (Some(call(0)), Some(0), "call L0"),
        (Some(lea(Size::Q, Reg::Rax, rip)), Some(2), "lea rax, [rip + L2]"),
        (Some(mov_mi(Size::D, rip, 0x1234_5678)), Some(1), "mov dword ptr [rip + L1], 305419896"),
        (Some(cmp_mi(Size::B, rip, 1)), Some(0), "cmp byte ptr [rip + L0], 1"),
        (None, Some(1), ""),
        (Some(jcc_near(Cond::L, 0)), Some(2), "{disp32} jl L2"),
        (Some(nop(15)), None, ".nops 15, 15"),
        (Some(nop(15)), None, ".nops 15, 15"),
        (Some(lock(add_mi(Size::Q, rip, 1))), Some(0), "lock add qword ptr [rip + L0], 1"),
        (
            Some(mov_rm(Size::Q, Reg::R13, rip.with_seg(Seg::Gs))),
            Some(1),
            "mov r13, qword ptr gs:[rip + L1]",
        ),
        (None, Some(2), ""),
        (Some(jcc_short(Cond::Ne, 0)), Some(1), "jne L1"),
        (Some(ret()), None, "ret"),
    ];
    let mut labels = vec![0; 3];
    let mut code = Vec::new();
    let mut offset = 0;
    for (inst, target, text) in items {
        match inst {
            None => labels[target.expect("a label")] = offset,
            Some(inst) => {
                offset += inst.len();
                code.push((inst, target, text.to_string()));
            }
        }
    }
    (code, labels)
}

#[test]
fn a_laid_out_function_matches_llvm() {
    let (items, labels) = function();
    let mut code = Vec::new();
    let mut patches = Vec::new();
    for (inst, target, _) in &items {
        if let Some(label) = target {
            let site = inst.site().expect("a referencing instruction has a site");
            patches.push((code.len(), site, *label));
        }
        code.extend_from_slice(inst.bytes());
    }
    for (start, site, label) in patches {
        let at = start + site.offset;
        // S + A - P, as a relocation would compute it.
        let disp = labels[label] as i64 + site.addend - at as i64;
        assert!(site.fixup.patch(&mut code, at, disp), "the target is in range");
    }
    let mut source = String::new();
    let mut placed = 0;
    let mut offset = 0;
    for (inst, _, text) in &items {
        while placed < labels.len() && labels[placed] == offset {
            writeln!(source, "L{placed}:").unwrap();
            placed += 1;
        }
        writeln!(source, "{text}").unwrap();
        offset += inst.len();
    }
    for tools in Tools::all() {
        let obj = tools.assemble_object("function", &source).expect("llvm-mc assembles it");
        assert_eq!(tools.text_bytes(&obj), code, "LLVM {}:\n{source}", tools.major);
    }
}
