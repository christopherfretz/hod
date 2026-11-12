//! `cargo xtask corpus census`: what the modules of the corpus are made
//! of. Contract: `spec/harness.md` "corpus", "census".
//!
//! The census is a scan of the module text, not a parse: a lexer that
//! knows comments, strings and quoted names, statements put together from
//! lines, and per statement kind a reading of just the parts that are
//! counted. What it cannot tell exactly is listed in the report
//! ([`APPROXIMATIONS`]) and what it does not recognise is counted in a
//! table of its own, so that a gap shows instead of hiding.

mod report;
mod scan;

use std::collections::{BTreeMap, HashMap};
use std::path::Path;

pub use report::{render, write_report};
pub use scan::{Tok, lex, scan_module};

/// The tables of the census, in report order.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum T {
    TopLevel,
    Header,
    Opcode,
    OpType,
    Cast,
    Predicate,
    InstrFlag,
    Gep,
    Align,
    Switch,
    CallKind,
    Intrinsic,
    IntrinsicDecl,
    InlineAsm,
    ModuleAsm,
    CallConv,
    Linkage,
    Visibility,
    ThreadLocal,
    Comdat,
    GlobalKind,
    Section,
    Signature,
    FnAttr,
    FnAttrDecl,
    ParamAttr,
    RetAttr,
    CallAttr,
    CallArgAttr,
    Bundle,
    IntWidth,
    TypeMention,
    VectorTy,
    AggByValue,
    ConstExpr,
    ConstKind,
    Eh,
    ModuleFlag,
    NamedMetadata,
    MdAttachInstr,
    MdAttachGlobal,
    DebugRecord,
    DiNode,
    Dwarf,
    Unrecognised,
}

impl T {
    pub const ALL: [T; 45] = [
        T::TopLevel,
        T::Header,
        T::Opcode,
        T::OpType,
        T::Cast,
        T::Predicate,
        T::InstrFlag,
        T::Gep,
        T::Align,
        T::Switch,
        T::CallKind,
        T::Intrinsic,
        T::IntrinsicDecl,
        T::InlineAsm,
        T::ModuleAsm,
        T::CallConv,
        T::Linkage,
        T::Visibility,
        T::ThreadLocal,
        T::Comdat,
        T::GlobalKind,
        T::Section,
        T::Signature,
        T::FnAttr,
        T::FnAttrDecl,
        T::ParamAttr,
        T::RetAttr,
        T::CallAttr,
        T::CallArgAttr,
        T::Bundle,
        T::IntWidth,
        T::TypeMention,
        T::VectorTy,
        T::AggByValue,
        T::ConstExpr,
        T::ConstKind,
        T::Eh,
        T::ModuleFlag,
        T::NamedMetadata,
        T::MdAttachInstr,
        T::MdAttachGlobal,
        T::DebugRecord,
        T::DiNode,
        T::Dwarf,
        T::Unrecognised,
    ];

    /// The table's heading and what its rows are.
    #[expect(clippy::too_many_lines, reason = "one line of text per table")]
    pub fn title(self) -> (&'static str, &'static str) {
        match self {
            T::TopLevel => ("Top-level statements", "Module-level statements by kind."),
            T::Header => (
                "Triples, data layouts, producers",
                "`target triple`, `target datalayout`, and the strings of `!llvm.ident`.",
            ),
            T::Opcode => ("Instructions by opcode", "Every instruction of every function body."),
            T::OpType => (
                "Instructions by opcode and operand type",
                "The type the instruction is written with (`add i32`, `load ptr`, `ret void`, \
                 `br i1` for a conditional branch); `struct` and `array` are first-class \
                 aggregates, a named type counts as what it names.",
            ),
            T::Cast => ("Casts", "Cast instructions by source and destination type."),
            T::Predicate => ("Comparison predicates", "`icmp` and `fcmp` by predicate."),
            T::InstrFlag => (
                "Instruction flags and orderings",
                "Flags written after an opcode (`nsw`, `inbounds`, fast-math flags, \
                 `volatile`, `atomic`), atomic orderings and `atomicrmw` operations, and \
                 whether an `alloca` has a count operand.",
            ),
            T::Gep => (
                "getelementptr shapes",
                "The source element type, the number of indices, and whether every index is \
                 a literal.",
            ),
            T::Align => {
                ("Alignments", "`align` of `alloca`, `load`, `store` and global variables.")
            }
            T::Switch => ("switch sizes", "`switch` by condition type and number of cases."),
            T::CallKind => (
                "Calls by kind",
                "`call`, `invoke` and `callbr` by what is called; `tail`, `musttail` and \
                 `notail` markers; calls written with an explicit function type (variadic \
                 callees).",
            ),
            T::Intrinsic => (
                "Intrinsics called",
                "Call sites by intrinsic, the overload suffix folded (`llvm.memcpy.p0.p0.i64` \
                 is `llvm.memcpy`).",
            ),
            T::IntrinsicDecl => {
                ("Intrinsics declared", "`declare` statements of `@llvm.*`, suffix folded.")
            }
            T::InlineAsm => (
                "Inline assembly",
                "`call ... asm` by constraint string and assembly text (cut at 60 characters).",
            ),
            T::ModuleAsm => ("Module-level assembly", "`module asm` lines by their first word."),
            T::CallConv => (
                "Calling conventions",
                "On definitions, declarations and call sites; `ccc (default)` is none written.",
            ),
            T::Linkage => (
                "Linkage",
                "Of function definitions and declarations, variables, aliases and IFUNCs; \
                 `external (default)` is none written.",
            ),
            T::Visibility => (
                "Visibility, preemption, DLL storage, unnamed_addr",
                "The words written on functions and globals.",
            ),
            T::ThreadLocal => ("Thread-local variables", "By TLS model as written."),
            T::Comdat => (
                "COMDATs",
                "`$name = comdat <kind>` statements, and uses on functions and globals.",
            ),
            T::GlobalKind => (
                "Global variables",
                "By kind (`global`, `constant`, declaration, alias, IFUNC) and value type; \
                 the `@llvm.*` variables by name; address spaces.",
            ),
            T::Section => ("Explicit sections", "`section \"...\"` on functions and globals."),
            T::Signature => (
                "Function signatures",
                "Return and parameter types of definitions and declarations, parameter \
                 counts, variadic functions, `gc`, `prefix`, `prologue`.",
            ),
            T::FnAttr => (
                "Function attributes on definitions",
                "Written on the `define` or through its `#N` attribute groups; string \
                 attributes with their value (cut at 48 characters).",
            ),
            T::FnAttrDecl => ("Function attributes on declarations", "As above, for `declare`."),
            T::ParamAttr => {
                ("Parameter attributes", "On parameters of definitions and declarations.")
            }
            T::RetAttr => ("Return attributes", "On definitions, declarations and call sites."),
            T::CallAttr => (
                "Call-site function attributes",
                "After the arguments of a call, directly or through `#N` groups.",
            ),
            T::CallArgAttr => ("Call-site argument attributes", "On the arguments of calls."),
            T::Bundle => ("Operand bundles", "By tag."),
            T::IntWidth => (
                "Integer types",
                "Every mention of an `iN` type in definitions, declarations, globals and \
                 function bodies (constants' types included).",
            ),
            T::TypeMention => (
                "Other type mentions",
                "Floating-point kinds, `ptr` (with address space), array types, `token`, \
                 target extension types; counted like the integer types.",
            ),
            T::VectorTy => ("Vector types", "Every mention of a vector type, by its exact shape."),
            T::AggByValue => (
                "Aggregates passed and returned by value",
                "Parameters, results and call arguments of struct or array type (not behind \
                 a pointer), by flattened shape.",
            ),
            T::ConstExpr => (
                "Constant expressions",
                "By kind, in global initializers and as instruction operands.",
            ),
            T::ConstKind => (
                "Constant kinds",
                "`zeroinitializer`, `undef`, `poison`, `null`, strings, `blockaddress`, \
                 floating-point literal forms, references to globals in initializers.",
            ),
            T::Eh => (
                "Exception handling",
                "`landingpad` clauses, personality functions, and the instructions `invoke`, \
                 `resume`, `landingpad` again for convenience.",
            ),
            T::ModuleFlag => ("Module flags", "`!llvm.module.flags` entries: name and value."),
            T::NamedMetadata => ("Named metadata", "`!name = !{...}` statements by name."),
            T::MdAttachInstr => {
                ("Metadata attachments on instructions", "`!kind` after an instruction.")
            }
            T::MdAttachGlobal => (
                "Metadata attachments on functions and globals",
                "`!kind` on `define`, `declare` and global variables.",
            ),
            T::DebugRecord => (
                "Debug records and debug intrinsics",
                "`#dbg_*` records (not instructions) against calls of `llvm.dbg.*`.",
            ),
            T::DiNode => (
                "Metadata nodes",
                "Numbered nodes by kind: `!DI*` specialized nodes, and plain tuples.",
            ),
            T::Dwarf => (
                "DWARF constants in metadata",
                "`DW_*` words in numbered nodes, and `emissionKind`.",
            ),
            T::Unrecognised => (
                "Not recognised by the scanner",
                "Statements whose first word the scanner does not know. Anything here is a \
                 gap in the census, not necessarily in the IR.",
            ),
        }
    }

    /// The most rows of the table the report shows.
    pub fn cap(self) -> usize {
        match self {
            T::Opcode | T::Intrinsic | T::Unrecognised | T::CallConv | T::Linkage => 400,
            T::OpType | T::AggByValue | T::Gep | T::Section | T::Align => 60,
            _ => 120,
        }
    }
}

/// What the scanner approximates; printed in the report.
pub const APPROXIMATIONS: &str = "\
The census is lexical. The scanner splits each module into tokens (comments, \
strings, `c\"...\"` constants and quoted names are single tokens and are never \
counted as code), joins lines into statements (a line continues the statement \
before it while a bracket is open, and when it starts with `to`, `catch`, \
`cleanup` or `filter` inside a function body), and reads only the parts of a \
statement that a table needs. Known approximations:

- A type is recognised where the grammar puts one (after an opcode, in a \
  parameter list); type *mentions* (integer widths, floating-point kinds, \
  vectors) are counted per token, so a constant's type counts as a mention.
- Named struct types are resolved to their shape for the by-value tables; \
  struct literals and named types are not counted as type mentions.
- Constant expressions are recognised by their keyword followed by `(` at a \
  position that is not the instruction's own opcode.
- `prefix` and `prologue` data on functions are counted but their constants \
  are not read.
- Instruction counts exclude `#dbg_*` records. A basic block is a label line, \
  plus the unlabelled entry block of a function.
- Module counts say in how many modules a row occurs at least once.
";

/// One cell of a table: occurrences, and the modules they are in.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Cell {
    pub count: u64,
    pub modules: u32,
    /// The last module that counted here (a serial of the scanning thread).
    last: u32,
}

/// The distributions kept per group.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum D {
    ModuleBytes,
    ModuleLines,
    Definitions,
    Declarations,
    GlobalVariables,
    BlocksPerModule,
    InstructionsPerModule,
    MetadataNodes,
    BlocksPerFunction,
    InstructionsPerFunction,
}

impl D {
    pub const ALL: [D; 10] = [
        D::ModuleBytes,
        D::ModuleLines,
        D::Definitions,
        D::Declarations,
        D::GlobalVariables,
        D::BlocksPerModule,
        D::InstructionsPerModule,
        D::MetadataNodes,
        D::BlocksPerFunction,
        D::InstructionsPerFunction,
    ];

    pub fn title(self) -> &'static str {
        match self {
            D::ModuleBytes => "module text, bytes",
            D::ModuleLines => "module text, lines",
            D::Definitions => "function definitions per module",
            D::Declarations => "function declarations per module",
            D::GlobalVariables => "global variables per module",
            D::BlocksPerModule => "basic blocks per module",
            D::InstructionsPerModule => "instructions per module",
            D::MetadataNodes => "numbered metadata nodes per module",
            D::BlocksPerFunction => "basic blocks per function",
            D::InstructionsPerFunction => "instructions per function",
        }
    }
}

/// The count of one group (a source with an LLVM major).
#[derive(Clone, Debug, Default)]
pub struct GroupCensus {
    pub modules: u32,
    tables: BTreeMap<T, HashMap<String, Cell>>,
    samples: BTreeMap<D, Vec<u64>>,
}

/// `n`, sum, median, 90th percentile (nearest rank) and maximum.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Summary {
    pub n: usize,
    pub sum: u64,
    pub median: u64,
    pub p90: u64,
    pub max: u64,
}

impl GroupCensus {
    /// Adds `by` to the row `key` of `table`.
    pub fn add(&mut self, table: T, key: &str, by: u64) {
        let serial = self.modules;
        let rows = self.tables.entry(table).or_default();
        let cell = match rows.get_mut(key) {
            Some(cell) => cell,
            None => rows.entry(key.to_owned()).or_default(),
        };
        cell.count += by;
        if cell.last != serial {
            cell.last = serial;
            cell.modules += 1;
        }
    }

    pub fn bump(&mut self, table: T, key: &str) {
        self.add(table, key, 1);
    }

    pub fn sample(&mut self, of: D, value: u64) {
        self.samples.entry(of).or_default().push(value);
    }

    /// The count of a row; zero when there is none.
    pub fn count(&self, table: T, key: &str) -> u64 {
        self.cell(table, key).count
    }

    pub fn cell(&self, table: T, key: &str) -> Cell {
        self.tables.get(&table).and_then(|rows| rows.get(key)).copied().unwrap_or_default()
    }

    /// The keys of a table, sorted.
    pub fn keys(&self, table: T) -> Vec<&str> {
        let mut keys: Vec<&str> =
            self.tables.get(&table).into_iter().flatten().map(|(key, _)| key.as_str()).collect();
        keys.sort_unstable();
        keys
    }

    pub fn summary(&self, of: D) -> Summary {
        let mut values = self.samples.get(&of).cloned().unwrap_or_default();
        values.sort_unstable();
        let n = values.len();
        if n == 0 {
            return Summary::default();
        }
        Summary {
            n,
            sum: values.iter().sum(),
            median: values[(n - 1) / 2],
            p90: values[(n * 9).div_ceil(10).max(1) - 1],
            max: values[n - 1],
        }
    }

    /// Adds another count of the same group (made by another thread).
    pub fn merge(&mut self, other: GroupCensus) {
        self.modules += other.modules;
        for (table, rows) in other.tables {
            let mine = self.tables.entry(table).or_default();
            for (key, cell) in rows {
                let into = mine.entry(key).or_default();
                into.count += cell.count;
                into.modules += cell.modules;
            }
        }
        for (of, mut values) in other.samples {
            self.samples.entry(of).or_default().append(&mut values);
        }
    }
}

/// Counts every entry of the corpus in `dir`, by group (`<source>/llvm<N>`,
/// in name order), `jobs` files at a time.
pub fn count_corpus(
    dir: &Path,
    entries: &[(String, String)],
    jobs: usize,
) -> Result<BTreeMap<String, GroupCensus>, String> {
    let next = std::sync::atomic::AtomicUsize::new(0);
    let results: Vec<Result<BTreeMap<String, GroupCensus>, String>> = std::thread::scope(|scope| {
        let workers: Vec<_> = (0..jobs.clamp(1, entries.len().max(1)))
            .map(|_| {
                scope.spawn(|| {
                    let mut mine: BTreeMap<String, GroupCensus> = BTreeMap::new();
                    loop {
                        let index = next.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                        let Some((group, entry)) = entries.get(index) else { break };
                        let path = dir.join(entry);
                        let text = std::fs::read_to_string(&path)
                            .map_err(|e| format!("cannot read '{}': {e}", path.display()))?;
                        let census = mine.entry(group.clone()).or_default();
                        scan_module(census, super::entry::module_of(&text));
                    }
                    Ok(mine)
                })
            })
            .collect();
        workers
            .into_iter()
            .map(|worker| {
                worker.join().unwrap_or_else(|_| Err("a census thread panicked".to_owned()))
            })
            .collect()
    });
    let mut all: BTreeMap<String, GroupCensus> = BTreeMap::new();
    for result in results {
        for (group, census) in result? {
            all.entry(group).or_default().merge(census);
        }
    }
    Ok(all)
}
