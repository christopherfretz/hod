//! `spec/eh.md` §7.1: `llc` is the oracle. Each case is LLVM text that
//! `llc` (LLVM 19 and 22, `-O0`, PIC) compiles; its `.eh_frame` is read
//! back with `llvm-dwarfdump --eh-frame` and its relocations, turned into
//! `hod_eh`'s input, encoded again, and spliced into the object in place of
//! llc's. The two must decode to the same CIEs and FDEs (augmentation,
//! encodings, rules at every code offset), their relocations must mean the
//! same, and where llc used no `DW_CFA_remember_state` (which hod's rows
//! restate instead, §4.3) the section and its relocations must be
//! byte-identical.

#![allow(clippy::too_many_lines, clippy::format_push_string)]

mod common;

use common::{compile, llc, round_trip};
use hod_obj::Arch;
use hod_testutil::Scratch;

const PERSONALITY: &str = r"
declare i32 @__gxx_personality_v0(...)
declare i32 @__gcc_personality_v0(...)
declare void @may_throw()
declare ptr @__cxa_begin_catch(ptr)
declare void @__cxa_end_catch()
";

/// The cases: a name and the module, without a target triple; `{CLOBBER}`
/// stands for a target's callee-saved registers.
fn cases() -> Vec<(&'static str, String)> {
    let mut cases = vec![
        (
            "leaf_and_non_leaf",
            r"
declare void @ext(ptr)
define i32 @leaf(i32 %a) uwtable { %b = add i32 %a, 1
  ret i32 %b }
define void @non_leaf() uwtable { call void @ext(ptr null)
  ret void }
define void @non_leaf_sync() { call void @ext(ptr null)
  ret void }
define void @no_table() nounwind uwtable { ret void }
"
            .to_string(),
        ),
        (
            "callee_saved",
            r#"
declare void @ext(ptr)
define void @saves() uwtable {
  call void asm sideeffect "", "{CLOBBER}"()
  call void @ext(ptr null)
  ret void
}
define void @saves_sync() {
  call void asm sideeffect "", "{CLOBBER}"()
  call void @ext(ptr null)
  ret void
}
"#
            .to_string(),
        ),
        (
            "frame_pointer",
            r#"
declare void @ext(ptr)
define void @fp_all() uwtable "frame-pointer"="all" { call void @ext(ptr null)
  ret void }
define void @fp_leaf() uwtable "frame-pointer"="all" { %a = alloca i32
  store volatile i32 1, ptr %a
  ret void }
define void @fp_none() uwtable "frame-pointer"="none" { call void @ext(ptr null)
  ret void }
"#
            .to_string(),
        ),
        (
            "large_frames",
            r#"
declare void @ext(ptr)
define void @big_leaf() uwtable "frame-pointer"="none" { %a = alloca [70000 x i8]
  store volatile i8 1, ptr %a
  ret void }
define void @big() uwtable { %a = alloca [70000 x i8]
  call void @ext(ptr %a)
  ret void }
define void @huge() uwtable "frame-pointer"="all" { %a = alloca [20000000 x i8]
  call void @ext(ptr %a)
  ret void }
"#
            .to_string(),
        ),
        (
            "dynamic_alloca",
            r"
declare void @ext(ptr)
define void @dyn(i64 %n) uwtable { %a = alloca i8, i64 %n
  call void @ext(ptr %a)
  ret void }
define void @dyn_two(i64 %n) uwtable { %a = alloca i8, i64 %n, align 64
  call void @ext(ptr %a)
  %b = alloca i32, i64 %n
  call void @ext(ptr %b)
  ret void }
"
            .to_string(),
        ),
        (
            "personality_shared_cie",
            format!(
                r"{PERSONALITY}
define void @plain_first() uwtable {{ call void @may_throw()
  ret void }}
define i32 @catcher() uwtable personality ptr @__gxx_personality_v0 {{
entry:
  invoke void @may_throw() to label %ok unwind label %lp
ok:
  ret i32 0
lp:
  %x = landingpad {{ ptr, i32 }} catch ptr null
  %e = extractvalue {{ ptr, i32 }} %x, 0
  %c = call ptr @__cxa_begin_catch(ptr %e)
  call void @__cxa_end_catch()
  ret i32 1
}}
define void @between() uwtable {{ call void @may_throw()
  ret void }}
define void @cleanup() uwtable personality ptr @__gxx_personality_v0 {{
entry:
  invoke void @may_throw() to label %ok unwind label %lp
ok:
  ret void
lp:
  %x = landingpad {{ ptr, i32 }} cleanup
  resume {{ ptr, i32 }} %x
}}
define void @no_pads() uwtable personality ptr @__gxx_personality_v0 {{
  call void @may_throw()
  ret void
}}
define linkonce_odr void @in_comdat() uwtable comdat personality ptr @__gxx_personality_v0 {{
entry:
  invoke void @may_throw() to label %ok unwind label %lp
ok:
  ret void
lp:
  %x = landingpad {{ ptr, i32 }} cleanup
  resume {{ ptr, i32 }} %x
}}
$in_comdat = comdat any
"
            ),
        ),
        (
            "two_personalities",
            format!(
                r"{PERSONALITY}
define void @c_cleanup() uwtable personality ptr @__gcc_personality_v0 {{
entry:
  invoke void @may_throw() to label %ok unwind label %lp
ok:
  ret void
lp:
  %x = landingpad {{ ptr, i32 }} cleanup
  resume {{ ptr, i32 }} %x
}}
define void @cxx_cleanup() uwtable personality ptr @__gxx_personality_v0 {{
entry:
  invoke void @may_throw() to label %ok unwind label %lp
ok:
  ret void
lp:
  %x = landingpad {{ ptr, i32 }} cleanup
  resume {{ ptr, i32 }} %x
}}
define void @plain() uwtable {{ call void @may_throw()
  ret void }}
define void @c_again() uwtable personality ptr @__gcc_personality_v0 {{
entry:
  invoke void @may_throw() to label %ok unwind label %lp
ok:
  ret void
lp:
  %x = landingpad {{ ptr, i32 }} cleanup
  resume {{ ptr, i32 }} %x
}}
"
            ),
        ),
    ];
    // Bodies that put the epilogue's rows far from the prologue's, so that
    // llc picks each advance opcode.
    let mut long = String::from("declare void @ext(ptr)\n");
    for (name, stores) in [("adv_loc", 2), ("adv_loc1", 20), ("adv_loc2", 400), ("adv_loc4", 12000)]
    {
        long.push_str(&format!("define void @{name}(ptr %p) uwtable {{\n"));
        for i in 0..stores {
            long.push_str(&format!("  store volatile i32 {i}, ptr %p\n"));
        }
        long.push_str("  call void @ext(ptr %p)\n  ret void\n}\n");
    }
    cases.push(("advance_opcodes", long));
    // Many functions sharing one CIE.
    let mut many = String::from("declare void @ext(ptr)\n");
    for i in 0..60 {
        let attrs = if i % 3 == 0 { "uwtable" } else { "" };
        many.push_str(&format!(
            "define void @f{i}(ptr %p) {attrs} {{\n  call void @ext(ptr %p)\n  ret void\n}}\n"
        ));
    }
    cases.push(("many_functions", many));
    cases
}

fn targets() -> [(Arch, &'static str, &'static str); 2] {
    [
        (Arch::AArch64, "aarch64-unknown-linux-gnu", "~{x19},~{x20},~{x28},~{d8},~{d15}"),
        (Arch::X86_64, "x86_64-unknown-linux-gnu", "~{rbx},~{r12},~{r15}"),
    ]
}

fn oracle(major: u32) {
    let Some(llc) = llc(major) else { return };
    let scratch = Scratch::new(&format!("eh-llc{major}"));
    let mut summary = Vec::new();
    for (arch, triple, clobber) in targets() {
        for (name, ir) in cases() {
            let ir = ir.replace("{CLOBBER}", clobber);
            let label = format!("{name}-{triple}-{major}");
            let object = compile(&llc, &scratch, &label, &ir, triple);
            for reverse in [false, true] {
                let Some(rt) = round_trip(&scratch, &label, arch, &ir, &object, reverse) else {
                    return;
                };
                let expect: &[&str] = match (name, arch) {
                    ("advance_opcodes", _) => &[
                        "DW_CFA_advance_loc",
                        "DW_CFA_advance_loc1",
                        "DW_CFA_advance_loc2",
                        "DW_CFA_advance_loc4",
                    ],
                    ("callee_saved", Arch::AArch64) => {
                        &["DW_CFA_offset_extended", "DW_CFA_restore_extended"]
                    }
                    ("frame_pointer", Arch::X86_64) => &["DW_CFA_def_cfa_register"],
                    _ => &[],
                };
                for op in expect {
                    assert!(rt.ops.contains(*op), "{label}: the case exercises {op}: {:?}", rt.ops);
                }
                if !reverse {
                    summary.push(format!(
                        "{label}: {} CIE, {} FDE, {}",
                        rt.cies,
                        rt.fdes,
                        if rt.identical {
                            "byte-identical"
                        } else if rt.used_state {
                            "rule-equal (llc remembers state)"
                        } else {
                            unreachable!("asserted byte-identical")
                        }
                    ));
                }
            }
        }
    }
    eprintln!("{}", summary.join("\n"));
}

/// §7.1 with LLVM 19.
#[test]
fn llc19_eh_frame_round_trips() {
    oracle(19);
}

/// §7.1 with LLVM 22.
#[test]
fn llc22_eh_frame_round_trips() {
    oracle(22);
}

/// The shape of a personality slot (§2.2) as an object file shows it: the
/// section's type, flags and alignment, the slot symbol's binding, type,
/// visibility and size, its relocation, and the COMDAT group it signs.
fn slot_shape(object: &[u8]) -> Vec<String> {
    let elf = common::Elf::parse(object.to_vec());
    let name = "DW.ref.__gxx_personality_v0";
    let index = elf.section(&format!(".data.{name}")).expect("the slot's section");
    let s = &elf.sections[index];
    let symbol = elf.symbols.iter().position(|y| y.name == name).expect("the slot symbol");
    let y = &elf.symbols[symbol];
    let relas = elf.relas(elf.rela_of(index).expect("the slot's relocation"));
    let target = &elf.symbols[relas[0].sym as usize].name;
    let group = elf
        .sections
        .iter()
        .enumerate()
        .find(|(_, g)| g.ty == 17 && g.info as usize == symbol)
        .map(|(i, _)| elf.bytes(i).to_vec())
        .expect("a group signed by the slot");
    let words: Vec<u32> =
        group.chunks_exact(4).map(|w| u32::from_le_bytes(w.try_into().unwrap())).collect();
    vec![
        format!("section type {} flags {:#x} align {} size {}", s.ty, s.flags, s.align, s.size),
        format!("symbol info {:#x} other {:#x} value {}", y.info, y.other, y.value),
        format!(
            "relocations {} type {} to {target} + {}",
            relas.len(),
            relas[0].ty,
            relas[0].addend
        ),
        format!("group flag {} holds the slot {}", words[0], words[1..].contains(&(index as u32))),
    ]
}

/// §2.2: `personality_ref` makes the slot as llc does.
#[test]
fn personality_slot_as_llc_makes_it() {
    let Some(llc) = llc(22) else { return };
    let scratch = Scratch::new("eh-slot");
    let (_, ir) = cases().into_iter().find(|c| c.0 == "personality_shared_cie").unwrap();
    let theirs = compile(&llc, &scratch, "slot", &ir, "aarch64-unknown-linux-gnu");
    let mut obj = hod_obj::Object::new(Arch::AArch64);
    let name = obj.add_string(b"__gxx_personality_v0").unwrap();
    let routine = obj.add_symbol(hod_obj::Symbol::undefined(name));
    hod_eh::personality_ref(&mut obj, routine).unwrap();
    let ours = hod_obj::elf::to_vec(&obj).unwrap();
    assert_eq!(slot_shape(&ours), slot_shape(&theirs));
}
