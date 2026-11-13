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

use hod_cg::{CgError, Chunk, ModuleEmitter, Scratch, compile_function};
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

/// What the `target-cpu` and `target-features` of one function attribute
/// set decide, worked out once per set for the module (WP-166): the
/// first item refused, and on x86-64 the target its functions are
/// compiled with.
struct Group {
    refused: Option<(&'static str, String)>,
    x64: hod_x64::X64,
}

/// The [`Group`] of each attribute set a body's function has, by
/// `FnAttrsId` index; `None` for a set no body uses.
fn groups(module: &Module, bodies: &[Body]) -> Vec<Option<Group>> {
    let arch = module.target().arch();
    let mut groups: Vec<Option<Group>> = Vec::new();
    for body in bodies {
        let SymbolKind::Function { attrs: id, .. } = module.symbol(body.symbol()).kind() else {
            continue;
        };
        let index = id.index();
        if groups.len() <= index {
            groups.resize_with(index + 1, || None);
        }
        if groups[index].is_some() {
            continue;
        }
        #[cfg(test)]
        tests::GROUPS_PARSED.with(|parsed| parsed.set(parsed.get() + 1));
        let attrs = module.fn_attrs(id);
        let refused = attrs.string(FnStr::TargetFeatures).and_then(|string| {
            let string = String::from_utf8_lossy(string);
            check_function_features(arch, &string)
                .err()
                .map(|(reason, feature)| (reason, feature.to_owned()))
        });
        let x64 = match arch {
            Arch::X86_64 => hod_x64::X64::of_attrs(attrs),
            Arch::Aarch64 => hod_x64::X64,
        };
        groups[index] = Some(Group { refused, x64 });
    }
    groups
}

/// One function: its target features, then its code.
fn compile_one(
    module: &Module,
    groups: &[Option<Group>],
    body: &Body,
    options: hod_cg::Options,
    scratch: &mut Scratch,
    chunk: &mut Chunk,
) -> Result<(), Failure> {
    let group = match module.symbol(body.symbol()).kind() {
        SymbolKind::Function { attrs, .. } => groups.get(attrs.index()).and_then(Option::as_ref),
        _ => None,
    };
    if let Some((reason, feature)) = group.and_then(|group| group.refused.as_ref()) {
        return Err(Failure::Feature { reason, feature: feature.clone() });
    }
    match module.target().arch() {
        Arch::Aarch64 => compile_function(&hod_a64::A64, module, body, &options, scratch, chunk),
        Arch::X86_64 => {
            let target = group.map_or(&hod_x64::X64, |group| &group.x64);
            compile_function(target, module, body, &options, scratch, chunk)
        }
    }
    .map_err(Failure::Cg)
}

/// The target's judgement of a feature string (`spec/codegen.md` §5.5,
/// §5.6.2): the first refused item, with its reason.
fn check_features(arch: Arch, string: &str) -> Result<(), (&'static str, &str)> {
    match arch {
        Arch::Aarch64 => hod_a64::features::check(string).map_err(|r| (r.reason, r.feature)),
        Arch::X86_64 => hod_x64::features::check(string).map_err(|r| (r.reason, r.feature)),
    }
}

/// [`check_features`] for a function's own `target-features`: on x86-64
/// `-x87` is left to code generation, which refuses it only for a
/// function with an `x86_fp80` value (`spec/codegen.md` §5.6.9).
fn check_function_features(arch: Arch, string: &str) -> Result<(), (&'static str, &str)> {
    match arch {
        Arch::Aarch64 => check_features(arch, string),
        Arch::X86_64 => {
            hod_x64::features::check_function(string).map_err(|r| (r.reason, r.feature))
        }
    }
}

/// What one worker made: chunk index, then the chunk or the first
/// function of it that failed.
type Made = Vec<(usize, Result<Chunk, (usize, Failure)>)>;

/// A worker: takes the next chunk until none is left or a function before
/// it failed.
fn work(
    module: &Module,
    groups: &[Option<Group>],
    bodies: &[Body],
    options: hod_cg::Options,
    chunks: &[Range<usize>],
    next: &AtomicUsize,
    first_failure: &AtomicUsize,
) -> Made {
    // The forced panics of the containment tests: on a worker, or on the
    // calling thread once the workers are started (both before any chunk
    // is taken, so that the site is met whatever the threads' timing).
    #[cfg(test)]
    crate::contain::fault::hit(crate::contain::fault::Site::Worker);
    #[cfg(test)]
    crate::contain::fault::hit(crate::contain::fault::Site::Codegen);
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
                compile_one(module, groups, &bodies[at], options, &mut scratch, &mut chunk)
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

/// The work, in IR instructions, of each chunk the calling thread compiles
/// alone: each is handed to the module emitter as soon as it is made, so
/// that the module's code is not held twice (once in its chunk, once in
/// the object) whatever its size (WP-166).
const SERIAL_CHUNK_INSTS: usize = PARALLEL_MIN_INSTS;

/// The functions of `bodies` compiled one chunk after another on the
/// calling thread, each chunk added to `emitter` once made, its buffers
/// then reused for the next.
///
/// # Errors
/// The first function that failed, with its index. The emitter's own
/// error comes after: once the emitter has refused, the rest of the
/// functions are still compiled (not added), so that a function's
/// refusal is the answer as when every chunk was compiled first.
fn compile_serial(
    module: &Module,
    groups: &[Option<Group>],
    bodies: &[Body],
    options: hod_cg::Options,
    emitter: &mut ModuleEmitter<'_>,
) -> Result<Result<(), CgError>, (usize, Failure)> {
    #[cfg(test)]
    crate::contain::fault::hit(crate::contain::fault::Site::Worker);
    #[cfg(test)]
    crate::contain::fault::hit(crate::contain::fault::Site::Codegen);
    let mut scratch = Scratch::new();
    let mut chunk = Chunk::new();
    let mut laid_out = Ok(());
    for range in plan(bodies, SERIAL_CHUNK_INSTS) {
        for at in range {
            compile_one(module, groups, &bodies[at], options, &mut scratch, &mut chunk)
                .map_err(|failure| (at, failure))?;
        }
        if laid_out.is_ok() {
            laid_out = emitter.add_chunk(&mut chunk);
        } else {
            chunk.clear();
        }
    }
    Ok(laid_out)
}

/// Compiles every body into chunks in module order, on `workers` threads,
/// and adds them to `emitter` in that order.
///
/// # Errors
/// The first function in module order that failed, with its index; then
/// the emitter's error.
fn compile_bodies(
    module: &Module,
    groups: &[Option<Group>],
    bodies: &[Body],
    options: hod_cg::Options,
    workers: usize,
    work_size: usize,
    emitter: &mut ModuleEmitter<'_>,
) -> Result<Result<(), CgError>, (usize, Failure)> {
    if workers <= 1 {
        return compile_serial(module, groups, bodies, options, emitter);
    }
    let chunks = plan(bodies, (work_size / (workers * CHUNKS_PER_WORKER)).max(1));
    let next = AtomicUsize::new(0);
    let first_failure = AtomicUsize::new(usize::MAX);
    let run = || work(module, groups, bodies, options, &chunks, &next, &first_failure);
    // A panic on any of these threads reaches the caller of
    // `generate_with` only after `scope` has joined every one of them
    // (`spec/cli.md` "Embedding").
    #[cfg(test)]
    let plan = crate::contain::fault::current();
    let mut made = std::thread::scope(|scope| {
        let helpers: Vec<_> = (1..workers)
            .map(|_| {
                #[cfg(test)]
                let plan = plan.clone();
                scope.spawn(move || {
                    #[cfg(test)]
                    let _worker = crate::contain::fault::enter_worker(plan);
                    run()
                })
            })
            .collect();
        let mut made = run();
        for helper in helpers {
            match helper.join() {
                Ok(more) => made.extend(more),
                Err(panic) => std::panic::resume_unwind(panic),
            }
        }
        made
    });
    made.sort_unstable_by_key(|&(index, _)| index);
    let mut chunks = Vec::with_capacity(made.len());
    let mut failed: Option<(usize, Failure)> = None;
    for (_, result) in made {
        match result {
            Ok(chunk) => chunks.push(chunk),
            Err((at, failure)) => {
                if failed.as_ref().is_none_or(|&(first, _)| at < first) {
                    failed = Some((at, failure));
                }
            }
        }
    }
    if let Some(failure) = failed {
        return Err(failure);
    }
    // Each chunk's tables are freed once it is added, as the object grows.
    let mut laid_out = Ok(());
    for mut chunk in chunks {
        if laid_out.is_ok() {
            laid_out = emitter.add_chunk(&mut chunk);
        }
    }
    Ok(laid_out)
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
/// `-split-dwarf-output` without `-split-dwarf-file`: `llc` writes an
/// ordinary object and an empty `.dwo` file; hod does not.
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
    // WP-062 (`spec/dwarf.md` §6.15): `-split-dwarf-file` splits, and
    // `-split-dwarf-output` puts the `.dwo` sections in a file.
    let split_dwarf = match (&options.split_dwarf_file, &options.split_dwarf_output) {
        (None, None) => hod_dwarf::SplitDwarf::Off,
        (None, Some(_)) => return option(OPTION_SPLIT_DWARF),
        (Some(_), None) => hod_dwarf::SplitDwarf::Single,
        (Some(_), Some(_)) => hod_dwarf::SplitDwarf::Separate,
    };
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
        debug_compression: options.debug_compression,
        split_dwarf,
    })
}

/// What split DWARF refuses of the module before any code is generated
/// (`spec/dwarf.md` §6.15): a `Dwarf Version` below 5, and a unit whose
/// `.dwo` name is not `-split-dwarf-file`'s (hod writes the unit's).
fn check_split_dwarf(module: &Module, options: &Options) -> Result<(), CompileError> {
    let Some(file) = &options.split_dwarf_file else { return Ok(()) };
    let refuse =
        |name: &'static str| Err(Unsupported::module(Construct::Named(name.into())).into());
    let units = module.debug.compile_units();
    if units.is_empty() {
        return Ok(());
    }
    // No flag is version 4 (WP-150, `spec/dwarf.md` §6.20).
    if module.props.dwarf_version.is_none_or(|v| v < 5) {
        return refuse(hod_dwarf::SPLIT_DWARF_4);
    }
    let file = file.as_os_str().as_encoded_bytes();
    for &unit in units {
        if let hod_ir::DiNode::CompileUnit(cu) = module.debug.node(unit)
            && !cu.split_debug_filename.is_empty()
            && cu.split_debug_filename != file
        {
            return refuse(hod_dwarf::SPLIT_NAME);
        }
    }
    Ok(())
}

/// Writes the `.dwo` file of [`hod_dwarf::SplitDwarf::Separate`]: the
/// companion the DIE writer attached, or an empty object as `llc` writes
/// for a module without split unit, compressed as the object's debug
/// sections are.
fn write_dwo(
    object: &mut hod_obj::Object,
    path: &std::path::Path,
    compression: hod_obj::Compression,
) -> Result<(), CompileError> {
    let mut dwo = object.take_dwo().unwrap_or_else(|| hod_obj::Object::new(object.arch()));
    dwo.compress_debug_sections(compression).map_err(from_obj)?;
    let bytes = hod_obj::elf::to_vec(&dwo).map_err(from_obj)?;
    std::fs::write(path, bytes).map_err(|e| {
        CompileError::InvalidOptions(format!("cannot write '{}': {e}", path.display()))
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
    let cg = cg_options(options)?;
    check_split_dwarf(module, options)?;
    // `-mattr` applies to every function.
    for item in &options.features {
        if let Err((reason, _)) = check_features(module.target().arch(), item) {
            return Err(Unsupported {
                construct: Construct::Named(reason.into()),
                function: None,
                detail: Some(item.clone()),
            }
            .into());
        }
    }

    // The emitter first: what it refuses of the whole module, the debug
    // information's plan above all (`spec/dwarf.md` §6.12), is refused
    // before any code is generated.
    let mut module_emitter =
        ModuleEmitter::new(module, &cg).map_err(|error| from_cg(error, None))?;
    // The functions' DIE refusals that need no code (`spec/dwarf.md` §6.13).
    hod_dwarf::check_functions(module, bodies).map_err(|(at, name)| Unsupported {
        construct: Construct::Named(name.into()),
        function: symbol_name(module, bodies[at].symbol()),
        detail: None,
    })?;

    let work_size: usize = bodies.iter().map(Body::inst_count).sum();
    let wanted = wanted_threads(threads, work_size, bodies.len());
    let lease = match (&threads.tokens, wanted) {
        (Some(tokens), 2..) => Some(Lease { tokens: &**tokens, count: tokens.take(wanted - 1) }),
        _ => None,
    };
    let workers = lease.as_ref().map_or(wanted, |lease| 1 + lease.count);
    let groups = groups(module, bodies);
    let compiled =
        compile_bodies(module, &groups, bodies, cg, workers, work_size, &mut module_emitter);
    drop(lease);
    let emitted = compiled.map_err(|(at, failure)| {
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
    #[cfg(test)]
    crate::contain::fault::hit(crate::contain::fault::Site::Finish);
    let mut emitted =
        emitted.and_then(|()| module_emitter.finish()).map_err(|error| from_cg(error, None))?;
    let object = hod_obj::elf::to_vec(&emitted.object).map_err(from_obj)?;
    if let Some(path) = &options.split_dwarf_output
        && cg.split_dwarf == hod_dwarf::SplitDwarf::Separate
    {
        write_dwo(&mut emitted.object, path, cg.debug_compression)?;
    }
    report.phase("object");
    Ok(object)
}

#[cfg(test)]
mod tests {
    use std::fmt::Write as _;

    use super::*;

    thread_local! {
        /// How many attribute sets [`groups`] has parsed on this thread.
        pub(super) static GROUPS_PARSED: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
    }

    // spec/cli.md "The back half", step 3 (WP-166): the `target-features`
    // of an attribute set are parsed once for the module, however many
    // functions have the set; both targets.
    #[test]
    fn features_are_parsed_once_per_attribute_set() {
        for (triple, shared, other) in [
            ("aarch64-unknown-linux-gnu", "+neon,+v8a", "+lse,+neon"),
            ("x86_64-unknown-linux-gnu", "+avx2,+cx8", "+sse4.2"),
        ] {
            let mut text = format!("target triple = \"{triple}\"\n");
            for (index, group) in [0, 0, 0, 1].into_iter().enumerate() {
                writeln!(text, "define i32 @f{index}() #{group} {{\n  ret i32 {index}\n}}")
                    .unwrap();
            }
            write!(
                text,
                "attributes #0 = {{ nounwind \"target-features\"=\"{shared}\" }}\n\
                 attributes #1 = {{ nounwind \"target-features\"=\"{other}\" }}\n"
            )
            .unwrap();
            let before = GROUPS_PARSED.with(std::cell::Cell::get);
            let object = crate::compile(text.as_bytes(), &obj());
            assert!(object.is_ok_and(|o| o.starts_with(b"\x7FELF")), "{triple}");
            assert_eq!(GROUPS_PARSED.with(std::cell::Cell::get) - before, 2, "{triple}");
        }
        // The functions of a set are compiled with its target, made once:
        // `for_function` keeps it.
        let text = "target triple = \"x86_64-unknown-linux-gnu\"\n\
                    define void @f() #0 {\n  ret void\n}\n\
                    attributes #0 = { \"target-features\"=\"+avx2\" }\n";
        let module =
            hod_ll::parse_module(text.as_bytes(), &hod_ll::ParseOptions::default()).unwrap().module;
        let groups = groups(&module, &[]);
        assert!(groups.is_empty(), "no body, no set parsed");
        let SymbolKind::Function { attrs, .. } =
            module.symbol(module.find_symbol(b"f").unwrap()).kind()
        else {
            panic!("f is a function")
        };
        let own = hod_x64::X64::of_attrs(module.fn_attrs(attrs));
        assert!(own.own && own.features.abi.avx && !hod_x64::X64.own);
    }

    fn obj() -> Options {
        Options { output_kind: crate::OutputKind::Object, ..Options::default() }
    }

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
        let single = Options { split_dwarf_file: Some("a.dwo".into()), ..Options::default() };
        assert_eq!(cg_options(&single).unwrap().split_dwarf, hod_dwarf::SplitDwarf::Single);
        let separate = Options { split_dwarf_output: Some("o/a.dwo".into()), ..single.clone() };
        assert_eq!(cg_options(&separate).unwrap().split_dwarf, hod_dwarf::SplitDwarf::Separate);
        assert_eq!(
            cg_options(&Options::default()).unwrap().split_dwarf,
            hod_dwarf::SplitDwarf::Off
        );
        // `Dwarf Version` below 5, or none (WP-150).
        let text = "target triple = \"aarch64-unknown-linux-gnu\"\n\
                    !llvm.dbg.cu = !{!0}\n!llvm.module.flags = !{!2}\n\
                    !0 = distinct !DICompileUnit(language: DW_LANG_C11, file: !1, \
                    splitDebugFilename: \"a.dwo\", emissionKind: FullDebug)\n\
                    !1 = !DIFile(filename: \"a.c\", directory: \"/w\")\n\
                    !2 = !{i32 7, !\"Dwarf Version\", i32 4}\n";
        let module =
            hod_ll::parse_module(text.as_bytes(), &hod_ll::ParseOptions::default()).unwrap().module;
        let named = |name: &'static str| {
            Err::<(), _>(CompileError::from(Unsupported::module(Construct::Named(name.into()))))
        };
        assert_eq!(check_split_dwarf(&module, &single), named(hod_dwarf::SPLIT_DWARF_4));
        assert_eq!(check_split_dwarf(&module, &Options::default()), Ok(()));
        // WP-150: no flag is `llc`'s version 4, refused the same way.
        let flagless = text.replace("!{!2}", "!{}");
        let flagless = hod_ll::parse_module(flagless.as_bytes(), &hod_ll::ParseOptions::default())
            .unwrap()
            .module;
        assert_eq!(flagless.props.dwarf_version, None);
        assert_eq!(check_split_dwarf(&flagless, &single), named(hod_dwarf::SPLIT_DWARF_4));
        let v5 = text.replace("i32 4}", "i32 5}");
        let module =
            hod_ll::parse_module(v5.as_bytes(), &hod_ll::ParseOptions::default()).unwrap().module;
        assert_eq!(check_split_dwarf(&module, &single), Ok(()));
        let other = Options { split_dwarf_file: Some("b.dwo".into()), ..Options::default() };
        assert_eq!(check_split_dwarf(&module, &other), named(hod_dwarf::SPLIT_NAME));
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
        assert_eq!(cg.debug_compression, hod_obj::Compression::None);
        let gz = Options { debug_compression: hod_obj::Compression::Zlib, ..Options::default() };
        assert_eq!(cg_options(&gz).unwrap().debug_compression, hod_obj::Compression::Zlib);
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
