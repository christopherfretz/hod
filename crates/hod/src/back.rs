//! The back half of [`crate::compile`]: code generation for a module the
//! front half accepted whole (`spec/cli.md` "The back half").
//!
//! The bodies are compiled into [`Chunk`]s of consecutive functions, on
//! the calling thread or, for a module large enough to pay for them, on
//! worker threads (`std::thread::scope`), each worker with its own
//! [`Scratch`]. The chunks are merged in module order by `emit_module`,
//! whose result does not depend on how the functions were divided
//! (`spec/codegen.md` §9.7, §11): the object is the same for every thread
//! count. A refusal anywhere refuses the module, and the one reported is
//! the first in module order, whichever thread met it.

use std::fmt;
use std::ops::Range;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use hod_a64::features;
use hod_cg::{CgError, Chunk, Scratch, compile_function, emit_module};
use hod_ir::{Arch, Body, FnStr, Module, SymbolKind};

use crate::front::{Report, symbol_name};
use crate::{CodeModel, CompileError, Construct, Options, RelocModel, Unsupported};

/// Where extra worker threads come from: a GNU make jobserver in the
/// command-line tool (`spec/cli.md` "Threads").
pub trait ThreadTokens: Send + Sync + fmt::Debug {
    /// Takes up to `wanted` tokens without waiting; returns how many it
    /// got. Each lets one more thread run.
    fn take(&self, wanted: usize) -> usize;
    /// Gives back `count` tokens [`ThreadTokens::take`] returned.
    fn give_back(&self, count: usize);
}

/// How many threads code generation may use (`spec/cli.md` "Threads").
///
/// The default is one: the calling thread, no other.
#[derive(Clone, Debug, Default)]
pub struct Threads {
    /// The most threads, the calling one included; 0 is 1.
    pub max: usize,
    /// Where every thread beyond the first must come from. `None`: `max`
    /// is granted outright (an explicit setting).
    pub tokens: Option<Arc<dyn ThreadTokens>>,
    /// The work, in IR instructions, each thread needs; 0 is
    /// [`PARALLEL_MIN_INSTS`]. For measuring the break-even.
    pub per_thread: usize,
}

impl PartialEq for Threads {
    fn eq(&self, other: &Threads) -> bool {
        self.max == other.max
            && self.per_thread == other.per_thread
            && match (&self.tokens, &other.tokens) {
                (None, None) => true,
                (Some(a), Some(b)) => std::ptr::addr_eq(Arc::as_ptr(a), Arc::as_ptr(b)),
                _ => false,
            }
    }
}

impl Eq for Threads {}

/// The work, in IR instructions, each thread must have: a module of `n`
/// times this many uses at most `n` threads, so one below twice this is
/// compiled on the calling thread alone. A thread then has about 0.4 ms
/// of work against about 30 µs to start and join it (measured:
/// `spec/landed/cli/WP-043.md`).
pub const PARALLEL_MIN_INSTS: usize = 4_096;

/// How many chunks each worker gets, about: more than one, so that a
/// worker that drew short functions takes more.
const CHUNKS_PER_WORKER: usize = 4;

/// Tokens taken from a [`ThreadTokens`], given back when dropped (also
/// when a worker panics).
struct Lease<'a> {
    tokens: &'a dyn ThreadTokens,
    count: usize,
}

impl Drop for Lease<'_> {
    fn drop(&mut self) {
        if self.count > 0 {
            self.tokens.give_back(self.count);
        }
    }
}

/// How many threads compile `work` instructions in `bodies` functions.
fn wanted_threads(threads: &Threads, work: usize, bodies: usize) -> usize {
    let per_thread = if threads.per_thread == 0 { PARALLEL_MIN_INSTS } else { threads.per_thread };
    threads.max.min(bodies).min(work / per_thread).max(1)
}

/// The bodies cut into runs of consecutive functions of about `per_chunk`
/// instructions each (at least one function each).
fn plan(bodies: &[Body], per_chunk: usize) -> Vec<Range<usize>> {
    let mut chunks = Vec::new();
    let mut start = 0;
    let mut size = 0;
    for (index, body) in bodies.iter().enumerate() {
        size += body.inst_count();
        if size >= per_chunk {
            chunks.push(start..index + 1);
            start = index + 1;
            size = 0;
        }
    }
    if start < bodies.len() {
        chunks.push(start..bodies.len());
    }
    chunks
}

/// Why a function did not compile.
enum Failure {
    /// A target feature it is compiled with.
    Feature { reason: &'static str, feature: String },
    /// Code generation's own.
    Cg(CgError),
}

/// One function: its target features, then its code.
fn compile_one(
    module: &Module,
    body: &Body,
    options: hod_cg::Options,
    scratch: &mut Scratch,
    chunk: &mut Chunk,
) -> Result<(), Failure> {
    if let SymbolKind::Function { attrs, .. } = module.symbol(body.symbol()).kind()
        && let Some(string) = module.fn_attrs(attrs).string(FnStr::TargetFeatures)
    {
        let string = String::from_utf8_lossy(string);
        if let Err(refused) = features::check(&string) {
            let feature = refused.feature.to_owned();
            return Err(Failure::Feature { reason: refused.reason, feature });
        }
    }
    compile_function(&hod_a64::A64, module, body, &options, scratch, chunk).map_err(Failure::Cg)
}

/// What one worker made: chunk index, then the chunk or the first
/// function of it that failed.
type Made = Vec<(usize, Result<Chunk, (usize, Failure)>)>;

/// A worker: takes the next chunk until none is left or a function before
/// it failed.
fn work(
    module: &Module,
    bodies: &[Body],
    options: hod_cg::Options,
    chunks: &[Range<usize>],
    next: &AtomicUsize,
    first_failure: &AtomicUsize,
) -> Made {
    let mut scratch = Scratch::new();
    let mut made = Vec::new();
    loop {
        let index = next.fetch_add(1, Ordering::Relaxed);
        // Chunks are taken in module order: once one starts after a
        // failure, every later one does.
        let Some(range) = chunks.get(index) else { break };
        if range.start > first_failure.load(Ordering::Relaxed) {
            break;
        }
        let mut chunk = Chunk::new();
        let mut result = Ok(());
        for at in range.clone() {
            // A function after a known failure cannot be the answer.
            if at > first_failure.load(Ordering::Relaxed) {
                break;
            }
            if let Err(failure) =
                compile_one(module, &bodies[at], options, &mut scratch, &mut chunk)
            {
                first_failure.fetch_min(at, Ordering::Relaxed);
                result = Err((at, failure));
                break;
            }
        }
        made.push((index, result.map(|()| chunk)));
    }
    made
}

/// Compiles every body into chunks in module order, on `workers` threads.
///
/// # Errors
/// The first function in module order that failed, with its index.
fn compile_bodies(
    module: &Module,
    bodies: &[Body],
    options: hod_cg::Options,
    workers: usize,
    work_size: usize,
) -> Result<Vec<Chunk>, (usize, Failure)> {
    let chunks = if workers <= 1 {
        std::iter::once(0..bodies.len()).collect()
    } else {
        plan(bodies, (work_size / (workers * CHUNKS_PER_WORKER)).max(1))
    };
    let next = AtomicUsize::new(0);
    let first_failure = AtomicUsize::new(usize::MAX);
    let run = || work(module, bodies, options, &chunks, &next, &first_failure);
    let mut made = if workers <= 1 {
        run()
    } else {
        std::thread::scope(|scope| {
            let helpers: Vec<_> = (1..workers).map(|_| scope.spawn(run)).collect();
            let mut made = run();
            for helper in helpers {
                match helper.join() {
                    Ok(more) => made.extend(more),
                    Err(panic) => std::panic::resume_unwind(panic),
                }
            }
            made
        })
    };
    made.sort_unstable_by_key(|&(index, _)| index);
    let mut out = Vec::with_capacity(made.len());
    let mut failed: Option<(usize, Failure)> = None;
    for (_, result) in made {
        match result {
            Ok(chunk) => out.push(chunk),
            Err((at, failure)) => {
                if failed.as_ref().is_none_or(|&(first, _)| at < first) {
                    failed = Some((at, failure));
                }
            }
        }
    }
    match failed {
        Some(failure) => Err(failure),
        None => Ok(out),
    }
}

/// A code-generation error as a decline (`spec/cli.md` "The back half"):
/// `Unsupported` is a reason, `Internal` an internal error.
fn from_cg(error: CgError, function: Option<String>) -> CompileError {
    match error {
        CgError::Unsupported { construct } => {
            Unsupported { construct: Construct::Named(construct.into()), function, detail: None }
                .into()
        }
        CgError::Internal(text) => CompileError::Internal(match function {
            Some(function) => format!("function '{function}': {text}"),
            None => format!("module emission: {text}"),
        }),
    }
}

/// The object writer's error: a name ELF cannot hold and a format limit
/// are reasons, anything else is an internal error.
fn from_obj(error: hod_obj::Error) -> CompileError {
    let reason = match error {
        hod_obj::Error::NulInName => OBJECT_NUL_IN_NAME,
        hod_obj::Error::TooLarge(_) => LIMIT_OBJECT,
        error => return CompileError::Internal(format!("object: {error}")),
    };
    Unsupported::module(Construct::Named(reason.into())).into()
}

/// A symbol name with a NUL byte, which ELF's string table cannot hold.
pub const OBJECT_NUL_IN_NAME: &str = "object:nul-in-name";
/// A count or size the object format cannot hold.
pub const LIMIT_OBJECT: &str = "limit:object";
/// A `-relocation-model` of 32-bit ARM (`ropi`, `rwpi`, `ropi-rwpi`).
pub const OPTION_RELOCATION_MODEL: &str = "option:relocation-model";
/// `-split-dwarf-output`: a `.dwo` file to write, which hod does not.
pub const OPTION_SPLIT_DWARF: &str = "option:split-dwarf-output";

/// `llc`'s options as code generation takes them.
fn cg_options(options: &Options) -> Result<hod_cg::Options, CompileError> {
    let option =
        |name: &'static str| Err(Unsupported::module(Construct::Named(name.into())).into());
    let reloc_model = match options.relocation_model {
        None | Some(RelocModel::Static) => hod_cg::RelocModel::Static,
        Some(RelocModel::Pic) => hod_cg::RelocModel::Pic,
        Some(RelocModel::DynamicNoPic) => hod_cg::RelocModel::DynamicNoPic,
        Some(RelocModel::Ropi | RelocModel::Rwpi | RelocModel::RopiRwpi) => {
            return option(OPTION_RELOCATION_MODEL);
        }
    };
    if options.split_dwarf_output.is_some() {
        return option(OPTION_SPLIT_DWARF);
    }
    let code_model = options.code_model.map(|model| match model {
        CodeModel::Tiny => hod_ir::CodeModel::Tiny,
        CodeModel::Small => hod_ir::CodeModel::Small,
        CodeModel::Kernel => hod_ir::CodeModel::Kernel,
        CodeModel::Medium => hod_ir::CodeModel::Medium,
        CodeModel::Large => hod_ir::CodeModel::Large,
    });
    Ok(hod_cg::Options {
        reloc_model,
        code_model,
        function_sections: options.function_sections,
        data_sections: options.data_sections,
        stress: hod_cg::Stress::default(),
    })
}

/// [`crate::generate`] with the threads it may use and the report its
/// phases go to.
///
/// # Errors
/// As [`crate::generate`].
pub fn generate_with(
    module: &Module,
    bodies: &[Body],
    options: &Options,
    threads: &Threads,
    report: &mut Report,
) -> Result<Vec<u8>, CompileError> {
    if module.target().arch() == Arch::X86_64 {
        return Err(Unsupported::module(Construct::CodegenNotImplemented).into());
    }
    let cg = cg_options(options)?;
    // `-mattr` applies to every function.
    for item in &options.features {
        if let Err(refused) = features::check_one(item) {
            return Err(Unsupported {
                construct: Construct::Named(refused.reason.into()),
                function: None,
                detail: Some(item.clone()),
            }
            .into());
        }
    }

    let work_size: usize = bodies.iter().map(Body::inst_count).sum();
    let wanted = wanted_threads(threads, work_size, bodies.len());
    let lease = match (&threads.tokens, wanted) {
        (Some(tokens), 2..) => Some(Lease { tokens: &**tokens, count: tokens.take(wanted - 1) }),
        _ => None,
    };
    let workers = lease.as_ref().map_or(wanted, |lease| 1 + lease.count);
    let compiled = compile_bodies(module, bodies, cg, workers, work_size);
    drop(lease);
    let chunks = compiled.map_err(|(at, failure)| {
        let function = symbol_name(module, bodies[at].symbol());
        match failure {
            Failure::Feature { reason, feature } => Unsupported {
                construct: Construct::Named(reason.into()),
                function,
                detail: Some(feature),
            }
            .into(),
            Failure::Cg(error) => from_cg(error, function),
        }
    })?;
    report.phase("emission");
    let emitted = emit_module(module, &cg, &chunks).map_err(|error| from_cg(error, None))?;
    drop(chunks);
    let object = hod_obj::elf::to_vec(&emitted.object).map_err(from_obj)?;
    report.phase("object");
    Ok(object)
}

#[cfg(test)]
mod tests {
    use super::*;

    // spec/cli.md "Threads": a module below the break-even is not worth a
    // thread; above it, one per `PARALLEL_MIN_INSTS`, at most `max` and
    // at most one per function.
    #[test]
    fn threads_wanted() {
        let max = |max| Threads { max, ..Threads::default() };
        assert_eq!(wanted_threads(&Threads::default(), 1 << 30, 100), 1);
        assert_eq!(wanted_threads(&max(8), PARALLEL_MIN_INSTS - 1, 100), 1);
        assert_eq!(wanted_threads(&max(8), 2 * PARALLEL_MIN_INSTS, 100), 2);
        assert_eq!(wanted_threads(&max(8), 100 * PARALLEL_MIN_INSTS, 100), 8);
        assert_eq!(wanted_threads(&max(8), 100 * PARALLEL_MIN_INSTS, 3), 3);
        assert_eq!(wanted_threads(&max(0), 100 * PARALLEL_MIN_INSTS, 3), 1);
    }

    #[test]
    fn options_for_code_generation() {
        let ropi = Options { relocation_model: Some(RelocModel::Ropi), ..Options::default() };
        let refused = |name: &'static str| {
            Err::<hod_cg::Options, _>(CompileError::from(Unsupported::module(Construct::Named(
                name.into(),
            ))))
        };
        assert_eq!(cg_options(&ropi), refused(OPTION_RELOCATION_MODEL));
        let dwo = Options { split_dwarf_output: Some("a.dwo".into()), ..Options::default() };
        assert_eq!(cg_options(&dwo), refused(OPTION_SPLIT_DWARF));
        let pic = Options {
            relocation_model: Some(RelocModel::Pic),
            code_model: Some(CodeModel::Large),
            function_sections: true,
            ..Options::default()
        };
        let cg = cg_options(&pic).unwrap();
        assert_eq!(cg.reloc_model, hod_cg::RelocModel::Pic);
        assert_eq!(cg.code_model, Some(hod_ir::CodeModel::Large));
        assert!(cg.function_sections && !cg.data_sections);
        assert_eq!(
            cg_options(&Options::default()).unwrap().reloc_model,
            hod_cg::RelocModel::Static
        );
    }

    #[test]
    fn errors_of_the_back_half() {
        let named = |name: &'static str, function: Option<&str>| {
            CompileError::from(Unsupported {
                construct: Construct::Named(name.into()),
                function: function.map(str::to_owned),
                detail: None,
            })
        };
        let limit = CgError::Unsupported { construct: hod_cg::LIMIT_FRAME_SIZE };
        assert_eq!(from_cg(limit, Some("f".into())), named("limit:frame-size", Some("f")));
        assert_eq!(
            from_cg(CgError::internal("oops"), Some("f".into())),
            CompileError::Internal("function 'f': oops".into())
        );
        assert_eq!(
            from_cg(CgError::internal("oops"), None),
            CompileError::Internal("module emission: oops".into())
        );
        assert_eq!(from_obj(hod_obj::Error::NulInName), named(OBJECT_NUL_IN_NAME, None));
        assert!(matches!(
            from_obj(hod_obj::Error::BadAlignment { align: 3 }),
            CompileError::Internal(_)
        ));
    }
}
