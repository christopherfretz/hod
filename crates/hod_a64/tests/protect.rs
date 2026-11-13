//! AArch64 branch protection (WP-185, `spec/codegen.md` §5.6.26,
//! `spec/eh.md` §4.5, `spec/object.md` "GNU property note").
//!
//! One module of leaf, non-leaf, indirect-call, computed-goto, switch,
//! landing-pad, `musttail` (direct and indirect), variadic,
//! dynamic-alloca, large-frame and `setjmp` functions, under each
//! `-mbranch-protection` form as clang 22 writes it (module flags of
//! value 2, the policy on every function):
//!
//! - shape, per function, against `llc -O0 -global-isel=0 -fast-isel=0`
//!   19 and 22: the entry instruction (exactly `llc`'s under `bti` and
//!   `pac-ret+leaf`; under `non-leaf` hod also signs the functions `llc`
//!   leaves unsigned, because hod's frame always saves `x30`), at least
//!   `llc`'s `bti j` count (equal to `llc` 22's: `llc` 19 puts none on a
//!   landing pad), every return and tail exit authenticated with the
//!   function's key;
//! - CFI: the B key's CIE (`zRB`, `zPLRB`) exactly where `llc` has it,
//!   and hod's `negate_ra_state` rows after the signing instruction and
//!   after every authentication;
//! - the note, byte for byte `llc`'s;
//! - the linked program (with a C++ driver built with
//!   `-mbranch-protection=standard`): `readelf -n` under GNU ld, lld
//!   (with `-z force-bti`, silent) and mold, and the program's output,
//!   the same as `llc` 22's object's;
//! - mutations of hod's object (a `bti j` made a `nop`, an `autiasp`
//!   made a `nop`, the `negate_ra_state` rows made `DW_CFA_nop`), each of
//!   which must kill the program where the processor enforces BTI and
//!   PAC (this host does; `qemu-aarch64 -cpu max` elsewhere);
//! - `gdb`'s backtrace through signed frames;
//! - C++ that clang 22 compiles with `-mbranch-protection=standard`
//!   (exceptions, `setjmp`, virtual calls, a coroutine, variadic, dynamic
//!   allocation, a large frame), run against `llc` 22's object;
//! - the refusal of an unknown scope or key.

mod common;

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

use common::linking::{Linker, Toolchain, exit_code};
use common::{assert_refused, emit, ll_header, parse, write_object};
use hod_cg::{Emitted, Options, RelocModel};
use hod_obj::SectionId;
use hod_testutil::{Outcome, Scratch, find_tool, run_with, tool};

/// hod's options: PIC, as `llc -relocation-model=pic` and the drivers.
fn pic() -> Options {
    Options { reloc_model: RelocModel::Pic, ..Options::default() }
}

/// `bti j`, `nop`, `autiasp`.
const BTI_J: u32 = 0xd503_249f;
const NOP: u32 = 0xd503_201f;
const AUTIASP: u32 = 0xd503_23bf;

const FUNCTIONS: &str = r"
declare void @may_throw(i32)
declare i32 @__gxx_personality_v0(...)
declare ptr @__cxa_begin_catch(ptr)
declare void @__cxa_end_catch()
declare i32 @setjmp(ptr) returns_twice
declare void @longjmp(ptr, i32) noreturn
declare void @llvm.va_start.p0(ptr)
declare void @llvm.va_end.p0(ptr)

define i32 @leaf(i32 %x) #0 {
  %r = add i32 %x, 1
  ret i32 %r
}

define i32 @nonleaf(i32 %x) #0 {
  %r = call i32 @leaf(i32 %x)
  %s = add i32 %r, 1
  ret i32 %s
}

define i32 @indirect(ptr %f, i32 %x) #0 {
  %r = call i32 %f(i32 %x)
  ret i32 %r
}

define i32 @computed(i32 %i) #0 {
entry:
  %tab = alloca [2 x ptr]
  store ptr blockaddress(@computed, %one), ptr %tab
  %p1 = getelementptr [2 x ptr], ptr %tab, i64 0, i64 1
  store ptr blockaddress(@computed, %two), ptr %p1
  %ix = zext i32 %i to i64
  %slot = getelementptr [2 x ptr], ptr %tab, i64 0, i64 %ix
  %t = load ptr, ptr %slot
  indirectbr ptr %t, [label %one, label %two]
one:
  ret i32 20
two:
  ret i32 30
}

define i32 @sw(i32 %x) #0 {
entry:
  switch i32 %x, label %other [
    i32 0, label %a
    i32 1, label %b
    i32 2, label %c
    i32 3, label %d
  ]
a:
  %ra = call i32 @leaf(i32 100)
  ret i32 %ra
b:
  ret i32 1
c:
  %rc = call i32 @nonleaf(i32 2)
  ret i32 %rc
d:
  ret i32 9
other:
  ret i32 -1
}

define i32 @mid(i32 %x) #0 {
  call void @may_throw(i32 %x)
  ret i32 %x
}

define i32 @pads(i32 %x) #0 personality ptr @__gxx_personality_v0 {
entry:
  %r = invoke i32 @mid(i32 %x) to label %ok unwind label %pad
ok:
  ret i32 0
pad:
  %lp = landingpad { ptr, i32 } catch ptr null
  %e = extractvalue { ptr, i32 } %lp, 0
  %o = call ptr @__cxa_begin_catch(ptr %e)
  %v = load i32, ptr %o
  call void @__cxa_end_catch()
  ret i32 %v
}

define i32 @tail(i32 %x) #0 {
  %y = add i32 %x, 2
  %r = musttail call i32 @leaf(i32 %y)
  ret i32 %r
}

define i32 @tail_ind(ptr %f, i32 %x) #0 {
  %r = musttail call i32 %f(ptr %f, i32 %x)
  ret i32 %r
}

define i32 @vararg(i32 %n, ...) #0 {
  %ap = alloca [32 x i8], align 8
  call void @llvm.va_start.p0(ptr %ap)
  call void @llvm.va_end.p0(ptr %ap)
  %r = call i32 @leaf(i32 %n)
  ret i32 %r
}

define i32 @dyn(i64 %n) #0 {
  %buf = alloca i32, i64 %n, align 4
  %last = getelementptr i32, ptr %buf, i64 2
  store volatile i32 40, ptr %last
  %v = load volatile i32, ptr %last
  %r = call i32 @leaf(i32 %v)
  ret i32 %r
}

define i32 @big(i32 %x) #0 {
  %big = alloca [70000 x i8], align 16
  %far = getelementptr [70000 x i8], ptr %big, i64 0, i64 69999
  %b = trunc i32 %x to i8
  store volatile i8 %b, ptr %far
  %l = load volatile i8, ptr %far
  %z = zext i8 %l to i32
  %r = call i32 @leaf(i32 %z)
  ret i32 %r
}

define void @jumper(ptr %buf) #0 {
  call void @longjmp(ptr %buf, i32 5)
  unreachable
}

define i32 @jmp() #0 {
  %buf = alloca [64 x i64], align 16
  %r = call i32 @setjmp(ptr %buf) #1
  %first = icmp eq i32 %r, 0
  br i1 %first, label %go, label %back
go:
  call void @jumper(ptr %buf)
  unreachable
back:
  ret i32 %r
}
";

/// The functions of [`FUNCTIONS`] (all of them with `#0`).
const NAMES: [&str; 14] = [
    "leaf", "nonleaf", "indirect", "computed", "sw", "mid", "pads", "tail", "tail_ind", "vararg",
    "dyn", "big", "jumper", "jmp",
];

/// The driver: `may_throw`, `tail_ind`'s callee, `main`.
const DRIVER: &str = r#"
#include <cstdio>
extern "C" {
int leaf(int); int nonleaf(int); int indirect(int (*)(int), int); int computed(int);
int sw(int); int pads(int); int tail(int); int tail_ind(void *, int); int vararg(int, ...);
int dyn(long); int big(int); int jmp(void);
void may_throw(int x) { if (x) throw x * 2; }
int self_tail(void *self, int x) { (void)self; return x * 3; }
}
int main() {
  std::printf("%d %d %d\n", leaf(1), nonleaf(1), indirect(leaf, 5));
  std::printf("%d %d\n", computed(0), computed(1));
  std::printf("%d %d %d %d %d\n", sw(0), sw(1), sw(2), sw(3), sw(9));
  std::printf("%d %d\n", pads(0), pads(21));
  std::printf("%d %d\n", tail(3), tail_ind((void *)self_tail, 4));
  std::printf("%d %d %d %d\n", vararg(6, 1, 2.0), dyn(3), big(200), jmp());
  return 0;
}
"#;

const EXPECTED: &str = "2 3 6\n20 30\n101 1 4 9 -1\n0 42\n6 12\n7 41 201 5\n";

/// A form of `-mbranch-protection`: its name, the attributes clang 22
/// puts on every function, and its module flags.
struct Form {
    name: &'static str,
    attrs: &'static str,
    bti: bool,
    sign: Option<&'static str>,
    b_key: bool,
    gcs: bool,
}

const FORMS: [Form; 6] = [
    Form { name: "bti", attrs: BTI, bti: true, sign: None, b_key: false, gcs: false },
    Form {
        name: "pac-ret",
        attrs: PAC,
        bti: false,
        sign: Some("non-leaf"),
        b_key: false,
        gcs: false,
    },
    Form {
        name: "pac-ret+leaf",
        attrs: LEAF,
        bti: false,
        sign: Some("all"),
        b_key: false,
        gcs: false,
    },
    Form {
        name: "pac-ret+b-key",
        attrs: BKEY,
        bti: false,
        sign: Some("non-leaf"),
        b_key: true,
        gcs: false,
    },
    Form {
        name: "standard",
        attrs: STANDARD,
        bti: true,
        sign: Some("non-leaf"),
        b_key: false,
        gcs: true,
    },
    Form { name: "gcs", attrs: GCS, bti: false, sign: None, b_key: false, gcs: true },
];

const BTI: &str = r#""branch-target-enforcement""#;
const PAC: &str = r#""sign-return-address"="non-leaf" "sign-return-address-key"="a_key""#;
const LEAF: &str = r#""sign-return-address"="all" "sign-return-address-key"="a_key""#;
const BKEY: &str = r#""sign-return-address"="non-leaf" "sign-return-address-key"="b_key""#;
const STANDARD: &str = r#""branch-target-enforcement" "guarded-control-stack" "sign-return-address"="non-leaf" "sign-return-address-key"="a_key""#;
const GCS: &str = r#""guarded-control-stack""#;

impl Form {
    fn module(&self, major: u32) -> String {
        let flag = |on: bool| u32::from(on) * 2;
        let signs = self.sign.is_some();
        let all = self.sign == Some("all");
        format!(
            "{}{FUNCTIONS}\nattributes #0 = {{ {} }}\nattributes #1 = {{ returns_twice }}\n\
             !llvm.module.flags = !{{!0, !1, !2, !3, !4}}\n\
             !0 = !{{i32 8, !\"branch-target-enforcement\", i32 {}}}\n\
             !1 = !{{i32 8, !\"sign-return-address\", i32 {}}}\n\
             !2 = !{{i32 8, !\"sign-return-address-all\", i32 {}}}\n\
             !3 = !{{i32 8, !\"sign-return-address-with-bkey\", i32 {}}}\n\
             !4 = !{{i32 8, !\"guarded-control-stack\", i32 {}}}\n",
            ll_header(major),
            self.attrs,
            flag(self.bti),
            flag(signs),
            flag(all),
            flag(self.b_key),
            flag(self.gcs),
        )
    }

    fn pac(&self) -> &'static str {
        if self.b_key { "pacibsp" } else { "paciasp" }
    }

    fn aut(&self) -> &'static str {
        if self.b_key { "autibsp" } else { "autiasp" }
    }
}

/// An entry instruction's class: a signing one, `bti c`, or neither.
fn class(entry: &str) -> &str {
    if entry.starts_with("pac") || entry == "bti c" { entry } else { "other" }
}

fn hod(module: &str) -> Emitted {
    let (m, bodies) = parse(module);
    emit(&m, &bodies, &pic())
}

fn llc(dir: &Scratch, major: u32, name: &str, module: &str) -> Option<PathBuf> {
    let llc = find_tool(&format!("llc-{major}"))?;
    let input = dir.write(&format!("{name}.ll"), module);
    let output = dir.join(format!("{name}.o"));
    let mut command = Command::new(llc);
    command
        .args(["-O0", "-global-isel=0", "-fast-isel=0", "-filetype=obj", "-relocation-model=pic"])
        .arg(&input)
        .arg("-o")
        .arg(&output);
    let ran = run_with(&mut command, &[], Duration::from_mins(2));
    assert!(ran.success(), "llc {major} {name}: {}", ran.stderr_text());
    Some(output)
}

fn run_tool(program: &Path, args: &[&str], file: &Path) -> Outcome {
    let mut command = Command::new(program);
    command.args(args).arg(file);
    let ran = run_with(&mut command, &[], Duration::from_mins(1));
    assert!(ran.success(), "{}: {}", program.display(), ran.stderr_text());
    ran
}

/// One function's code: `(offset in the function, instruction text)`,
/// and the offsets of branches with a `R_AARCH64_JUMP26` (tail exits).
#[derive(Default, Debug)]
struct Code {
    start: u64,
    insts: Vec<(u64, String)>,
    jumps: Vec<u64>,
}

/// `llvm-objdump -dr` of an object, by function.
fn disassemble(object: &Path) -> Option<BTreeMap<String, Code>> {
    let objdump = tool("llvm-objdump")?;
    let text = run_tool(&objdump, &["-dr", "--no-show-raw-insn"], object).stdout_text();
    let mut funcs: BTreeMap<String, Code> = BTreeMap::new();
    let mut current: Option<String> = None;
    for line in text.lines() {
        let trimmed = line.trim();
        if let Some(rest) = line.strip_suffix(">:") {
            let (address, name) = rest.split_once(" <").unwrap();
            let start = u64::from_str_radix(address, 16).unwrap();
            funcs.insert(name.to_owned(), Code { start, ..Code::default() });
            current = Some(name.to_owned());
            continue;
        }
        let Some(code) = current.as_ref().and_then(|name| funcs.get_mut(name)) else { continue };
        let Some((address, rest)) = trimmed.split_once(':') else { continue };
        let Ok(address) = u64::from_str_radix(address, 16) else { continue };
        let rest = rest.split("//").next().unwrap_or(rest);
        let words = rest.split_whitespace().collect::<Vec<_>>().join(" ");
        if words.starts_with("R_AARCH64_JUMP26") {
            code.jumps.push(address - code.start);
        } else if !words.starts_with("R_AARCH64") {
            code.insts.push((address - code.start, words));
        }
    }
    Some(funcs)
}

impl Code {
    fn entry(&self) -> &str {
        &self.insts[0].1
    }

    fn count(&self, text: &str) -> usize {
        self.insts.iter().filter(|(_, i)| i == text).count()
    }

    fn signs(&self) -> bool {
        self.entry().starts_with("pac")
    }

    /// The instruction before each exit: every `ret`, every branch with
    /// a `JUMP26` relocation, every `br x16`.
    fn before_exits(&self) -> Vec<&str> {
        let mut out = Vec::new();
        for (at, (offset, inst)) in self.insts.iter().enumerate().skip(1) {
            let exit = inst == "ret" || inst == "br x16" || self.jumps.contains(offset);
            if exit {
                out.push(self.insts[at - 1].1.as_str());
            }
        }
        out
    }

    fn offsets_of(&self, prefix: &str) -> Vec<u64> {
        self.insts.iter().filter(|(_, i)| i.starts_with(prefix)).map(|(o, _)| *o).collect()
    }
}

/// Per FDE, by the function's start: its CIE's augmentation and the
/// offsets (in the function) of its `negate_ra_state` rows.
fn frames(object: &Path) -> Option<BTreeMap<u64, (String, Vec<u64>)>> {
    let dwarfdump = tool("llvm-dwarfdump")?;
    let text = run_tool(&dwarfdump, &["--eh-frame"], object).stdout_text();
    let mut augmentation: BTreeMap<String, String> = BTreeMap::new();
    let mut out = BTreeMap::new();
    let mut cie: Option<String> = None;
    let mut fde: Option<(u64, u64)> = None;
    for line in text.lines() {
        let words: Vec<&str> = line.split_whitespace().collect();
        if words.len() == 4 && words[3] == "CIE" {
            cie = Some(words[0].to_owned());
            fde = None;
        } else if words.len() >= 5 && words[3] == "FDE" {
            let cie_at = words[4].trim_start_matches("cie=").to_owned();
            let pc = words[5].trim_start_matches("pc=").split("...").next().unwrap();
            let pc = u64::from_str_radix(pc, 16).unwrap();
            out.insert(pc, (augmentation[&cie_at].clone(), Vec::new()));
            fde = Some((pc, pc));
            cie = None;
        } else if let (Some(at), Some(aug)) = (&cie, line.trim().strip_prefix("Augmentation:")) {
            augmentation.insert(at.clone(), aug.trim().trim_matches('"').to_owned());
        } else if let Some((pc, loc)) = fde.as_mut() {
            let row = line.trim();
            if row.starts_with("DW_CFA_advance_loc") {
                let to = row.rsplit(" to 0x").next().unwrap();
                *loc = u64::from_str_radix(to, 16).unwrap();
            } else if row.starts_with("DW_CFA_AARCH64_negate_ra_state") {
                out.get_mut(pc).unwrap().1.push(*loc - *pc);
            } else if row.starts_with("0x") {
                fde = None;
            }
        }
    }
    Some(out)
}

/// `.note.gnu.property` as `llvm-readelf -x` dumps it (the bytes).
fn note(object: &Path) -> Option<String> {
    let readelf = tool("llvm-readelf")?;
    let mut command = Command::new(readelf);
    command.args(["-x", ".note.gnu.property"]).arg(object);
    let ran = run_with(&mut command, &[], Duration::from_mins(1));
    let text = ran.stdout_text() + &ran.stderr_text();
    Some(text.lines().filter(|l| l.trim_start().starts_with("0x")).collect::<Vec<_>>().join("\n"))
}

// Shape, CFI and note against `llc` 19 and 22, every form.
#[test]
fn shape_cfi_and_note_agree_with_llc() {
    let dir = Scratch::new("a64-protect-shape");
    let mut compared = 0;
    for form in &FORMS {
        let emitted = hod(&form.module(22));
        let object = write_object(&dir, &format!("hod-{}.o", form.name), &emitted);
        let Some(hod_code) = disassemble(&object) else { return };
        let Some(hod_frames) = frames(&object) else { return };
        let hod_note = note(&object).unwrap();
        for name in NAMES {
            let code = &hod_code[name];
            let what = format!("{} {name}", form.name);
            // The entry.
            let expected = match (form.sign, form.bti) {
                (Some(_), _) => form.pac(),
                (None, true) => "bti c",
                (None, false) => "",
            };
            if expected.is_empty() {
                assert!(
                    !code.entry().starts_with("pac") && !code.entry().starts_with("bti"),
                    "{what}"
                );
            } else {
                assert_eq!(code.entry(), expected, "{what}");
            }
            // Every exit authenticates, with the function's key.
            for before in code.before_exits() {
                if form.sign.is_some() {
                    assert_eq!(before, form.aut(), "{what}: an exit not authenticated");
                } else {
                    assert!(!before.starts_with("auti"), "{what}");
                }
            }
            // No `bti` at all without BTI.
            if !form.bti {
                assert_eq!(code.count("bti j") + code.count("bti c"), 0, "{what}");
            }
            // CFI: the key's CIE, a negate row after the signing and
            // after every authentication.
            let (aug, negates) = &hod_frames[&code.start];
            assert_eq!(aug.ends_with('B'), form.sign.is_some() && form.b_key, "{what}: {aug}");
            if form.sign.is_some() {
                let mut wanted = vec![4];
                wanted.extend(code.offsets_of("auti").iter().map(|o| o + 4));
                for at in &wanted {
                    assert!(negates.contains(at), "{what}: no negate at {at}: {negates:?}");
                }
                assert_eq!(negates.len() % 2, 0, "{what}: unbalanced {negates:?}");
                for (n, at) in negates.iter().enumerate() {
                    let state = n % 2 == 0; // the state after this row
                    let auth_before = code.offsets_of("auti").iter().any(|o| o + 4 == *at);
                    assert!(!state || *at == 4 || !auth_before, "{what}: {negates:?}");
                }
            } else {
                assert!(negates.is_empty(), "{what}: {negates:?}");
            }
        }
        for major in [19, 22] {
            let Some(theirs) =
                llc(&dir, major, &format!("llc{major}-{}", form.name), &form.module(major))
            else {
                continue;
            };
            let llc_code = disassemble(&theirs).unwrap();
            let llc_frames = frames(&theirs).unwrap();
            assert_eq!(note(&theirs).unwrap(), hod_note, "{} llc {major}: the note", form.name);
            for name in NAMES {
                let (ours, their) = (&hod_code[name], &llc_code[name]);
                let what = format!("{} {name} llc {major}", form.name);
                match form.sign {
                    Some("all") | None => {
                        assert_eq!(class(ours.entry()), class(their.entry()), "{what}")
                    }
                    Some(_) => {
                        if their.signs() {
                            assert_eq!(ours.entry(), their.entry(), "{what}");
                        } else if form.bti {
                            assert_eq!(their.entry(), "bti c", "{what}");
                        }
                    }
                }
                if their.signs() {
                    for before in their.before_exits() {
                        assert_eq!(before, form.aut(), "{what}: llc's exit");
                    }
                }
                let (mine, llcs) = (ours.count("bti j"), their.count("bti j"));
                if major == 22 {
                    assert_eq!(mine, llcs, "{what}: bti j");
                } else {
                    assert!(mine >= llcs, "{what}: bti j {mine} < {llcs}");
                }
                let b =
                    |f: &BTreeMap<u64, (String, Vec<u64>)>, c: &Code| f[&c.start].0.ends_with('B');
                if their.signs() {
                    assert_eq!(b(&hod_frames, ours), b(&llc_frames, their), "{what}: the B key");
                }
                compared += 1;
            }
        }
    }
    eprintln!("compared {compared} functions with llc");
}

/// The driver, built with `-mbranch-protection=standard` (so the
/// program's note keeps what the module has).
fn driver(dir: &Scratch, toolchain: &Toolchain) -> Option<PathBuf> {
    toolchain.compile_cxx(dir, "driver", DRIVER, &["-O0", "-mbranch-protection=standard"])
}

fn link(toolchain: &Toolchain, linker: &Linker, inputs: &[&Path], exe: &Path, flags: &[&str]) {
    let mut all = inputs.to_vec();
    all.push(Path::new("-lstdc++"));
    toolchain.link_ok(linker, flags, &all, exe);
}

/// Whether programs here run with BTI and PAC enforced: the host's CPU
/// has both (`/proc/cpuinfo`'s `bti` and `paca`), or the emulator models
/// `max`.
fn enforced(toolchain: &Toolchain) -> bool {
    if toolchain.emulated() {
        return true;
    }
    let cpuinfo = std::fs::read_to_string("/proc/cpuinfo").unwrap_or_default();
    let flags: Vec<&str> = cpuinfo
        .lines()
        .filter(|l| l.starts_with("Features"))
        .flat_map(str::split_whitespace)
        .collect();
    flags.contains(&"bti") && flags.contains(&"paca")
}

/// The AArch64 feature line of `readelf -n`, or "" without one.
fn features(exe: &Path) -> String {
    let Some(readelf) = find_tool("readelf") else { return String::new() };
    let text = run_tool(&readelf, &["-n"], exe).stdout_text();
    text.lines()
        .find_map(|l| l.trim().strip_prefix("Properties: AArch64 feature:"))
        .unwrap_or("")
        .trim()
        .to_owned()
}

// The linked program: its note under each linker, and its output, the
// same as `llc` 22's object's.
#[test]
fn programs_link_and_run_under_every_linker() {
    let dir = Scratch::new("a64-protect-run");
    let Some(toolchain) = Toolchain::aarch64(&dir) else { return };
    let toolchain = toolchain.with_emulated_cpu("max");
    let Some(driver) = driver(&dir, &toolchain) else { return };
    for form in &FORMS {
        let ours = write_object(&dir, &format!("hod-{}.o", form.name), &hod(&form.module(22)));
        let theirs = llc(&dir, 22, &format!("llc-{}", form.name), &form.module(22));
        for linker in &toolchain.linkers {
            for (tag, object) in [("hod", Some(&ours)), ("llc", theirs.as_ref())] {
                let Some(object) = object else { continue };
                let exe = dir.join(format!("{tag}-{}-{}", form.name, linker.name));
                let force: &[&str] =
                    if form.bti && linker.name == "lld" { &["-Wl,-z,force-bti"] } else { &[] };
                link(&toolchain, linker, &[&driver, object], &exe, force);
                let ran = toolchain.run(linker, &exe);
                assert_eq!(
                    exit_code(&ran),
                    Some(0),
                    "{tag} {} {}: {}",
                    form.name,
                    linker.name,
                    ran.stderr_text()
                );
                assert_eq!(ran.stdout_text(), EXPECTED, "{tag} {} {}", form.name, linker.name);
                let seen = features(&exe);
                eprintln!("{tag} {} {}: AArch64 feature: {seen:?}", form.name, linker.name);
                if linker.name == "mold" {
                    // mold 2.37 leaves the AArch64 property out.
                    continue;
                }
                assert_eq!(
                    seen.contains("BTI"),
                    form.bti,
                    "{tag} {} {}: {seen}",
                    form.name,
                    linker.name
                );
                assert_eq!(
                    seen.contains("PAC"),
                    form.sign.is_some(),
                    "{tag} {} {}: {seen}",
                    form.name,
                    linker.name
                );
            }
        }
    }
}

/// A module of `form`, parsed and emitted.
fn compiled(module: &str) -> (hod_ir::Module, Emitted) {
    let (m, bodies) = parse(module);
    let emitted = emit(&m, &bodies, &pic());
    (m, emitted)
}

/// Every `word` in the code of the function `name` replaced by `with`;
/// how many.
fn mutate_code(
    m: &hod_ir::Module,
    emitted: &mut Emitted,
    name: &str,
    word: u32,
    with: u32,
) -> usize {
    let placed = emitted
        .funcs
        .iter()
        .find(|f| m.symbol(f.record.sym).name() == name.as_bytes())
        .unwrap_or_else(|| panic!("no function {name}"));
    let (section, range) = (placed.section, placed.record.code.clone());
    let data = emitted.object.data_mut(section);
    let mut changed = 0;
    for at in (range.start as usize..range.end as usize).step_by(4) {
        if data[at..at + 4] == word.to_le_bytes() {
            data[at..at + 4].copy_from_slice(&with.to_le_bytes());
            changed += 1;
        }
    }
    changed
}

/// Every FDE's `DW_CFA_AARCH64_negate_ra_state` at the start of its
/// instructions (`advance_loc 4; negate`) made `DW_CFA_nop`.
fn drop_negate_rows(emitted: &mut Emitted) -> usize {
    let object = &mut emitted.object;
    let eh = (0..object.section_count())
        .map(SectionId::from_index)
        .find(|&s| object.section_name(s) == b".eh_frame")
        .expect(".eh_frame");
    let data = object.data_mut(eh);
    let (mut at, mut changed) = (0, 0);
    while at + 8 <= data.len() {
        let len = u32::from_le_bytes(data[at..at + 4].try_into().unwrap()) as usize;
        let id = u32::from_le_bytes(data[at + 4..at + 8].try_into().unwrap());
        if id != 0 {
            let aug_len = data[at + 16] as usize;
            let first = at + 17 + aug_len;
            if data[first..first + 2] == [0x44, 0x2d] {
                data[first + 1] = 0;
                changed += 1;
            }
        }
        at += 4 + len;
    }
    changed
}

// Each mutation kills the program where BTI and PAC are enforced.
#[test]
fn mutations_are_caught_by_the_processor() {
    let dir = Scratch::new("a64-protect-mutate");
    let Some(toolchain) = Toolchain::aarch64(&dir) else { return };
    let toolchain = toolchain.with_emulated_cpu("max");
    if !enforced(&toolchain) {
        eprintln!("BTI and PAC are not enforced here: the mutations are not run");
        return;
    }
    let Some(driver) = driver(&dir, &toolchain) else { return };
    let Some(bfd) = toolchain.linkers.iter().find(|l| l.name == "bfd") else { return };
    let standard = &FORMS[4];
    let module = standard.module(22);
    let cases: [(&str, fn(&hod_ir::Module, &mut Emitted) -> usize); 3] = [
        ("a bti j dropped", |m, e| mutate_code(m, e, "computed", BTI_J, NOP)),
        ("an autiasp skipped", |m, e| mutate_code(m, e, "nonleaf", AUTIASP, NOP)),
        ("the negate rows dropped", |_, e| drop_negate_rows(e)),
    ];
    let baseline = write_object(&dir, "base.o", &compiled(&module).1);
    let exe = dir.join("base");
    link(&toolchain, bfd, &[&driver, &baseline], &exe, &[]);
    assert_eq!(toolchain.run(bfd, &exe).stdout_text(), EXPECTED);
    for (n, (what, mutate)) in cases.iter().enumerate() {
        let (m, mut emitted) = compiled(&module);
        assert!(mutate(&m, &mut emitted) > 0, "{what}: nothing to mutate");
        let object = write_object(&dir, &format!("m{n}.o"), &emitted);
        let exe = dir.join(format!("m{n}"));
        link(&toolchain, bfd, &[&driver, &object], &exe, &[]);
        let ran = toolchain.run(bfd, &exe);
        assert_eq!(exit_code(&ran), None, "{what}: not killed: {}", ran.stdout_text());
        eprintln!("{what}: exit {:?}, status {:?}", exit_code(&ran), ran.status);
    }
}

// gdb walks a backtrace through hod's signed frames.
#[test]
fn gdb_backtraces_through_signed_frames() {
    let dir = Scratch::new("a64-protect-gdb");
    let Some(toolchain) = Toolchain::aarch64(&dir) else { return };
    if toolchain.emulated() {
        return;
    }
    let Some(gdb) = find_tool("gdb") else { return };
    let Some(driver) = driver(&dir, &toolchain) else { return };
    let Some(bfd) = toolchain.linkers.iter().find(|l| l.name == "bfd") else { return };
    for form in [&FORMS[4], &FORMS[3]] {
        let object = write_object(&dir, &format!("{}.o", form.name), &hod(&form.module(22)));
        let exe = dir.join(form.name);
        link(&toolchain, bfd, &[&driver, &object], &exe, &[]);
        let mut command = Command::new(&gdb);
        command
            .args(["-q", "-batch", "-nx", "-ex", "break may_throw", "-ex", "run", "-ex", "bt"])
            .arg(&exe);
        let ran = run_with(&mut command, &[], Duration::from_mins(2));
        let text = ran.stdout_text();
        let frames: Vec<&str> = text
            .lines()
            .filter(|l| l.starts_with('#'))
            .filter_map(|l| {
                ["may_throw", "mid", "pads", "main"].into_iter().find(|f| {
                    l.contains(&format!(" {f} "))
                        || l.contains(&format!(" {f} ("))
                        || l.ends_with(&format!(" {f} ()"))
                })
            })
            .collect();
        assert_eq!(frames, ["may_throw", "mid", "pads", "main"], "{}:\n{text}", form.name);
        assert!(!text.contains("Backtrace stopped"), "{}:\n{text}", form.name);
    }
}

const CXX: &str = r#"
#include <coroutine>
#include <csetjmp>
#include <cstdarg>
#include <cstdio>
#include <exception>

struct Base { virtual int f(int x) { return x; } virtual ~Base() = default; };
struct Derived : Base { int f(int x) override { return x * 10; } };

struct Task {
  struct promise_type {
    int value = 0;
    Task get_return_object() { return Task{std::coroutine_handle<promise_type>::from_promise(*this)}; }
    std::suspend_always initial_suspend() noexcept { return {}; }
    std::suspend_always final_suspend() noexcept { return {}; }
    std::suspend_always yield_value(int v) { value = v; return {}; }
    void return_void() {}
    void unhandled_exception() { std::terminate(); }
  };
  std::coroutine_handle<promise_type> h;
  ~Task() { if (h) h.destroy(); }
};

Task counter(int n) { for (int i = 1; i <= n; ++i) co_yield i * i; }

static std::jmp_buf env;
[[gnu::noinline]] void jump_back(int v) { std::longjmp(env, v); }

[[gnu::noinline]] int sum(int n, ...) {
  va_list ap; va_start(ap, n);
  int s = 0;
  for (int i = 0; i < n; ++i) s += va_arg(ap, int);
  va_end(ap);
  return s;
}

[[gnu::noinline]] int deep(int n) {
  if (n == 0) throw n + 77;
  volatile char big[9000];
  big[8999] = (char)n;
  return deep(n - 1) + big[8999];
}

[[gnu::noinline]] int dynamic(int n) {
  int *p = (int *)__builtin_alloca(n * sizeof(int));
  for (int i = 0; i < n; ++i) p[i] = i;
  return p[n - 1];
}

int main() {
  Base *b = new Derived;
  int (*fp)(int) = [](int x) { return x + 1; };
  std::printf("virtual %d pointer %d\n", b->f(4), fp(4));
  delete b;
  int caught = 0;
  try { deep(5); } catch (int v) { caught = v; }
  std::printf("caught %d\n", caught);
  int r = setjmp(env);
  if (r == 0) jump_back(9);
  std::printf("longjmp %d\n", r);
  std::printf("sum %d dynamic %d\n", sum(4, 1, 2, 3, 4), dynamic(12));
  Task t = counter(4);
  int total = 0;
  while (!t.h.done()) { t.h.resume(); if (!t.h.done()) total += t.h.promise().value; }
  std::printf("coroutine %d\n", total);
  return 0;
}
"#;

const CXX_EXPECTED: &str =
    "virtual 40 pointer 5\ncaught 77\nlongjmp 9\nsum 10 dynamic 11\ncoroutine 30\n";

// C++ clang 22 compiles with `-mbranch-protection=standard`, through
// hod, linked and run where BTI and PAC are enforced, as `llc` 22's.
#[test]
fn clang_cxx_runs_with_branch_protection() {
    let dir = Scratch::new("a64-protect-cxx");
    let Some(toolchain) = Toolchain::aarch64(&dir) else { return };
    let toolchain = toolchain.with_emulated_cpu("max");
    let Some(clang) = find_tool("clang++-22") else { return };
    let source = dir.write("p.cpp", CXX);
    let ll = dir.join("p.ll");
    let mut command = Command::new(clang);
    command
        .args(["-std=c++20", "-O0", "-S", "-emit-llvm", "-mbranch-protection=standard"])
        .args(["--target=aarch64-linux-gnu", "-fPIC"])
        .arg(&source)
        .arg("-o")
        .arg(&ll);
    let ran = run_with(&mut command, &[], Duration::from_mins(3));
    assert!(ran.success(), "clang: {}", ran.stderr_text());
    let text = std::fs::read_to_string(&ll).unwrap();
    let ours = write_object(&dir, "hod.o", &hod(&text));
    let theirs = llc(&dir, 22, "llc", &text);
    for linker in &toolchain.linkers {
        for (tag, object) in [("hod", Some(&ours)), ("llc", theirs.as_ref())] {
            let Some(object) = object else { continue };
            let exe = dir.join(format!("{tag}-{}", linker.name));
            link(&toolchain, linker, &[object], &exe, &[]);
            let ran = toolchain.run(linker, &exe);
            assert_eq!(exit_code(&ran), Some(0), "{tag} {}: {}", linker.name, ran.stderr_text());
            assert_eq!(ran.stdout_text(), CXX_EXPECTED, "{tag} {}", linker.name);
            if linker.name != "mold" {
                let seen = features(&exe);
                assert!(
                    seen.contains("BTI") && seen.contains("PAC"),
                    "{tag} {}: {seen}",
                    linker.name
                );
            }
        }
    }
}

// An unknown scope or key is refused by name.
#[test]
fn unknown_values_are_refused() {
    let function = |attrs: &str| {
        format!("define i32 @f(i32 %x) #0 {{\n  ret i32 %x\n}}\nattributes #0 = {{ {attrs} }}\n")
    };
    assert_refused(&[
        (&function(r#""sign-return-address"="some""#), hod_a64::protect::PROTECT_VALUE),
        (
            &function(r#""sign-return-address"="all" "sign-return-address-key"="c_key""#),
            hod_a64::protect::PROTECT_VALUE,
        ),
    ]);
}
