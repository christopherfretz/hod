//! Named registers (WP-064, `spec/codegen.md` §5.5.1 "WP-064"):
//! `llvm.read_register`, `llvm.read_volatile_register` and
//! `llvm.write_register`, against `llc -O0 -global-isel=0 -fast-isel=0`
//! of LLVM 19 and 22.
//!
//! The instructions that touch the register (`mrs`, `msr`, `mov xd, sp`,
//! `mov xd, x29`) are compared word for word with `llc`'s, the register
//! that receives or gives the value masked out; a table of functions runs
//! natively, built by hod in every mode and by `llc`, against values that
//! hold on any machine; everything else is refused by name, and the
//! interpreter refuses the intrinsics by name.

mod common;
#[path = "common/native.rs"]
mod native;

use std::fmt::Write as _;
use std::path::PathBuf;
use std::process::Command;
use std::time::Duration;

use common::{assert_refused, chunk, ll_header, parse};
use hod_a64::named_reg::{self, Named, REGISTER, SYSREGS, WIDTH, WRITE_GPR};
use hod_cg::{Options, RelocModel, Stress};
use hod_interp::{Interp, Program, Stop};
use hod_testutil::{LLVM_MAJORS, Scratch, run_with, tool};

const DEFAULT: Options = Options {
    reloc_model: RelocModel::Static,
    code_model: None,
    function_sections: false,
    data_sections: false,
    stress: Stress { min_pool: false, far: false, displace: false },
    debug_compression: hod_obj::Compression::None,
};

const MODES: [(&str, Options); 3] = [
    ("plain", DEFAULT),
    ("stress", Options { stress: Stress::ALL, ..DEFAULT }),
    ("far", Options { stress: Stress { min_pool: false, far: true, displace: false }, ..DEFAULT }),
];

const DECLARE: &str = "declare i64 @llvm.read_register.i64(metadata)\n\
     declare i64 @llvm.read_volatile_register.i64(metadata)\n\
     declare void @llvm.write_register.i64(metadata, i64)\n";

/// A function that reads `name` and returns it.
fn reader(index: usize, name: &str, volatile: bool) -> String {
    let which = if volatile { "read_volatile_register" } else { "read_register" };
    format!(
        "define i64 @r{index}() {{\n  %a = call i64 @llvm.{which}.i64(metadata !{{!\"{name}\"}})\n  \
         ret i64 %a\n}}\n"
    )
}

/// A function that writes its argument to `name`.
fn writer(index: usize, name: &str) -> String {
    format!(
        "define void @w{index}(i64 %v) {{\n  call void @llvm.write_register.i64(metadata \
         !{{!\"{name}\"}}, i64 %v)\n  ret void\n}}\n"
    )
}

/// The words of `code` that touch a named register, with the general
/// register that receives or gives the value masked out: `mrs`, `msr`
/// (register), `mov xd, sp` and `mov xd, x29`/`x30`.
fn touching(code: &[u32]) -> Vec<u32> {
    code.iter()
        // `mov x29, sp` sets up a frame record.
        .filter(|&&word| word != 0x9100_03fd)
        .map(|&word| word & !0x1f)
        .filter(|&word| {
            word & 0xfff0_0000 == 0xd530_0000
                || word & 0xfff0_0000 == 0xd510_0000
                || word == 0x9100_03e0
                || word == 0xaa1d_03e0
                || word == 0xaa1e_03e0
        })
        .collect()
}

/// The code words of an object's `.text`, through `llvm-objdump`.
fn object_words(object: &std::path::Path) -> Option<Vec<u32>> {
    let objdump = tool("llvm-objdump")?;
    let mut command = Command::new(objdump);
    command.args(["-d", "--no-leading-addr"]).arg(object);
    let outcome = run_with(&mut command, &[], Duration::from_mins(1));
    assert!(outcome.success(), "llvm-objdump: {}", outcome.stderr_text());
    Some(
        outcome
            .stdout_text()
            .lines()
            .filter(|line| line.starts_with(char::is_whitespace))
            .filter_map(|line| {
                let hex = line.split_whitespace().next()?;
                if hex.len() == 8 { u32::from_str_radix(hex, 16).ok() } else { None }
            })
            .collect(),
    )
}

/// `llc` of `major` on `text`, to an object; `None` if not installed.
fn llc(dir: &Scratch, major: u32, text: &str) -> Option<PathBuf> {
    let llc = PathBuf::from(format!("/usr/lib/llvm-{major}/bin/llc"));
    if !llc.is_file() {
        return None;
    }
    let input = dir.write(&format!("llc-{major}.ll"), text);
    let output = dir.join(format!("llc-{major}.o"));
    let mut command = Command::new(llc);
    command
        .args(["-O0", "-global-isel=0", "-fast-isel=0", "-filetype=obj"])
        .arg(&input)
        .arg("-o")
        .arg(&output);
    let outcome = run_with(&mut command, &[], Duration::from_mins(2));
    assert!(outcome.success(), "llc-{major}: {}", outcome.stderr_text());
    Some(output)
}

/// The names the encoding test covers: every table entry (in another
/// case too), the general registers, and the generic form.
fn names() -> Vec<String> {
    let mut names: Vec<String> =
        SYSREGS.iter().map(|(name, ..)| name.to_ascii_lowercase()).collect();
    names.extend(["CntVct_El0", "sp", "x29", "x30"].map(str::to_owned));
    names.extend(
        ["3:3:14:0:2", "2:0:0:0:0", "3:7:15:15:7", "3:0:0:0:5", "2:3:0:5:0"].map(str::to_owned),
    );
    names
}

// §5.5.1 "WP-064": every register hod reads or writes by name, encoded
// as `llc` 19 and 22 encode it.
#[test]
fn encodings_match_llc() {
    let mut body = String::from(DECLARE);
    let mut index = 0;
    for name in names() {
        let named = named_reg::named(&name).unwrap_or_else(|| panic!("{name} is named"));
        body.push_str(&reader(index, &name, index % 2 == 1));
        index += 1;
        if matches!(named, Named::Sys(_, true)) {
            body.push_str(&writer(index, &name));
            index += 1;
        }
    }
    let (module, bodies) = parse(&format!("{}{body}", ll_header(22)));
    let ours = chunk(&module, &bodies, &DEFAULT).unwrap_or_else(|e| panic!("{e}"));
    let words: Vec<u32> =
        ours.code.chunks(4).map(|w| u32::from_le_bytes([w[0], w[1], w[2], w[3]])).collect();
    let ours = touching(&words);
    assert_eq!(ours.len(), index, "one register access per function");
    let dir = Scratch::new("a64-named-reg-enc");
    let mut checked = 0;
    for major in LLVM_MAJORS {
        let Some(object) = llc(&dir, major, &format!("{}{body}", ll_header(major))) else {
            continue;
        };
        let Some(theirs) = object_words(&object) else { continue };
        assert_eq!(touching(&theirs), ours, "llc-{major}");
        checked += 1;
    }
    eprintln!("{index} register accesses agree with {checked} llc versions");
}

// §5.5.1 "WP-064": the reads and writes run, built by hod in every mode
// and by `llc`, with results that hold on any machine: the counter does
// not go back, the stack pointer is 16-byte aligned and near a local,
// the frame pointer is aligned and above the stack pointer, and what is
// written to `TPIDRRO_EL0`-free state (`FPCR`, `NZCV`, `TPIDR_EL0`
// restored at once) reads back.
#[test]
fn registers_run_as_llc_builds_them() {
    let body = format!(
        "{DECLARE}\
define i64 @counter() {{
  %a = call i64 @llvm.read_volatile_register.i64(metadata !0)
  %b = call i64 @llvm.read_volatile_register.i64(metadata !1)
  %up = icmp uge i64 %b, %a
  %r = zext i1 %up to i64
  ret i64 %r
}}
define i64 @stack() {{
  %p = alloca i64
  store i64 1, ptr %p
  %sp = call i64 @llvm.read_register.i64(metadata !2)
  %low = and i64 %sp, 15
  %addr = ptrtoint ptr %p to i64
  %d = sub i64 %addr, %sp
  %near = icmp ult i64 %d, 4096
  %aligned = icmp eq i64 %low, 0
  %both = and i1 %near, %aligned
  %r = zext i1 %both to i64
  ret i64 %r
}}
define i64 @frame() {{
  %fp = call i64 @llvm.read_register.i64(metadata !3)
  %sp = call i64 @llvm.read_register.i64(metadata !2)
  %lr = call i64 @llvm.read_register.i64(metadata !{{!\"x30\"}})
  %above = icmp uge i64 %fp, %sp
  %low = and i64 %fp, 7
  %aligned = icmp eq i64 %low, 0
  %ret = icmp ne i64 %lr, 0
  %a = and i1 %above, %aligned
  %b = and i1 %a, %ret
  %r = zext i1 %b to i64
  ret i64 %r
}}
define i64 @fpcr(i64 %x) {{
  %old = call i64 @llvm.read_register.i64(metadata !4)
  %rm = and i64 %x, 12582912
  call void @llvm.write_register.i64(metadata !4, i64 %rm)
  %got = call i64 @llvm.read_volatile_register.i64(metadata !4)
  call void @llvm.write_register.i64(metadata !4, i64 %old)
  ret i64 %got
}}
define i64 @nzcv(i64 %x) {{
  %flags = and i64 %x, 4026531840
  call void @llvm.write_register.i64(metadata !5, i64 %flags)
  %got = call i64 @llvm.read_volatile_register.i64(metadata !5)
  ret i64 %got
}}
define i64 @tpidr(i64 %x) {{
  %old = call i64 @llvm.read_register.i64(metadata !6)
  call void @llvm.write_register.i64(metadata !6, i64 %x)
  %got = call i64 @llvm.read_volatile_register.i64(metadata !6)
  call void @llvm.write_register.i64(metadata !6, i64 %old)
  ret i64 %got
}}
define i64 @generic() {{
  %a = call i64 @llvm.read_volatile_register.i64(metadata !7)
  %f = call i64 @llvm.read_register.i64(metadata !8)
  %nz = icmp ne i64 %a, 0
  %fnz = icmp ne i64 %f, 0
  %b = and i1 %nz, %fnz
  %r = zext i1 %b to i64
  ret i64 %r
}}
!0 = !{{!\"cntvct_el0\"}}
!1 = !{{!\"CNTVCT_EL0\"}}
!2 = !{{!\"sp\"}}
!3 = !{{!\"x29\"}}
!4 = !{{!\"fpcr\"}}
!5 = !{{!\"nzcv\"}}
!6 = !{{!\"tpidr_el0\"}}
!7 = !{{!\"3:3:14:0:2\"}}
!8 = !{{!\"cntfrq_el0\"}}
"
    );
    let mut calls: Vec<native::Call> =
        ["counter", "stack", "frame", "generic"].map(|name| (name.to_owned(), [0; 6])).to_vec();
    let mut want = String::from("1\n1\n1\n1\n");
    for x in [0u64, 0x40_0000, 0x80_0000, 0xc0_0000, u64::MAX] {
        calls.push(("fpcr".to_owned(), [x, 0, 0, 0, 0, 0]));
        writeln!(want, "{}", x & 0xc0_0000).unwrap();
    }
    for x in [0u64, 0x4000_0000, 0xf000_0000, u64::MAX] {
        calls.push(("nzcv".to_owned(), [x, 0, 0, 0, 0, 0]));
        writeln!(want, "{}", x & 0xf000_0000).unwrap();
    }
    for x in [0u64, 1, 0x1234_5678_9abc_def0] {
        calls.push(("tpidr".to_owned(), [x, 0, 0, 0, 0, 0]));
        writeln!(want, "{x}").unwrap();
    }
    let count = native::run_table("a64-named-reg", &body, &calls, &MODES, Some(&want));
    eprintln!("{count} calls agree");
}

// §5.5.1 "WP-064": what hod does not read or write by name is refused by
// name: the general registers `llc` refuses or misreads, a system
// register outside the table, a malformed generic form, a write of a
// read-only register or of a general one, another type than `i64`, a
// string where `llc` wants a node.
#[test]
fn the_rest_is_refused_by_name() {
    let read = |name: &str| format!("{DECLARE}{}", reader(0, name, false));
    let write = |name: &str| format!("{DECLARE}{}", writer(0, name));
    let mut cases: Vec<(String, &'static str)> = Vec::new();
    for name in [
        "x0",
        "x1",
        "x18",
        "x28",
        "xzr",
        "fp",
        "lr",
        "SP",
        "X29",
        "w0",
        "wsp",
        "pc",
        "bogus",
        "midr_el2",
        "cntvctss_el0",
        "0:0:0:0:0",
        "1:3:14:0:2",
        "4:0:0:0:0",
        "3:8:0:0:0",
        "3:3:14:0:2x",
        "3:3:14:0",
        "3:3:14:0:2:1",
        "S3_3_C14_C0_2",
        "",
    ] {
        cases.push((read(name), REGISTER));
    }
    for name in ["cntvct_el0", "midr_el1", "ctr_el0"] {
        cases.push((write(name), REGISTER));
    }
    for name in ["sp", "x29", "x30"] {
        cases.push((write(name), WRITE_GPR));
    }
    cases.push((
        "declare i32 @llvm.read_register.i32(metadata)\ndefine i32 @r() {\n  \
         %a = call i32 @llvm.read_register.i32(metadata !{!\"fpcr\"})\n  ret i32 %a\n}\n"
            .to_owned(),
        WIDTH,
    ));
    cases.push((
        "declare void @llvm.write_register.i32(metadata, i32)\ndefine void @w(i32 %v) {\n  \
         call void @llvm.write_register.i32(metadata !{!\"fpcr\"}, i32 %v)\n  ret void\n}\n"
            .to_owned(),
        WIDTH,
    ));
    for form in ["!\"sp\"", "!{!\"sp\", !\"x29\"}", "!{}"] {
        cases.push((
            format!(
                "{DECLARE}define i64 @r() {{\n  %a = call i64 @llvm.read_register.i64(metadata \
                 {form})\n  ret i64 %a\n}}\n"
            ),
            REGISTER,
        ));
    }
    let cases: Vec<(&str, &'static str)> = cases.iter().map(|(t, r)| (t.as_str(), *r)).collect();
    assert_refused(&cases);
}

// §5.5.1 "WP-064": the interpreter has no registers; it refuses the
// three intrinsics by name.
#[test]
fn the_interpreter_refuses_them_by_name() {
    let text = format!(
        "{}{DECLARE}{}{}{}",
        ll_header(22),
        reader(0, "sp", false),
        reader(1, "cntvct_el0", true),
        writer(2, "fpcr")
    );
    let (module, bodies) = parse(&text);
    let program = Program::with_bodies(&module, &bodies).expect("a program");
    for (name, args, reason) in [
        ("r0", vec![], "intrinsic:llvm.read_register"),
        ("r1", vec![], "intrinsic:llvm.read_volatile_register"),
        ("w2", vec![hod_interp::Val::int(64, 0)], "intrinsic:llvm.write_register"),
    ] {
        let sym = module.find_symbol(name.as_bytes()).expect("a function");
        let outcome = Interp::new(&program).run(sym, &args);
        assert_eq!(outcome.result, Err(Stop::Unsupported(reason.into())), "{name}");
    }
}
