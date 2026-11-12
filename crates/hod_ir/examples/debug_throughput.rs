//! A throughput sanity check for the debug tables, not a gate
//! (`spec/ir.md` §6.10).
//!
//! ```text
//! cargo run --release -p hod_ir --example debug_throughput [functions]
//! ```
//!
//! Builds the debug information of a module of small C++ functions, in
//! the mix a standard-library translation unit has at `-g -O0` (for each
//! function a distinct subprogram with its declaration, a subroutine
//! type, parameter and local variables, a lexical block, pointer and
//! member types, and every fourth function a class), and the locations
//! of their instructions, a third of them inlined. Reports nodes and
//! locations built per second, the time to read and to verify a node,
//! and the bytes a node and a location take.

use std::time::Instant;

use hod_ir::{
    DW_TAG_MEMBER, DW_TAG_POINTER_TYPE, DW_TAG_STRUCTURE_TYPE, DebugInfo, DiBasicType,
    DiCompileUnit, DiCompositeType, DiDerivedType, DiFile, DiFlags, DiId, DiLexicalBlock,
    DiLocalVariable, DiLocation, DiNode, DiSubprogram, DiSubroutineType, EmissionKind, IrResult,
    LocId, Module, SpFlags, TargetSpec,
};

/// Instruction locations for each function.
const LOCS: u32 = 24;

/// The names of one function's nodes, made before the clock starts.
struct Names {
    function: String,
    linkage: String,
    class: String,
}

fn names(functions: usize) -> Vec<Names> {
    (0..functions)
        .map(|index| Names {
            function: format!("function_{index}"),
            linkage: format!("_ZN5outer5inner11function_{index}EPKcRSt6vectorIiSaIiEE"),
            class: format!("_ZTSN5outer7Class{index}E"),
        })
        .collect()
}

/// The debug information of `names.len()` functions. Returns the tables
/// and the number of locations asked for.
#[allow(clippy::too_many_lines)]
fn build(names: &[Names]) -> IrResult<(DebugInfo, usize)> {
    let mut d = DebugInfo::default();
    let file =
        d.add(&DiFile { filename: b"unit.cpp", directory: b"/src", ..Default::default() }.into())?;
    let unit = DiCompileUnit {
        language: 0x21,
        file: Some(file),
        producer: b"a compiler",
        emission_kind: EmissionKind::FullDebug,
        ..Default::default()
    };
    let unit = d.add_distinct(&unit.into())?;
    d.add_compile_unit(unit)?;
    let int =
        d.add(&DiBasicType { name: b"int", size: 32, encoding: 5, ..Default::default() }.into())?;
    let character = DiBasicType { name: b"char", size: 8, encoding: 8, ..Default::default() };
    let character = d.add(&character.into())?;
    let mut asked = 0;
    let mut class = int;
    for (index, name) in names.iter().enumerate() {
        let line = index as u32 * 7 + 1;
        // Every fourth function is a member of a new class.
        if index % 4 == 0 {
            class = d.reserve()?;
            let member = |name, offset| DiDerivedType {
                tag: DW_TAG_MEMBER,
                name,
                scope: Some(class),
                file: Some(file),
                line,
                base_type: Some(int),
                size: 32,
                offset,
                ..Default::default()
            };
            let members = [
                d.add(&member(b"first", 0).into())?,
                d.add(&member(b"second", 32).into())?,
                d.add(&member(b"third", 64).into())?,
            ];
            let node = DiCompositeType {
                tag: DW_TAG_STRUCTURE_TYPE,
                name: &name.function.as_bytes()[..8],
                file: Some(file),
                line,
                size: 96,
                flags: DiFlags::TYPE_PASS_BY_VALUE,
                elements: &members,
                identifier: name.class.as_bytes(),
                ..Default::default()
            };
            d.define_distinct(class, &node.into())?;
        }
        // The types: a pointer to the class, uniqued, and a signature
        // that every fourth function shares.
        let pointer = DiDerivedType {
            tag: DW_TAG_POINTER_TYPE,
            base_type: Some(if index % 2 == 0 { class } else { character }),
            size: 64,
            ..Default::default()
        };
        let pointer = d.add(&pointer.into())?;
        let signature =
            d.add(&DiSubroutineType { types: &[int, pointer, int], ..Default::default() }.into())?;
        // The member declaration and the definition, which names its
        // variables and is named by them.
        let declaration = DiSubprogram {
            name: name.function.as_bytes(),
            linkage_name: name.linkage.as_bytes(),
            scope: Some(class),
            file: Some(file),
            line,
            ty: Some(signature),
            scope_line: line,
            flags: DiFlags::PROTOTYPED,
            ..Default::default()
        };
        let declaration = d.add(&declaration.into())?;
        let function = d.reserve()?;
        let variable = |name, arg, scope, ty| DiLocalVariable {
            name,
            arg,
            scope: Some(scope),
            file: Some(file),
            line: line + u32::from(arg),
            ty: Some(ty),
            ..Default::default()
        };
        let block =
            DiLexicalBlock { scope: Some(function), file: Some(file), line: line + 3, column: 5 };
        let block = d.add_distinct(&block.into())?;
        let variables = [
            d.add(&variable(b"this", 1, function, pointer).into())?,
            d.add(&variable(b"count", 2, function, int).into())?,
            d.add(&variable(b"total", 0, block, int).into())?,
        ];
        let node = DiSubprogram {
            name: name.function.as_bytes(),
            linkage_name: name.linkage.as_bytes(),
            scope: Some(class),
            file: Some(file),
            line,
            ty: Some(signature),
            scope_line: line,
            flags: DiFlags::PROTOTYPED,
            sp_flags: SpFlags::DEFINITION,
            unit: Some(unit),
            declaration: Some(declaration),
            retained_nodes: &variables,
            ..Default::default()
        };
        d.define_distinct(function, &node.into())?;
        // The locations: a new line every other instruction, and every
        // third function inlined at a call site of its own.
        let inlined_at = if index % 3 == 2 {
            d.distinct_loc(DiLocation { line, column: 9, scope: block, inlined_at: LocId::NONE })?
        } else {
            LocId::NONE
        };
        for step in 0..LOCS {
            let scope = if step < LOCS / 2 { function } else { block };
            let column = 3 + (step % 4) as u16 * 6;
            d.loc(DiLocation { line: line + step / 2, column, scope, inlined_at })?;
        }
        asked += LOCS as usize + usize::from(index % 3 == 2);
    }
    Ok((d, asked))
}

/// Reads every node and location, as DWARF emission does.
fn read(d: &DebugInfo) -> u64 {
    let mut sum = 0;
    for index in 0..d.node_count() as u32 {
        sum += match d.node(DiId::new(index)) {
            DiNode::Subprogram(node) => u64::from(node.line) + node.retained_nodes.len() as u64,
            DiNode::LocalVariable(node) => u64::from(node.arg) + node.name.len() as u64,
            DiNode::DerivedType(node) => node.size + node.offset,
            DiNode::CompositeType(node) => node.size + node.elements.len() as u64,
            DiNode::SubroutineType(node) => node.types.len() as u64,
            other => other.kind() as u64,
        };
    }
    for index in 0..d.loc_count() as u32 {
        sum += d.location(LocId::new(index)).map_or(0, |loc| u64::from(loc.line));
    }
    sum
}

#[allow(clippy::cast_precision_loss, reason = "figures printed with one decimal")]
fn ratio(a: u128, b: usize) -> f64 {
    a as f64 / b as f64
}

/// The fastest of five runs of `run`, in nanoseconds, and its last result.
fn fastest<T>(mut run: impl FnMut() -> IrResult<T>) -> IrResult<(u128, T)> {
    let mut best: Option<(u128, T)> = None;
    for _ in 0..5 {
        let start = Instant::now();
        let result = run()?;
        let nanos = start.elapsed().as_nanos();
        if best.as_ref().is_none_or(|&(least, _)| nanos < least) {
            best = Some((nanos, result));
        }
    }
    Ok(best.expect("five runs"))
}

fn main() -> IrResult<()> {
    let functions: usize = match std::env::args().nth(1) {
        Some(text) => text.parse().expect("the number of functions"),
        None => 100_000,
    };
    let names = names(functions);
    let (building, (debug, asked)) = fastest(|| build(&names))?;
    let stats = debug.stats();
    // The locations alone: new ones, the same ones again, and ones that
    // are not interned.
    let scoped = || {
        let mut d = DebugInfo::default();
        let file = d.add(&DiFile { filename: b"unit.cpp", ..Default::default() }.into())?;
        let node = DiSubprogram { name: b"f", file: Some(file), ..Default::default() };
        let scope = d.add(&node.into())?;
        Ok((d, scope))
    };
    let at = |index: u32, scope| DiLocation {
        line: index / 2,
        column: (index % 4) as u16,
        scope,
        inlined_at: LocId::NONE,
    };
    let (locating, (mut table, scope)) = fastest(|| {
        let (mut d, scope) = scoped()?;
        for index in 0..asked as u32 {
            d.loc(at(index, scope))?;
        }
        Ok((d, scope))
    })?;
    let (repeating, ()) = fastest(|| {
        for index in 0..asked as u32 {
            table.loc(at(index, scope))?;
        }
        Ok(())
    })?;
    assert_eq!(table.loc_count(), asked, "a location seen before is not a new one");
    let (pushing, ()) = fastest(|| {
        let (mut d, scope) = scoped()?;
        for index in 0..asked as u32 {
            d.distinct_loc(at(index, scope))?;
        }
        Ok(())
    })?;
    let (reading, sum) = fastest(|| Ok(read(&debug)))?;
    let mut m = Module::new(TargetSpec::aarch64_linux_gnu());
    m.debug = debug;
    let (verifying, ()) = fastest(|| m.verify_debug())?;
    let entries = stats.nodes + asked;
    println!(
        "{functions} functions: {} nodes, {asked} locations asked for, {} kept",
        stats.nodes, stats.locs
    );
    println!("  build, nodes and locations  {:6.1} ns each", ratio(building, entries));
    println!(
        "                              {:6.1} million a second",
        1e3 / ratio(building, entries)
    );
    println!("  locations alone, new        {:6.1} ns each", ratio(locating, asked));
    println!("                              {:6.1} million a second", 1e3 / ratio(locating, asked));
    println!("  locations alone, seen       {:6.1} ns each", ratio(repeating, asked));
    println!("  locations alone, distinct   {:6.1} ns each", ratio(pushing, asked));
    println!("  read (sum {sum})   {:6.1} ns each", ratio(reading, stats.nodes + stats.locs));
    println!(
        "  verify                      {:6.1} ns each",
        ratio(verifying, stats.nodes + stats.locs)
    );
    println!(
        "  node bytes                  {:6.1} bytes/node",
        ratio(stats.node_bytes as u128, stats.nodes)
    );
    println!(
        "  string bytes                {:6.1} bytes/node",
        ratio(stats.string_bytes as u128, stats.nodes)
    );
    println!(
        "  location bytes              {:6.1} bytes/location",
        ratio(stats.loc_bytes as u128, stats.locs)
    );
    Ok(())
}
