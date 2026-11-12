//! The differential harness (`spec/codegen.md` §12): a generated program
//! is run by the interpreter and, compiled for the test machine, by the
//! executor; the result and the trace of host calls must be equal.

use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::sync::atomic::Ordering;

use hod_cg::{CgError, Chunk, Options, Scratch, Stress, compile_function};
use hod_interp::generate::{Coverage, Options as GenOptions, generate};
use hod_interp::{Stop, Val};
use hod_ir::{Body, Module};

use super::exec;
use super::target::Tm;

/// No stress, each stress option alone, and all of them.
pub const MODES: [(&str, Stress); 5] = [
    ("plain", Stress { min_pool: false, far: false, displace: false }),
    ("min-pool", Stress { min_pool: true, far: false, displace: false }),
    ("far", Stress { min_pool: false, far: true, displace: false }),
    ("displace", Stress { min_pool: false, far: false, displace: true }),
    ("all", Stress::ALL),
];

/// The generator options the differential tests run over: the default,
/// the minimum, sizes, each feature off, and the minimum with groups of
/// features on.
pub fn option_sets() -> Vec<(String, GenOptions)> {
    let default = GenOptions::default;
    let minimal = GenOptions::minimal;
    let mut sets = vec![
        ("default".to_owned(), default()),
        ("minimal".to_owned(), minimal()),
        ("sized-8x30".to_owned(), default().sized(8, 30)),
        ("sized-2x200".to_owned(), default().sized(2, 200)),
        ("sized-12x100".to_owned(), default().sized(12, 100)),
        ("minimal-1x200".to_owned(), minimal().sized(1, 200)),
    ];
    macro_rules! off {
        ($($field:ident),+) => {
            $(sets.push((
                concat!("no-", stringify!($field)).to_owned(),
                GenOptions { $field: false, ..default() },
            ));)+
        };
    }
    off!(
        narrow_ints,
        odd_ints,
        i128,
        floats,
        compares,
        selects,
        casts,
        memory,
        globals,
        ptr_int,
        intrinsics,
        branches,
        loops,
        switches,
        unreachable_blocks,
        calls,
        indirect_calls,
        many_args,
        hosts
    );
    sets.push((
        "only-control".to_owned(),
        GenOptions { compares: true, branches: true, loops: true, switches: true, ..minimal() },
    ));
    sets.push((
        "only-calls".to_owned(),
        GenOptions { calls: true, many_args: true, hosts: true, functions: 6, ..minimal() },
    ));
    sets.push((
        "only-memory".to_owned(),
        GenOptions { memory: true, globals: true, casts: true, i128: true, ..minimal() },
    ));
    sets.push((
        "only-floats".to_owned(),
        GenOptions { floats: true, casts: true, compares: true, selects: true, ..minimal() },
    ));
    sets
}

/// What a batch of differential runs did.
#[derive(Debug, Default)]
pub struct Tally {
    /// Programs generated.
    pub generated: u64,
    /// Programs the interpreter could not run, by its reason.
    pub interp_unsupported: BTreeMap<String, u64>,
    /// Compilations attempted: one per program and stress mode.
    pub attempted: u64,
    /// Compilations the core or the target refused, by construct.
    pub rejected: BTreeMap<String, u64>,
    /// Compilations whose run was compared with the interpreter's, and
    /// was equal.
    pub compared: u64,
    /// Functions compiled, IR instructions in them, code bytes emitted.
    pub functions: u64,
    pub insts: u64,
    pub code_bytes: u64,
    /// Machine instructions executed and host calls compared.
    pub steps: u64,
    pub events: u64,
    /// What the generated programs contained.
    pub coverage: Coverage,
}

impl Tally {
    /// A report of the counts, one per line.
    pub fn report(&self, tm: &Tm) -> String {
        let mut text = String::new();
        let attempts = tm.attempts.load(Ordering::Relaxed);
        let far = tm.far_attempts.load(Ordering::Relaxed);
        let _ = writeln!(text, "programs generated:        {}", self.generated);
        let _ = writeln!(text, "interpreter unsupported:   {:?}", self.interp_unsupported);
        let _ = writeln!(text, "compilations attempted:    {}", self.attempted);
        let _ = writeln!(text, "compilations rejected:     {:?}", self.rejected);
        let _ = writeln!(text, "runs compared and equal:   {}", self.compared);
        let _ = writeln!(text, "functions compiled:        {}", self.functions);
        let _ = writeln!(text, "function attempts:         {attempts} ({far} in far mode)");
        let _ = writeln!(text, "IR instructions compiled:  {}", self.insts);
        let _ = writeln!(text, "code bytes emitted:        {}", self.code_bytes);
        let _ = writeln!(text, "machine steps executed:    {}", self.steps);
        let _ = writeln!(text, "host calls compared:       {}", self.events);
        text
    }
}

/// Compiles every body of a program into `chunk`, in order.
pub fn compile_all(
    tm: &Tm,
    module: &Module,
    bodies: &[Body],
    options: Options,
    scratch: &mut Scratch,
    chunk: &mut Chunk,
) -> Result<(), CgError> {
    bodies.iter().try_for_each(|body| compile_function(tm, module, body, &options, scratch, chunk))
}

/// A worker's state and the counts of what it did.
#[derive(Debug, Default)]
pub struct Harness {
    pub tm: Tm,
    pub scratch: Scratch,
    pub chunk: Chunk,
    pub tally: Tally,
}

impl Harness {
    pub fn new() -> Harness {
        Harness::default()
    }

    /// Generates the program of `seed` under `options`, runs it in the
    /// interpreter, and compares each of `modes` against that.
    ///
    /// # Errors
    /// A description of the first difference, or of what failed.
    pub fn check(
        &mut self,
        seed: u64,
        options: &GenOptions,
        modes: &[(&str, Stress)],
    ) -> Result<(), String> {
        let program = generate(seed, options);
        self.tally.generated += 1;
        self.tally.coverage.merge(&program.coverage);
        let outcome = program.run();
        let expected = match &outcome.result {
            Ok(Some(Val::Int { bits: 64, value })) => *value as u64,
            Err(Stop::Unsupported(reason)) => {
                *self.tally.interp_unsupported.entry(reason.clone()).or_default() += 1;
                return Ok(());
            }
            other => return Err(format!("the interpreter did not return an i64: {other:?}")),
        };
        let args: Vec<u64> = program.args.iter().filter_map(Val::as_u64).collect();
        for &(name, stress) in modes {
            self.tally.attempted += 1;
            self.chunk.clear();
            let compiled = compile_all(
                &self.tm,
                &program.module,
                &program.bodies,
                Options { stress },
                &mut self.scratch,
                &mut self.chunk,
            );
            match compiled {
                Ok(()) => {}
                Err(CgError::Unsupported { construct }) => {
                    *self.tally.rejected.entry(construct.to_owned()).or_default() += 1;
                    continue;
                }
                Err(error) => return Err(format!("mode {name}: {error}")),
            }
            let ran = exec::run(&program.module, &self.chunk, program.entry, &args)
                .map_err(|what| format!("mode {name}: the compiled program failed: {what}"))?;
            if ran.ret[0] != expected {
                return Err(format!(
                    "mode {name}: the compiled program returned {:#x}, the interpreter {expected:#x}",
                    ran.ret[0]
                ));
            }
            if ran.trace != outcome.trace {
                let at = ran.trace.iter().zip(&outcome.trace).take_while(|(a, b)| a == b).count();
                return Err(format!(
                    "mode {name}: the traces differ at host call {at}: compiled {:?}, interpreted {:?} \
                     ({} and {} calls)",
                    ran.trace.get(at),
                    outcome.trace.get(at),
                    ran.trace.len(),
                    outcome.trace.len()
                ));
            }
            self.tally.compared += 1;
            self.tally.functions += program.bodies.len() as u64;
            self.tally.insts += program.bodies.iter().map(|b| b.inst_count() as u64).sum::<u64>();
            self.tally.code_bytes += self.chunk.code.len() as u64;
            self.tally.steps += ran.steps;
            self.tally.events += ran.trace.len() as u64;
        }
        Ok(())
    }
}
