//! What the differential harness (`tests/diff.rs`) generates: one line
//! per instruction group that has landed.
//!
//! A group package that lands adds its line to [`GROUPS`], switching on
//! the `hod_interp::generate::Options` its constructs need (and, for
//! WP-035, `generator`: `hod_interp`'s generator always emits integer
//! arithmetic, so no program of it compiles before the integer group).
//! Until then the harness runs the skeleton's own programs
//! (`common::skeleton`). [`missing`] names what is still off; the harness
//! prints it.

use hod_interp::generate::Options as GenOptions;

/// What the harness may generate.
#[derive(Clone, Debug)]
pub struct Enabled {
    /// Programs of `hod_interp::generate` with [`Enabled::options`].
    pub generator: bool,
    /// The generator's switches.
    pub options: GenOptions,
}

/// What a group switches on.
pub type Enable = fn(&mut Enabled);

/// The groups that have landed, in landing order, each with what it
/// switches on.
pub const GROUPS: &[(&str, Enable)] = &[
    // The skeleton: `br`, `ret`, `unreachable`, constants, arguments,
    // blocks without predecessors (through `common::skeleton`).
    ("WP-033", |e| e.options.unreachable_blocks = true),
    // The integer group: the generator itself, integers of every width, compares, selects, casts.
    ("WP-035", |e| {
        e.generator = true;
        e.options.narrow_ints = true;
        e.options.odd_ints = true;
        e.options.casts = true;
        e.options.compares = true;
        e.options.selects = true;
    }),
    // The memory group: loads, stores, `ptradd`, slots, globals.
    ("WP-036", |e| (e.options.memory, e.options.globals) = (true, true)),
    // Control flow: `condbr`, `switch`, loops with `phi`s, `indirectbr`.
    ("WP-037", |e| {
        let o = &mut e.options;
        (o.branches, o.loops, o.switches, o.block_addresses) = (true, true, true, true);
    }),
    // Calls: direct and indirect, many arguments.
    ("WP-038", |e| {
        e.options.calls = true;
        e.options.indirect_calls = true;
        e.options.many_args = true;
    }),
    // The floating-point group: `float`, `double`, `fcmp`, conversions.
    ("WP-039", |e| e.options.floats = true),
];

/// What every landed group enables together.
pub fn enabled() -> Enabled {
    let mut e = Enabled { generator: false, options: GenOptions::minimal() };
    for (_, enable) in GROUPS {
        enable(&mut e);
    }
    e
}

/// The generator's switches that are still off, and `generator` itself
/// while it is: the missing mask.
pub fn missing(e: &Enabled) -> Vec<&'static str> {
    let o = &e.options.effective();
    let mut out = Vec::new();
    if !e.generator {
        out.push("generator");
    }
    macro_rules! off {
        ($($field:ident),+) => { $(if !o.$field { out.push(stringify!($field)); })+ };
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
    out
}
