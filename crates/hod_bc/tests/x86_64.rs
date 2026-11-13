//! What x86-64 modules bring to the readers and the printer (WP-138):
//! `x86_fp80` types, constants and initializers, the address spaces
//! 270..272, the x86 calling conventions, the `byval`/`inreg`/`nest`/
//! `preallocated`/`inalloca` attributes, the target attribute strings,
//! `llvm.x86.*` intrinsics, Intel-dialect inline assembly and the x86
//! module flags. For each LLVM major: `hod_ll` on the text and `hod_bc` on
//! `llvm-as`'s bitcode give the same module and bodies (`spec/bc.md` §9);
//! `hod_ir`'s printer writes text `llvm-as` takes, which reads back to the
//! same text (`spec/ir.md` §9). The constructs hod refuses are refused by
//! name on both routes.

mod common;

use std::fmt::Write as _;

use hod_ir::{Body, Dialect, Module};

const MAJORS: [u32; 2] = [19, 22];

const HEADER: &str = "target datalayout = \"e-m:e-p270:32:32-p271:32:32-p272:64:64-i64:64-i128:128-f80:128-n8:16:32:64-S128\"\n\
                      target triple = \"x86_64-unknown-linux-gnu\"\n";

/// Every x86-64 construct the front half takes.
const MODULE: &str = r#"
%S = type { i64, x86_fp80 }
%T = type { i8, i128 }
%struct.__va_list_tag = type { i32, i32, ptr, ptr }

@ld = global x86_fp80 0xK3FFF8000000000000000, align 16
@s = global %S { i64 1, x86_fp80 0xKC000C000000000000000 }, align 16
@arr = global [2 x x86_fp80] [x86_fp80 0xK7FFF8000000000000000, x86_fp80 0xK00000000000000000001], align 16
@t = global %T { i8 1, i128 2 }, align 16
@va = global [1 x %struct.__va_list_tag] zeroinitializer, align 16

define i32 @arith() #0 {
  %a = load x86_fp80, ptr @ld, align 16
  %b = fadd x86_fp80 %a, 0xK4000C000000000000000
  %c = fmul x86_fp80 %b, %a
  %d = fdiv x86_fp80 %c, 0xK4000C000000000000000
  %e = fsub x86_fp80 %d, %a
  %f = frem x86_fp80 %e, %a
  %n = fneg x86_fp80 %f
  %cmp = fcmp olt x86_fp80 %n, %a
  %i = fptosi x86_fp80 %d to i32
  %x = fpext double 1.5 to x86_fp80
  %g = fptrunc x86_fp80 %x to double
  %h = sitofp i64 -3 to x86_fp80
  %u = uitofp i64 7 to x86_fp80
  %fu = fptoui x86_fp80 %u to i64
  %fl = fptrunc x86_fp80 %h to float
  %w = bitcast x86_fp80 %h to i80
  store x86_fp80 %n, ptr @ld, align 16
  ret i32 %i
}

define x86_64_sysvcc i32 @sysv(ptr byval(%S) align 16 %p, i32 inreg %q, ptr nest %r) #0 {
  %c = call x86_64_sysvcc i32 @sysv(ptr byval(%S) align 16 %p, i32 inreg %q, ptr nest %r)
  ret i32 %c
}

declare win64cc void @w64()
declare x86_stdcallcc void @std()
declare x86_fastcallcc void @fast()
declare x86_thiscallcc void @this()
declare x86_vectorcallcc void @vec()
declare x86_regcallcc void @reg()
declare x86_intrcc void @intr(ptr byval(i64))
declare void @pre(ptr preallocated(i32))
declare void @ina(ptr inalloca(i32))

; Not one LLVM's auto-upgrade rewrites (`llvm.x86.sse2.psrl.dq` would be
; a `shufflevector` in llvm-as's bitcode and a call in hod_ll's reading).
declare void @llvm.x86.sse2.pause()

define void @spin() {
  call void @llvm.x86.sse2.pause()
  ret void
}

define i32 @asm() {
  call void asm sideeffect inteldialect "mov eax, eax", "~{eax},~{dirflag},~{fpsr},~{flags}"()
  %x = call i32 asm "movl $1, $0", "={ax},r,~{dirflag},~{fpsr},~{flags}"(i32 1)
  ret i32 %x
}

define ptr addrspace(270) @spaces(ptr addrspace(271) %p, ptr addrspace(272) %q) {
  ret ptr addrspace(270) null
}

attributes #0 = { noinline nounwind optnone uwtable "frame-pointer"="all" "target-cpu"="x86-64" "target-features"="+cmov,+cx8,+fxsr,+mmx,+sse,+sse2,+x87" "tune-cpu"="generic" }

!llvm.module.flags = !{!0, !1, !2, !3, !4, !5, !6, !7}
!0 = !{i32 8, !"PIC Level", i32 2}
!1 = !{i32 7, !"PIE Level", i32 2}
!2 = !{i32 1, !"Code Model", i32 1}
!3 = !{i32 8, !"cf-protection-branch", i32 1}
!4 = !{i32 8, !"cf-protection-return", i32 1}
!5 = !{i32 1, !"RtLibUseGOT", i32 1}
!6 = !{i32 7, !"direct-access-external-data", i32 1}
!7 = !{i32 4, !"kcfi", i32 1}
"#;

/// `hod_ll`'s module and bodies of `text`.
fn read(text: &[u8]) -> (Module, Vec<Body>) {
    let mut parsed =
        hod_ll::parse_module(text, &hod_ll::ParseOptions::default()).expect("the module reads");
    let mut parser = hod_ll::BodyParser::new(&parsed.text);
    let mut bodies = Vec::new();
    for function in parsed.text.functions() {
        let body = parser.parse(&mut parsed.module, function).expect("the body reads");
        bodies.push(body.expect("hod takes the body"));
    }
    (parsed.module, bodies)
}

fn print(module: &Module, bodies: &[Body], major: u32) -> String {
    let mut out = String::new();
    module.print(&mut out, Dialect::new(major).expect("a dialect"), bodies).expect("prints");
    out
}

/// A line of a dump, with the attributes of an `llvm.x86.*` declaration
/// left out: `llvm-as` gives such a declaration the attributes of LLVM's
/// intrinsic table (`nounwind`, ...), which `hod_ll` has only for the
/// intrinsics `hod_ir` knows.
fn masked(line: &str) -> String {
    match (line.starts_with("llvm.x86."), line.find(" attrs={"), line.find("} ret=")) {
        (true, Some(start), Some(end)) => {
            format!("{} attrs=... {}", &line[..start], &line[end + 2..])
        }
        _ => line.to_owned(),
    }
}

/// Panics with the first line where `a` and `b` differ.
fn same(a: &str, b: &str, what: &str) {
    let (a, b): (Vec<String>, Vec<String>) =
        (a.lines().map(masked).collect(), b.lines().map(masked).collect());
    if let Some((x, y)) = a.iter().zip(&b).find(|(x, y)| x != y) {
        panic!("{what} differ:\n  {x}\n  {y}");
    }
    assert_eq!(a.len(), b.len(), "{what}: line counts");
}

#[test]
fn both_readers_and_the_printer() {
    let text = format!("{HEADER}{MODULE}");
    for major in MAJORS {
        let Ok(bitcode) = common::assemble(major, text.as_bytes()) else {
            eprintln!("note: no llvm-as {major}; skipping");
            continue;
        };
        let by_text = common::read_text_bodies(text.as_bytes()).expect("hod_ll");
        let by_bitcode = common::read_bitcode_bodies(&bitcode).expect("hod_bc");
        same(&by_text, &by_bitcode, &format!("LLVM {major}: hod_ll and hod_bc"));

        let (module, bodies) = read(text.as_bytes());
        for body in &bodies {
            body.verify(&module).expect("verifies");
        }
        let printed = print(&module, &bodies, major);
        if let Err(error) = common::assemble(major, printed.as_bytes()) {
            panic!("LLVM {major}: llvm-as refuses the printed text: {error}\n{printed}");
        }
        for keyword in [
            // WP-141: every convention LLVM has a keyword for is
            // printed by its keyword.
            "x86_64_sysvcc i32 @sysv",
            "call x86_64_sysvcc i32 @sysv",
            "win64cc void @w64",
            "x86_stdcallcc void @std",
            "x86_fastcallcc void @fast",
            "x86_thiscallcc void @this",
            "x86_vectorcallcc void @vec",
            "x86_regcallcc void @reg",
            "x86_intrcc void @intr",
            "byval({ i64, x86_fp80 }) align 16",
            "inreg",
            "nest",
            "preallocated(i32)",
            "inalloca(i32)",
            "0xK3FFF8000000000000000",
            "0xKC000C000000000000000",
            "inteldialect",
            "={ax}",
            "addrspace(270)",
            "\"tune-cpu\"=\"generic\"",
            "call void @llvm.x86.sse2.pause()",
        ] {
            assert!(printed.contains(keyword), "LLVM {major}: no `{keyword}` in\n{printed}");
        }
        let (again, again_bodies) = read(printed.as_bytes());
        assert_eq!(
            print(&again, &again_bodies, major),
            printed,
            "LLVM {major}: printing is not a fixed point"
        );
        let reread = common::read_text_bodies(printed.as_bytes()).expect("hod_ll");
        same(&reread, &by_text, &format!("LLVM {major}: the printed text and the source"));
    }
}

/// The two refusals of the x86-64 sample: `x86_mmx` (LLVM 19; LLVM 20
/// removed the type) and `!kcfi_type` on a function. Both readers name
/// them.
#[test]
fn named_refusals() {
    let cases: [(u32, &str, &str); 3] = [
        (
            19,
            "define i64 @f(i64 %a) {\n  %m = bitcast i64 %a to x86_mmx\n  %b = bitcast x86_mmx %m to i64\n  ret i64 %b\n}\n",
            "x86_mmx",
        ),
        (19, "define void @k() !kcfi_type !0 {\n  ret void\n}\n!0 = !{i32 12345}\n", "kcfi_type"),
        (22, "define void @k() !kcfi_type !0 {\n  ret void\n}\n!0 = !{i32 12345}\n", "kcfi_type"),
    ];
    for (major, body, name) in cases {
        let text = format!("{HEADER}{body}");
        let Ok(bitcode) = common::assemble(major, text.as_bytes()) else {
            eprintln!("note: no llvm-as {major}; skipping");
            continue;
        };
        let by_text = common::read_text_bodies(text.as_bytes());
        let by_bitcode = common::read_bitcode_bodies(&bitcode);
        for (route, result) in [("hod_ll", &by_text), ("hod_bc", &by_bitcode)] {
            let refused = match result {
                Err(error) => error.contains(name),
                Ok(dump) => dump.contains("unsupported") && dump.contains(name),
            };
            assert!(refused, "LLVM {major} {route}: `{name}` is not refused by name: {result:?}");
        }
    }
}

/// WP-141: `CallConv::keyword` is LLVM's spelling of every convention,
/// in each dialect. For LLVM 19 and 22, a module declaring a function of
/// each number from 0 to 130, and 1023, goes through `llvm-as` and
/// `llvm-dis`: LLVM's spelling of each (a keyword or `ccN`) is
/// `keyword()`'s where the dialect has it (`keyword_since`); `hod_ll`
/// reads `llvm-dis`'s text and `hod_bc` the bitcode to the same numbers;
/// the printer's text of either dialect is taken by `llvm-as` and comes
/// back from `llvm-dis` as LLVM's own text.
#[test]
fn calling_convention_keywords_are_llvms() {
    let numbers: Vec<u32> = (0..=130).chain([1023]).collect();
    let mut text = String::from(HEADER);
    for &n in &numbers {
        // The verifier wants an `i1` first parameter of
        // `amdgpu_gfx_whole_wave` and a `byval` one of `x86_intrcc`.
        let param = if n == 83 { "ptr byval(i64)" } else { "i1" };
        writeln!(text, "declare cc {n} void @f{n}({param})").unwrap();
    }
    for major in MAJORS {
        let Ok(bitcode) = common::assemble(major, text.as_bytes()) else {
            eprintln!("note: no llvm-as {major}; skipping");
            continue;
        };
        let llvm = String::from_utf8(common::disassemble(major, &bitcode).unwrap()).unwrap();
        let spelled = |llvm: &str, n: u32| -> String {
            let line = llvm
                .lines()
                .find(|l| l.starts_with("declare ") && l.contains(&format!(" @f{n}(")))
                .unwrap_or_else(|| panic!("LLVM {major}: no @f{n} in\n{llvm}"));
            let head = &line["declare ".len()..line.find(&format!(" void @f{n}(")).unwrap_or(8)];
            // LLVM writes `avr_intrcc` and `avr_signalcc` with a space
            // of their own.
            head.trim_end().to_owned()
        };
        for &n in &numbers {
            let cc = hod_ir::CallConv::from_llvm(n).unwrap();
            let expected = match cc.keyword() {
                Some(_) if n == 0 => String::new(),
                Some(keyword) if cc.keyword_since() <= major => keyword.to_owned(),
                _ => format!("cc{n}"),
            };
            let head = if n == 0 { String::new() } else { spelled(&llvm, n) };
            assert_eq!(head, expected, "LLVM {major}: convention {n}");
            if !expected.is_empty() && !expected.starts_with("cc") {
                assert_eq!(hod_ir::CallConv::from_keyword(expected.as_bytes()), Some(cc));
            }
        }

        let ccs = |module: &Module| -> Vec<u32> {
            numbers
                .iter()
                .map(|n| {
                    let sym = module.find_symbol(format!("f{n}").as_bytes()).expect("declared");
                    match module.symbol(sym).kind() {
                        hod_ir::SymbolKind::Function { cc, .. } => u32::from(cc.llvm_id()),
                        _ => panic!("@f{n} is a function"),
                    }
                })
                .collect()
        };
        let (by_text, _) = read(llvm.as_bytes());
        assert_eq!(ccs(&by_text), numbers, "LLVM {major}: hod_ll on llvm-dis's text");
        let (by_bitcode, _) = common::read_bitcode(&bitcode).expect("hod_bc reads the bitcode");
        assert_eq!(ccs(&by_bitcode), numbers, "LLVM {major}: hod_bc");

        let printed = print(&by_text, &[], major);
        let again = common::assemble(major, printed.as_bytes())
            .unwrap_or_else(|error| panic!("LLVM {major}: llvm-as refuses\n{error}\n{printed}"));
        let again = String::from_utf8(common::disassemble(major, &again).unwrap()).unwrap();
        for &n in &numbers[1..] {
            assert_eq!(spelled(&again, n), spelled(&llvm, n), "LLVM {major}: printed {n}");
        }
        assert_eq!(ccs(&read(printed.as_bytes()).0), numbers, "LLVM {major}: printed, read back");
    }
}
