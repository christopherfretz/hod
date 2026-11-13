//! Builds a batch of cases against one `llc`, runs it (under
//! `qemu-x86_64` on another host) and
//! compares what each side received (`spec/abi-x64.md` §11).

use std::collections::{BTreeMap, HashSet};
use std::ffi::OsString;
use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

use hod_cg::abi::Place;
use hod_testutil::{Outcome, Target, run, run_with, tools_required};

use crate::emit::{self, Ext, Func, Item};
use crate::ir;
use crate::sigs::{Attr, Blobs, Case, Plan, Slot, assumed_bits, produced_bits, table};
use crate::ty::Rng;

/// One `llc`: its major version and its options.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Config {
    pub major: u32,
    pub opt: &'static str,
    /// `-fast-isel=0`: `SelectionDAG` at `-O0`.
    pub dag: bool,
}

/// Every `llc` the grid runs against. `-O0` selects instructions with
/// `FastISel`, which hands calls it cannot lower to `SelectionDAG`;
/// `-O0 -fast-isel=0` and `-O2` use `SelectionDAG` throughout. The two
/// lower calls with separate code.
impl Config {
    pub fn tag(self) -> String {
        format!("{}{}{}", self.major, self.opt, if self.dag { "-dag" } else { "" })
    }
}

pub const CONFIGS: [Config; 6] = [
    Config { major: 22, opt: "-O0", dag: true },
    Config { major: 22, opt: "-O0", dag: false },
    Config { major: 22, opt: "-O2", dag: false },
    Config { major: 19, opt: "-O0", dag: true },
    Config { major: 19, opt: "-O0", dag: false },
    Config { major: 19, opt: "-O2", dag: false },
];

/// The tool `name` of LLVM `major`, or `None` when it is not installed;
/// that fails the test under `HOD_REQUIRE_TOOLS=1`.
pub fn llvm_tool(major: u32, name: &str) -> Option<PathBuf> {
    let installed = PathBuf::from(format!("/usr/lib/llvm-{major}/bin/{name}"));
    let found = if installed.is_file() {
        Some(installed)
    } else {
        let versioned = format!("{name}-{major}");
        let path = std::env::var_os("PATH").unwrap_or_default();
        std::env::split_paths(&path).map(|dir| dir.join(&versioned)).find(|file| file.is_file())
    };
    if found.is_none() {
        assert!(
            !tools_required(),
            "required tool `{name}` {major} not found (HOD_REQUIRE_TOOLS=1)"
        );
        eprintln!("note: `{name}` {major} not found; skipping the tool-backed part of this test");
    }
    found
}

/// The tools a batch needs. The C driver, the system root and the
/// emulator are WP-025's (`hod_testutil::Target`, `spec/harness.md`
/// "Cross runs"): programs run under `qemu-x86_64` with the baseline CPU
/// model, which faults on an instruction above SSE2.
#[derive(Clone, Debug)]
pub struct Tools {
    /// `clang` 22, which assembles and compiles for x86-64 whatever the
    /// host.
    pub assembler: PathBuf,
    /// A C compiler driver for x86-64 Linux that links.
    pub gcc: PathBuf,
    /// What runs an x86-64 program: nothing on an x86-64 host, else
    /// `qemu-x86_64 -cpu <baseline> -L <system root>`.
    pub runner: Vec<OsString>,
    pub llc: Vec<(Config, PathBuf)>,
}

impl Tools {
    /// The tools, or `None` when one is missing.
    pub fn find() -> Option<Tools> {
        let assembler = llvm_tool(22, "clang")?;
        let target = Target::X86_64;
        let gcc = target.tool("gcc")?;
        let mut runner = Vec::new();
        if !target.is_host() {
            let sysroot = target.tool("sysroot")?;
            runner.push(target.tool("emulator")?.into_os_string());
            runner.extend(target.emulator_args(&sysroot));
        }
        let mut llc = Vec::new();
        for config in CONFIGS {
            llc.push((config, llvm_tool(config.major, "llc")?));
        }
        Some(Tools { assembler, gcc, runner, llc })
    }

    /// A command that runs the x86-64 program `exe`.
    pub fn command(&self, exe: &Path) -> Command {
        match self.runner.split_first() {
            None => Command::new(exe),
            Some((first, rest)) => {
                let mut command = Command::new(first);
                command.args(rest).arg(exe);
                command
            }
        }
    }
}

/// The target `clang` assembles and compiles for.
pub const TARGET: &str = "--target=x86_64-unknown-linux-gnu";

/// A scratch directory in the build tree, removed on drop.
pub use hod_testutil::Scratch;

/// Runs a tool that must succeed.
pub fn must(what: &str, outcome: &Outcome) {
    assert!(outcome.success(), "{what} failed:\n{}", tail(&outcome.stderr_text()));
}

pub fn tail(text: &str) -> String {
    let lines: Vec<&str> = text.lines().collect();
    lines[lines.len().saturating_sub(30)..].join("\n")
}

/// What a batch found.
#[derive(Clone, Default, Debug)]
pub struct Report {
    /// The cases run with hod calling and with LLVM calling, for each
    /// configuration (`Config::tag`).
    pub runs: BTreeMap<String, [usize; 2]>,
    /// Functions `llc` itself crashed on, which no case could then check.
    pub llc_crashes: Vec<String>,
    pub failures: Vec<String>,
}

impl Report {
    pub fn add(&mut self, other: Report) {
        for (tag, [hod, llvm]) in other.runs {
            let runs = self.runs.entry(tag).or_default();
            runs[0] += hod;
            runs[1] += llvm;
        }
        self.llc_crashes.extend(other.llc_crashes);
        self.failures.extend(other.failures);
    }

    /// Counts one run under the configuration `tag`.
    pub fn count(&mut self, tag: &str, hod_calls: bool) {
        self.runs.entry(tag.to_string()).or_default()[usize::from(!hod_calls)] += 1;
    }

    /// The runs, as "llc 22-O0: 10 hod calls, 12 LLVM calls; …".
    pub fn summary(&self) -> String {
        let one = |(tag, [hod, llvm]): (&String, &[usize; 2])| {
            format!("llc {tag}: {hod} hod calls, {llvm} LLVM calls")
        };
        self.runs.iter().map(one).collect::<Vec<_>>().join("; ")
    }

    /// Fails the test if any case failed.
    pub fn check(&self, what: &str) {
        if let Some(path) = std::env::var_os("HOD_ABI_FAILURES") {
            // For whoever is investigating: every failure, not the first.
            let _ = std::fs::write(path, self.failures.join("\n"));
        }
        let shown: Vec<&str> = self.failures.iter().take(6).map(String::as_str).collect();
        assert!(
            self.failures.is_empty(),
            "{what}: {} failure(s); the first:\n{}",
            self.failures.len(),
            shown.join("\n")
        );
    }
}

/// Whether a case runs against a configuration, with hod calling or with
/// LLVM calling.
fn runs(plan: &Plan, config: Config, hod_calls: bool) -> bool {
    // LLVM 19 keeps an integer wider than 64 bits (and a vector of `i64`
    // split into general pieces) all in registers or all on the stack,
    // and then gives up the registers left (§4.2).
    if config.major < 22
        && plan.case.has_wide_general()
        && (plan.asg.stack_size > 0 || plan.case.fixed.is_some())
    {
        return false;
    }
    // LLVM has no `va_arg` for this target: a variadic callee is written
    // in C (see `variadic.rs`).
    !(hod_calls && plan.case.fixed.is_some())
}

pub fn item(slot: &Slot, ret: bool, sret: bool) -> Item {
    let ext = slot.int.and_then(|bits| {
        let (signed, to) = produced_bits(bits, slot.ext, ret)?;
        Some(Ext { bits, signed, to })
    });
    Item {
        src: slot.dirty,
        memory: slot.memory,
        dst: slot.out,
        size: slot.image.size as u64,
        bytes: slot.value_bytes(),
        int: slot.int.is_some(),
        ext,
        sret,
    }
}

const DRIVER: &str = r#"
#include <stdio.h>
#include <stdlib.h>
#include <unistd.h>

unsigned char hodabi_in[IN_SIZE] __attribute__((aligned(16)));
unsigned char hodabi_outa[OUT_SIZE] __attribute__((aligned(16)));
unsigned char hodabi_outb[OUT_SIZE] __attribute__((aligned(16)));

typedef void step(void);
STEPS

int main(int argc, char **argv) {
    unsigned from = strtoul(argv[1], 0, 10), to = strtoul(argv[2], 0, 10);
    if (fread(hodabi_in, 1, IN_SIZE, stdin) != IN_SIZE) return 2;
    /* argv[3], when given: a '1' for each step to run, a '0' for each
       step whose LLVM side is a stub. */
    const char *wanted = argc > 3 ? argv[3] : 0;
    for (unsigned i = from; i < to; i++) {
        if (wanted && wanted[i] != '1') continue;
        fprintf(stderr, "@step %u\n", i);
        steps[i]();
    }
    fwrite(hodabi_outa, 1, OUT_SIZE, stdout);
    fwrite(hodabi_outb, 1, OUT_SIZE, stdout);
    return 0;
}
"#;

/// The hod side of a batch and its driver, built once and linked against
/// each LLVM side: the blobs, and one step per case and direction (the
/// function `<a><index>` in which hod calls, then `<b><index>` in which
/// LLVM calls).
#[derive(Debug)]
pub struct Harness {
    pub scratch: Scratch,
    pub blobs: Blobs,
    cases: usize,
}

impl Harness {
    /// Assembles `asm` and compiles the driver for `cases` cases.
    ///
    /// # Panics
    /// Panics if the assembler or the compiler refuses its file.
    pub fn new(label: &str, mut blobs: Blobs, cases: usize, asm: &str, tools: &Tools) -> Harness {
        // Never an empty array in C.
        blobs.input.resize(blobs.input.len().max(16), 0);
        blobs.out_size = blobs.out_size.max(16);
        let mut steps = String::from("step ");
        let mut table = String::from("static step *const steps[] = {");
        for index in 0..cases {
            let _ = write!(steps, "hc_{index}, lc_{index}, ");
            let _ = write!(table, "hc_{index}, lc_{index}, ");
        }
        steps.push_str("hodabi_unused;\n");
        table.push_str("};\n");
        let driver = DRIVER
            .replace("IN_SIZE", &blobs.input.len().to_string())
            .replace("OUT_SIZE", &blobs.out_size.to_string())
            .replace("STEPS", &(steps + &table));

        let scratch = Scratch::new(label);
        let asm = scratch.write("hod.s", asm);
        let driver = scratch.write("driver.c", driver);
        let assembled = run(
            &tools.assembler,
            [
                TARGET.as_ref(),
                "-c".as_ref(),
                asm.as_os_str(),
                "-o".as_ref(),
                scratch.join("hod.o").as_os_str(),
            ],
        );
        must("assembling the hod side", &assembled);
        let compiled = run(
            &tools.gcc,
            [
                "-O1".as_ref(),
                "-c".as_ref(),
                driver.as_os_str(),
                "-o".as_ref(),
                scratch.join("driver.o").as_os_str(),
            ],
        );
        must("compiling the driver", &compiled);
        Harness { scratch, blobs, cases }
    }

    /// Links the hod side with `objects`, runs every step, and returns
    /// the two output blobs and which steps crashed. A step that crashes
    /// is left out and the steps around it run again.
    ///
    /// # Errors
    /// A description and the steps that crashed, when more steps crash
    /// than is worth isolating.
    pub fn execute(
        &self,
        tag: &str,
        objects: &[PathBuf],
        wanted: &[bool],
        tools: &Tools,
    ) -> Result<Ran, (String, Vec<bool>)> {
        let exe = self.scratch.join(format!("run{tag}"));
        let mut link = Command::new(&tools.gcc);
        link.arg(self.scratch.join("driver.o")).arg(self.scratch.join("hod.o"));
        link.args(objects).arg("-o").arg(&exe);
        must("linking", &run_with(&mut link, &[], hod_testutil::DEFAULT_TIMEOUT));

        let steps = self.cases * 2;
        assert_eq!(wanted.len(), steps, "one flag per step");
        let wanted: String = wanted.iter().map(|&run| if run { '1' } else { '0' }).collect();
        let size = self.blobs.out_size as usize;
        let mut out = vec![0u8; 2 * size];
        let mut crashed = vec![false; steps];
        let mut ranges = vec![(0, steps)];
        let mut crashes = 0;
        while let Some((from, to)) = ranges.pop() {
            if from >= to {
                continue;
            }
            let outcome = run_with(
                tools.command(&exe).arg(from.to_string()).arg(to.to_string()).arg(&wanted),
                &self.blobs.input,
                Duration::from_mins(2),
            );
            if outcome.success() && outcome.stdout.len() == 2 * size {
                // Each run starts from zeroed blobs and writes only its
                // own steps' bytes.
                for (byte, new) in out.iter_mut().zip(&outcome.stdout) {
                    *byte |= new;
                }
                continue;
            }
            let stderr = outcome.stderr_text();
            let step = stderr
                .rsplit("@step ")
                .next()
                .and_then(|rest| rest.split_whitespace().next()?.parse::<usize>().ok())
                .unwrap_or(from)
                .clamp(from, to - 1);
            crashed[step] = true;
            crashes += 1;
            if crashes > 16 {
                return Err(("more than 16 steps crash".to_string(), crashed));
            }
            ranges.push((from, step));
            ranges.push((step + 1, to));
        }
        Ok((out, crashed))
    }
}

/// What [`Harness::execute`] returns: the two output blobs, and which
/// steps crashed.
pub type Ran = (Vec<u8>, Vec<bool>);

/// Runs `llc` on `source`.
pub fn compile(llc: &Path, config: Config, source: &Path, object: &Path) -> Outcome {
    run(
        llc,
        [
            config.opt.as_ref(),
            if config.dag { "-fast-isel=0" } else { "-fast-isel=1" }.as_ref(),
            "-filetype=obj".as_ref(),
            "-relocation-model=pic".as_ref(),
            source.as_os_str(),
            "-o".as_ref(),
            object.as_os_str(),
        ],
    )
}

/// The cases of one batch with their bytes.
#[derive(Debug)]
pub struct Batch {
    pub plans: Vec<Plan>,
    harness: Harness,
}

impl Batch {
    /// Plans `cases` with values drawn from `seed` and assembles the hod
    /// side.
    ///
    /// # Panics
    /// Panics if a case is unsupported or the assembler refuses the file.
    pub fn new(label: &str, cases: &[Case], seed: u64, tools: &Tools) -> Batch {
        let mut types = table();
        let mut blobs = Blobs::default();
        let mut rng = Rng(seed);
        let plans: Vec<Plan> =
            cases.iter().map(|case| Plan::new(case, &mut types, &mut blobs, &mut rng)).collect();

        let mut asm = String::new();
        emit::prelude(&mut asm);
        for (index, plan) in plans.iter().enumerate() {
            let args: Vec<Item> = plan
                .args
                .iter()
                .zip(&plan.case.params)
                .map(|(slot, param)| item(slot, false, param.attr == Attr::Sret))
                .collect();
            let ret = plan.ret.as_ref().map(|slot| item(slot, true, false));
            let names = [format!("hc_{index}"), format!("lf_{index}"), format!("hf_{index}")];
            let func = Func {
                name: &names[0],
                peer: &names[1],
                out: ir::OUT_A,
                asg: &plan.asg,
                args: &args,
                ret,
                variadic: plan.case.fixed.is_some(),
            };
            emit::caller(&mut asm, func);
            let callee =
                Func { name: &names[2], peer: "", out: ir::OUT_B, variadic: false, ..func };
            emit::callee(&mut asm, callee);
        }
        emit::epilogue(&mut asm);
        let harness = Harness::new(label, blobs, plans.len(), &asm, tools);
        Batch { plans, harness }
    }

    /// Compiles the LLVM side with `llc`, links, runs and compares.
    pub fn run(&self, config: Config, llc: &Path, tools: &Tools) -> Report {
        let mut report = Report::default();
        let tag = config.tag();
        let scratch = &self.harness.scratch;
        let object = scratch.join(format!("llvm{tag}.o"));
        // A function `llc` crashes on says nothing about the convention:
        // it is left out, reported, and the rest of the batch runs.
        let mut left_out: HashSet<(usize, bool)> = HashSet::new();
        let runs = |left_out: &HashSet<(usize, bool)>, index: usize, hod_calls: bool| {
            runs(&self.plans[index], config, hod_calls) && !left_out.contains(&(index, hod_calls))
        };
        loop {
            let module =
                ir::module(&self.plans, &|index, hod_calls| runs(&left_out, index, hod_calls));
            let source = scratch.write(&format!("llvm{tag}.ll"), module);
            let compiled = compile(llc, config, &source, &object);
            if compiled.success() {
                break;
            }
            let stderr = compiled.stderr_text();
            let culprit = stderr.split("on function '@l").nth(1).and_then(|rest| {
                let hod_calls = rest.starts_with('f');
                let digits: String = rest[1..]
                    .trim_start_matches('_')
                    .chars()
                    .take_while(char::is_ascii_digit)
                    .collect();
                Some((digits.parse::<usize>().ok()?, hod_calls))
            });
            match culprit {
                Some(culprit) if left_out.len() < 32 && left_out.insert(culprit) => {
                    report.llc_crashes.push(format!(
                        "[llc {tag}] `{}` ({})",
                        self.plans[culprit.0].case.text(),
                        if culprit.1 { "as callee" } else { "as caller" }
                    ));
                }
                _ => {
                    report
                        .failures
                        .push(format!("[llc {tag}] llc refused the batch:\n{}", tail(&stderr)));
                    return report;
                }
            }
        }
        // Step `2i` is hod calling case `i`, `2i + 1` LLVM calling it.
        let wanted: Vec<bool> = (0..self.plans.len() * 2)
            .map(|step| runs(&left_out, step / 2, step % 2 == 0))
            .collect();
        let (out, crashed) = match self.harness.execute(&tag, &[object], &wanted, tools) {
            Ok(ran) => ran,
            Err(problem) => {
                // Name the cases of the steps that crashed (step `2i` is
                // hod calling case `i`, `2i + 1` LLVM calling it).
                let (mut problem, crashed) = problem;
                for (step, _) in crashed.iter().enumerate().filter(|(_, crashed)| **crashed) {
                    let case = &self.plans[step / 2].case;
                    let side = if step % 2 == 0 { "hod calls LLVM" } else { "LLVM calls hod" };
                    let _ = write!(problem, "\n    case {}, {side}: `{}`", step / 2, case.text());
                }
                report.failures.push(format!("[llc {tag}] {problem}"));
                return report;
            }
        };
        let size = self.harness.blobs.out_size as usize;

        for (index, plan) in self.plans.iter().enumerate() {
            for hod_calls in [true, false] {
                if !runs(&left_out, index, hod_calls) {
                    continue;
                }
                report.count(&tag, hod_calls);
                let blob = if hod_calls { &out[..size] } else { &out[size..] };
                let problem = if crashed[index * 2 + usize::from(!hod_calls)] {
                    Some("the step crashed".to_string())
                } else {
                    compare(plan, hod_calls, blob)
                };
                if let Some(problem) = problem {
                    let side = if hod_calls { "hod calls LLVM" } else { "LLVM calls hod" };
                    report.failures.push(format!(
                        "[llc {tag}] case {index}, {side}: `{}`: {problem}\n    {:?}",
                        plan.case.text(),
                        plan.asg
                    ));
                }
            }
        }
        report
    }
}

/// The first difference between what `plan` expects and `out`.
pub fn compare(plan: &Plan, hod_calls: bool, out: &[u8]) -> Option<String> {
    for (index, slot) in plan.args.iter().enumerate() {
        // The callee consumes arguments: LLVM when hod calls.
        let place = plan.parts_of(index).last().map(|part| part.place);
        if let Some(problem) = compare_slot(slot, false, hod_calls, place, out) {
            return Some(format!("argument {index}: {problem}"));
        }
    }
    let slot = plan.ret.as_ref()?;
    // The caller consumes the return value: LLVM when LLVM calls.
    let place = plan.asg.ret.last().map(|part| part.place);
    compare_slot(slot, true, !hod_calls, place, out)
        .map(|problem| format!("return value: {problem}"))
}

/// Compares one slot. `llvm_consumes` tells which side recorded it; `place`
/// is where its last part travelled.
fn compare_slot(
    slot: &Slot,
    ret: bool,
    llvm_consumes: bool,
    place: Option<Place>,
    out: &[u8],
) -> Option<String> {
    let (want, mask) = match slot.int {
        // LLVM extended what it received, by sign under `signext` and by
        // zero otherwise, relying on the producer as far as it does: every
        // bit must be right.
        Some(bits) if llvm_consumes => {
            slot.image.extended(bits, slot.ext == Some(true), ir::recorded_bits(bits, slot.ext))
        }
        // hod recorded the whole unit: what §7 lets a consumer assume
        // must be there.
        Some(bits) => {
            let place = place.unwrap_or(Place::Stack(0));
            let (signed, to) = assumed_bits(bits, slot.ext, ret, place);
            slot.image.extended(bits, signed, to)
        }
        // LLVM stored an aggregate member by member, an integer as its
        // store-size bytes: a false belief about the bits above a
        // member's width (§7 gives it none) would show in those bytes.
        None if llvm_consumes => {
            let whole = |&bits: &u8| if bits == 0 { 0 } else { 0xff };
            (slot.image.clean.clone(), slot.image.mask.iter().map(whole).collect())
        }
        None => (slot.image.clean.clone(), slot.image.mask.clone()),
    };
    let got = &out[slot.out as usize..][..want.len()];
    let differs = |index: usize| (got[index] ^ want[index]) & mask[index] != 0;
    let first = (0..want.len()).find(|&index| differs(index))?;
    let hex = |bytes: &[u8]| {
        bytes.iter().fold(String::new(), |mut text, byte| {
            let _ = write!(text, "{byte:02x}");
            text
        })
    };
    let end = want.len().min(first + 16);
    Some(format!(
        "byte {first}: want {} got {} (mask {})",
        hex(&want[first..end]),
        hex(&got[first..end]),
        hex(&mask[first..end])
    ))
}
