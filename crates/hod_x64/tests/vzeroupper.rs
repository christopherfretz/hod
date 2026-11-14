//! WP-203: where hod writes `vzeroupper` (`spec/abi-x64.md` §8.7),
//! against where `llc` 19 and 22 write it at `-O0` (`FastISel` and
//! `SelectionDAG`), for functions built for several machines: before
//! which calls, before which tail branches and before the return.

#[path = "../../hod_obj/tests/common/linking.rs"]
#[allow(dead_code, reason = "a shared helper: not every function is used here")]
mod linking;

#[path = "common/aggr_gen.rs"]
#[allow(dead_code, reason = "a shared helper: not every function is used here")]
mod aggr_gen;

use std::path::Path;
use std::process::Command;
use std::time::Duration;

use aggr_gen::{PIE_FLAGS, header, llc_of, parse, pic};
use hod_cg::Stress;
use hod_testutil::{Scratch, run_with, tool};

/// The functions: each shape of §8.7's rule.
const BODIES: &str = r#"
declare void @g()
declare void @g2()
declare <4 x i64> @w(<4 x i64>)
declare void @h(<4 x i64>, ptr)
declare void @h3(ptr)
declare void @vf(i32, ...)
declare void @bv(ptr byval({ <4 x i64>, <4 x i64> }) align 32)
declare i32 @pers(...)

define void @two(ptr %p) #0 {
  %v = load <4 x i64>, ptr %p, align 1
  %q = getelementptr i8, ptr %p, i64 64
  store <4 x i64> %v, ptr %q, align 1
  call void @g()
  call void @g2()
  ret void
}
define void @after(ptr %p) #0 {
  call void @g()
  %v = load <4 x i64>, ptr %p, align 1
  %q = getelementptr i8, ptr %p, i64 64
  store <4 x i64> %v, ptr %q, align 1
  ret void
}
define void @branch(ptr %p, i1 %c) #0 {
  br i1 %c, label %a, label %b
a:
  %v = load <4 x i64>, ptr %p, align 1
  %q = getelementptr i8, ptr %p, i64 64
  store <4 x i64> %v, ptr %q, align 1
  br label %j
b:
  br label %j
j:
  call void @g()
  ret void
}
define void @result(ptr %p) #0 {
  %v = load <4 x i64>, ptr %p, align 1
  %r = call <4 x i64> @w(<4 x i64> %v)
  call void @g()
  ret void
}
define void @unused(<4 x i64> %a) #0 {
  call void @g()
  ret void
}
define void @used(<4 x i64> %a, ptr %p) #0 {
  store <4 x i64> %a, ptr %p, align 1
  call void @g()
  ret void
}
define <4 x i64> @returned(ptr %p) #0 {
  call void @g()
  %v = load <4 x i64>, ptr %p, align 1
  ret <4 x i64> %v
}
define void @variadic(ptr %p) #0 {
  %v = load <4 x i64>, ptr %p, align 1
  call void (i32, ...) @vf(i32 1, <4 x i64> %v)
  ret void
}
define void @byval(ptr %p) #0 {
  call void @bv(ptr byval({ <4 x i64>, <4 x i64> }) align 32 %p)
  ret void
}
define void @loop(ptr %p, i32 %n) #0 {
e:
  br label %l
l:
  %i = phi i32 [0, %e], [%k, %l]
  call void @g()
  %v = load <4 x i64>, ptr %p, align 1
  %q = getelementptr i8, ptr %p, i64 64
  store <4 x i64> %v, ptr %q, align 1
  %k = add i32 %i, 1
  %c = icmp slt i32 %k, %n
  br i1 %c, label %l, label %x
x:
  ret void
}
define void @tail_wide(<4 x i64> %a, ptr %p) #0 {
  store <4 x i64> %a, ptr %p, align 1
  musttail call void @h(<4 x i64> zeroinitializer, ptr %p)
  ret void
}
define void @tail(ptr %p) #0 {
  %v = load <4 x i64>, ptr %p, align 1
  %q = getelementptr i8, ptr %p, i64 64
  store <4 x i64> %v, ptr %q, align 1
  musttail call void @h3(ptr %p)
  ret void
}
define void @arith(ptr %p) #0 {
  %v = load <8 x i32>, ptr %p, align 1
  %w = add <8 x i32> %v, %v
  %q = getelementptr i8, ptr %p, i64 64
  store <8 x i32> %w, ptr %q, align 1
  call void @g()
  ret void
}
define void @narrow(ptr %p) #0 {
  %v = load <4 x i32>, ptr %p, align 1
  %q = getelementptr i8, ptr %p, i64 64
  store <4 x i32> %v, ptr %q, align 1
  call void @g()
  ret void
}
define void @unwinds(ptr %p) #0 personality ptr @pers {
  %v = load <4 x i64>, ptr %p, align 1
  %q = getelementptr i8, ptr %p, i64 64
  store <4 x i64> %v, ptr %q, align 1
  invoke void @g() to label %ok unwind label %pad
ok:
  %u = load <4 x i64>, ptr %p, align 1
  store <4 x i64> %u, ptr %q, align 1
  ret void
pad:
  %l = landingpad { ptr, i32 } cleanup
  %x = load <4 x i64>, ptr %p, align 1
  store <4 x i64> %x, ptr %q, align 1
  resume { ptr, i32 } %l
}
"#;

/// The machines, and how many functions write a `vzeroupper` for each.
const MACHINES: [(&str, &str); 9] = [
    ("\"target-cpu\"=\"x86-64\"", "none: no AVX"),
    ("\"target-cpu\"=\"x86-64\" \"target-features\"=\"+avx\"", "all"),
    ("\"target-cpu\"=\"haswell\"", "all"),
    ("\"target-cpu\"=\"znver3\"", "all"),
    ("\"target-cpu\"=\"skylake-avx512\"", "all"),
    ("\"target-cpu\"=\"btver2\"", "none: no tuning"),
    ("\"target-cpu\"=\"knl\"", "none: no tuning"),
    ("\"target-cpu\"=\"haswell\" \"target-features\"=\"-vzeroupper\"", "none: turned off"),
    ("\"target-cpu\"=\"btver2\" \"target-features\"=\"+vzeroupper\"", "all"),
];

/// Per function, in order: each call (`c`) and tail branch (`t`),
/// upper-case when a `vzeroupper` comes right before it, then `R` when
/// one comes before some return. hod has one shared `ret` after every
/// block (which a function ending in a tail branch keeps unused), `llc`
/// one per returning block (§8.7).
fn events(object: &Path) -> Vec<(String, String)> {
    let objdump = tool("llvm-objdump").expect("llvm-objdump");
    let mut command = Command::new(objdump);
    command.args(["-d", "-r", "--no-show-raw-insn"]).arg(object);
    let outcome = run_with(&mut command, &[], Duration::from_mins(1));
    assert!(outcome.success(), "llvm-objdump: {}", outcome.stderr_text());
    let mut out: Vec<(String, String)> = Vec::new();
    let mut zero = false;
    let mut jumped = false;
    for line in outcome.stdout_text().lines() {
        if let Some(name) = line.strip_suffix(">:").and_then(|l| l.split('<').nth(1)) {
            out.push((name.to_string(), String::new()));
            continue;
        }
        let Some((_, events)) = out.last_mut() else { continue };
        let words: Vec<&str> = line.split_whitespace().collect();
        let mnemonic = words.get(1).copied().unwrap_or("");
        if line.contains("R_X86_64_PLT32") {
            if jumped {
                events.push(if zero { 'T' } else { 't' });
                zero = false;
            }
            jumped = false;
            continue;
        }
        jumped = mnemonic == "jmp";
        match mnemonic {
            "vzeroupper" => zero = true,
            "callq" | "call" => {
                events.push(if zero { 'C' } else { 'c' });
                zero = false;
            }
            "retq" | "ret" => {
                if zero && !events.ends_with('R') {
                    events.push('R');
                }
                zero = false;
            }
            _ => {}
        }
    }
    for (_, events) in &mut out {
        if events.contains('R') {
            events.retain(|e| e != 'R');
            events.push('R');
        }
    }
    out
}

/// §8.7: hod writes `vzeroupper` where `llc` does, for every machine.
#[test]
fn vzeroupper_where_llc_writes_it() {
    let Some(_) = tool("llvm-objdump") else { return };
    let scratch = Scratch::new("x64-vzeroupper");
    for (index, (attrs, what)) in MACHINES.iter().enumerate() {
        let text = format!("{}{BODIES}\nattributes #0 = {{ {attrs} }}\n{PIE_FLAGS}", header());
        let (module, bodies) = parse(&text);
        let bytes = hod_x64::compile_to_object(&module, &bodies, &pic(Stress::default()))
            .unwrap_or_else(|e| panic!("{attrs}: {e:?}"));
        let ours = scratch.write(&format!("hod{index}.o"), bytes);
        let hod = events(&ours);
        let zeroed = hod.iter().filter(|(_, e)| e.chars().any(char::is_uppercase)).count();
        assert_eq!(zeroed == 0, what.starts_with("none"), "{attrs}: {hod:?}");
        let source = scratch.write(&format!("m{index}.ll"), &text);
        for major in [19, 22] {
            let Some(llc) = llc_of(major) else { continue };
            for fast in ["-fast-isel=1", "-fast-isel=0"] {
                let object = scratch.join(format!("llc{index}-{major}{fast}.o"));
                let mut command = Command::new(&llc);
                command
                    .args(["-O0", fast, "-relocation-model=pic", "-filetype=obj"])
                    .arg(&source)
                    .arg("-o")
                    .arg(&object);
                let outcome = run_with(&mut command, &[], Duration::from_mins(2));
                assert!(outcome.success(), "llc {major}: {}", outcome.stderr_text());
                let mut theirs = events(&object);
                if fast == "-fast-isel=0" {
                    // §8.7: `SelectionDAG` without AVX2 does integer
                    // arithmetic on 256-bit vectors in two `xmm` halves,
                    // loaded and stored as halves: no `ymm` at all.
                    let avx2 = ["haswell", "znver3", "skylake"].iter().any(|c| attrs.contains(c));
                    for (name, events) in &mut theirs {
                        if name == "arith" && !avx2 && *what == "all" {
                            *events = events.to_uppercase();
                        }
                    }
                }
                assert_eq!(hod, theirs, "llc {major} {fast} {attrs}");
            }
        }
        eprintln!("vzeroupper {attrs}: {zeroed} of {} functions write one", hod.len());
    }
}
