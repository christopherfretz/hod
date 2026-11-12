//! The bridge from the functions' records to `hod_dwarf` (`spec/dwarf.md`
//! §5.7, §9; `spec/codegen.md` §9.7): the line table of every compile
//! unit, and the units' DIEs with the ranges of their code.
//!
//! [`DebugLines::new`] runs once per module, before any function is
//! placed: one [`UnitPlan`] per compile unit that is written (its file
//! table is the line table's, `spec/dwarf.md` §4.4), and nothing at all
//! for a module without `!llvm.dbg.cu` or whose units are all `NoDebug`.
//! [`DebugLines::add_function`] runs as each function is placed: its
//! transitions become `hod_dwarf` transitions (a `LocId` becomes the file
//! index of its scope's file, its line and its column; an inlined
//! location keeps its own scope's file and line, as `llc` writes it), the
//! policy (`function_rows`) makes the rows, and the rows are encoded at
//! once as the function's own sequence into its unit's [`LineProgram`],
//! relative to the section symbol of its code plus its offset. A
//! sequence's bytes depend only on its rows, and functions are placed in
//! module order, so the program is the same for any division into
//! chunks. [`DebugLines::finish`] writes `.debug_line` (one unit per
//! compile unit, in `!llvm.dbg.cu` order), then the DIEs with
//! `DW_AT_stmt_list` and the code spans of each unit.
//!
//! Nothing is allocated per function or per row: the transitions and the
//! rows are in two buffers reused across functions, and a location's file
//! is found through a cache by scope.

use hod_dwarf::{
    CodeSpan, FunctionInput, FunctionLines, GlobalAddr, LineProgram, LineUnit, LocalPlace,
    LocalRow, Row, ScopeRow, Sequence, SourceLoc, Transition, UnitInput, UnitPlan, function_rows,
    has_dies, write_debug_info, write_debug_line_units,
};
use hod_ir::{DebugInfo, DiId, DiKind, DiNode, EmissionKind, Module, SymId, SymbolKind};
use hod_obj::{Arch, Binding, Object, SectionId, SymbolId};

use super::unsupported;
use crate::error::DEBUG_FRAME_PLACE;
use crate::error::{CgError, CgResult};
use crate::record::{FrameDesc, LineRow, VarPlace, VarRow};

/// "Not looked up yet" in [`DebugLines::scope_file`].
const UNKNOWN: u32 = u32::MAX;
/// "The scope has no file".
const NO_FILE: u32 = u32::MAX - 1;

/// One compile unit that is written.
#[derive(Debug)]
struct Unit<'m> {
    plan: UnitPlan<'m>,
    /// The unit's sequences, one per function, in module order.
    program: LineProgram,
    /// The code of the unit's functions.
    code: Vec<CodeSpan>,
    /// The unit's global-variable expressions, sorted.
    globals: Vec<DiId>,
    /// Whether the unit has DIEs: not a `DebugDirectivesOnly` unit, which
    /// is only a line table (`spec/dwarf.md` §6.10).
    dies: bool,
}

/// The debug information of a module being emitted.
#[derive(Debug)]
pub(super) struct DebugLines<'m> {
    debug: &'m DebugInfo,
    units: Vec<Unit<'m>>,
    /// Per node: the raw id of a scope's file, [`NO_FILE`], or
    /// [`UNKNOWN`].
    scope_file: Vec<u32>,
    /// The last scope looked up and its file index in the last unit.
    last: Option<(u32, DiId, u32)>,
    transitions: Vec<Transition>,
    rows: Vec<Row>,
    scopes: Vec<ScopeRow>,
    locals: Vec<LocalRow>,
}

/// Where a function was placed, and its frame.
#[derive(Clone, Copy, Debug)]
pub(super) struct Placed {
    /// Its section.
    pub(super) section: SectionId,
    /// Its offset there.
    pub(super) offset: u64,
    /// Its code's size.
    pub(super) size: u32,
    /// Its frame.
    pub(super) frame: FrameDesc,
}

fn dwarf_err(error: hod_dwarf::Error) -> CgError {
    match error {
        // `spec/dwarf.md` §6.4: what the function DIEs refuse has a
        // stable name of its own.
        hod_dwarf::Error::Unsupported { what, .. } if what.starts_with("debug:") => {
            CgError::unsupported(what)
        }
        hod_dwarf::Error::Unsupported { .. } => CgError::unsupported(unsupported::DEBUG_DIE),
        // §6.12: what is wrong with the input is a decline, by name; what
        // is left (a line row, a span, a symbol, a global's address the
        // bridge made) is the bridge's bug.
        hod_dwarf::Error::NotACompileUnit { .. }
        | hod_dwarf::Error::BadReference { .. }
        | hod_dwarf::Error::MemberOutsideType { .. }
        | hod_dwarf::Error::TypeCycle { .. }
        | hod_dwarf::Error::MissingSize { .. }
        | hod_dwarf::Error::NulInName => CgError::unsupported(unsupported::DEBUG_MALFORMED),
        hod_dwarf::Error::TooLarge => CgError::unsupported(unsupported::DEBUG_TOO_LARGE),
        error => CgError::internal(format!("debug information: {error}")),
    }
}

impl<'m> DebugLines<'m> {
    /// The plans of `module`'s compile units; `None` when nothing is to
    /// be written (no unit, or only `NoDebug` units), which costs nothing
    /// further.
    ///
    /// # Errors
    /// [`unsupported::DEBUG_DIE`], a `debug:*` name of the plan;
    /// [`unsupported::DEBUG_MALFORMED`] for a unit the tables do not
    /// describe.
    pub(super) fn new(module: &'m Module, arch: Arch) -> CgResult<Option<Box<DebugLines<'m>>>> {
        let debug = &module.debug;
        let mut units = Vec::new();
        for &unit in debug.compile_units() {
            let DiNode::CompileUnit(cu) = debug.node(unit) else {
                return Err(CgError::unsupported(unsupported::DEBUG_MALFORMED));
            };
            if cu.emission_kind == EmissionKind::NoDebug {
                continue;
            }
            // `LineTablesOnly` and `DebugDirectivesOnly` (WP-061,
            // `spec/dwarf.md` §6.10): the plan refuses what they cannot
            // describe; the second gets no DIEs at all.
            let plan = UnitPlan::for_functions(module, unit).map_err(dwarf_err)?;
            let mut globals = cu.globals.to_vec();
            globals.sort_unstable();
            let dies = has_dies(cu.emission_kind);
            units.push(Unit {
                plan,
                program: LineProgram::new(arch),
                code: Vec::new(),
                globals,
                dies,
            });
        }
        if units.is_empty() {
            return Ok(None);
        }
        Ok(Some(Box::new(DebugLines {
            debug,
            units,
            scope_file: vec![UNKNOWN; debug.node_count()],
            last: None,
            transitions: Vec::new(),
            rows: Vec::new(),
            scopes: Vec::new(),
            locals: Vec::new(),
        })))
    }

    /// The index of the written unit of subprogram `sp`, and the
    /// subprogram's file index and scope line there.
    fn unit_of(&mut self, sp: DiId) -> Option<(usize, SourceLoc)> {
        if self.debug.find_kind(sp) != Some(DiKind::Subprogram) {
            return None;
        }
        let DiNode::Subprogram(node) = self.debug.node(sp) else { return None };
        let unit = node.unit?;
        let index = self.units.iter().position(|u| u.plan.unit() == unit)?;
        let file = node.file.and_then(|f| self.units[index].plan.file_index(f)).unwrap_or(0);
        let line = if node.scope_line != 0 { node.scope_line } else { node.line };
        Some((index, SourceLoc { file, line, column: 0 }))
    }

    /// The file index in unit `unit` of the file of local scope `scope`.
    fn file_of(&mut self, unit: usize, scope: DiId) -> u32 {
        if let Some((u, s, file)) = self.last
            && u == unit as u32
            && s == scope
        {
            return file;
        }
        let Some(&cached) = self.scope_file.get(scope.index()) else { return 0 };
        let raw = if cached == UNKNOWN {
            let file = match self.debug.node(scope) {
                DiNode::Subprogram(n) => n.file,
                DiNode::LexicalBlock(n) => n.file,
                DiNode::LexicalBlockFile(n) => n.file,
                _ => None,
            };
            let raw = file.map_or(NO_FILE, DiId::raw);
            self.scope_file[scope.index()] = raw;
            raw
        } else {
            cached
        };
        // A scope without a file is in the unit's primary file, as
        // `llc` numbers it.
        let file = if raw == NO_FILE {
            0
        } else {
            self.units[unit].plan.file_index(DiId::new(raw)).unwrap_or(0)
        };
        self.last = Some((unit as u32, scope, file));
        file
    }

    /// Encodes the sequence of the function `sym` whose transitions are
    /// `lines`, placed as `placed` says, records its code span, and adds
    /// its DIEs (`spec/dwarf.md` §6.4) from `lines` and its variable rows
    /// `vars`. A function without a subprogram of a written unit gets
    /// nothing.
    ///
    /// # Errors
    /// A `debug:*` name for a function whose DIEs are not written
    /// exactly; an internal error for rows the line writer refuses (a
    /// broken record).
    pub(super) fn add_function(
        &mut self,
        module: &Module,
        obj: &mut Object,
        sym: SymId,
        lines: &[LineRow],
        vars: &[VarRow],
        placed: Placed,
    ) -> CgResult<()> {
        let Placed { section, offset, size, frame } = placed;
        let subprogram = match module.symbol(sym).kind() {
            SymbolKind::Function { subprogram, .. } => subprogram,
            _ => None,
        };
        // A function without a subprogram has no line information, even
        // when its instructions have locations: `llc` writes neither rows
        // nor a range for it (§5.7).
        let Some(sp) = subprogram else { return Ok(()) };
        let Some((unit, scope)) = self.unit_of(sp) else {
            return Ok(());
        };
        if size == 0 {
            return Ok(());
        }
        let symbol = obj.section_symbol(section);
        let addend = offset as i64;
        let code = CodeSpan { symbol, addend, size: u64::from(size) };
        self.units[unit].code.push(code);

        // The DIEs (§6.4).
        self.scopes.clear();
        self.scopes.extend(lines.iter().map(|r| ScopeRow { offset: r.offset, loc: r.loc }));
        self.locals.clear();
        self.locals.extend(vars.iter().map(|v| LocalRow {
            node: v.var,
            loc: v.loc,
            expr: v.expr,
            place: match v.place {
                VarPlace::Frame(o) => LocalPlace::Frame(o),
                VarPlace::FrameIndirect(o) => LocalPlace::FrameIndirect(o),
                VarPlace::Label(o) => LocalPlace::Label(o),
                VarPlace::Refused(name) => LocalPlace::Refused(name),
                VarPlace::Slot(_) | VarPlace::Indirect(_) => LocalPlace::Refused(DEBUG_FRAME_PLACE),
            },
        }));
        let input = FunctionInput {
            subprogram: sp,
            code,
            frame_base: frame.frame_base,
            scopes: &self.scopes,
            vars: &self.locals,
        };
        self.units[unit].plan.add_function(&input).map_err(dwarf_err)?;
        if lines.is_empty() {
            return Ok(());
        }

        let mut transitions = std::mem::take(&mut self.transitions);
        transitions.clear();
        transitions.reserve(lines.len());
        for row in lines {
            let debug = self.debug;
            let loc = debug.location(row.loc).map(|l| SourceLoc {
                file: self.file_of(unit, l.scope),
                line: l.line,
                column: l.column,
            });
            transitions.push(Transition { offset: row.offset, loc, block_start: row.block_start });
        }
        let f = FunctionLines { scope, size, transitions: &transitions, epilogue: true };
        self.rows.clear();
        let made = function_rows(&f, &mut self.rows);
        self.transitions = transitions;
        made.map_err(dwarf_err)?;
        let u = &mut self.units[unit];
        let seq = Sequence { order: 0, symbol, addend, size, rows: &self.rows };
        u.program.push(u.plan.files().files().len(), &seq).map_err(dwarf_err)
    }

    /// Writes `.debug_line` and the DIEs of every unit that has code or
    /// globals. `place` gives a variable's symbol and addend for its
    /// address, if it was placed.
    ///
    /// # Errors
    /// [`unsupported::DEBUG_DIE`]; an internal error for what the writers
    /// refuse.
    pub(super) fn finish(
        &mut self,
        module: &Module,
        obj: &mut Object,
        mut place: impl FnMut(&mut Object, SymId) -> Option<(SymbolId, i64)>,
    ) -> CgResult<()> {
        // Each unit's placed globals.
        let mut globals: Vec<Vec<GlobalAddr>> = vec![Vec::new(); self.units.len()];
        for &(sym, expr) in module.global_exprs() {
            let Some(unit) = self.units.iter().position(|u| u.globals.binary_search(&expr).is_ok())
            else {
                continue;
            };
            if let Some((symbol, addend)) = place(obj, sym) {
                globals[unit].push(GlobalAddr { expr, symbol, addend });
            }
        }
        for u in &mut self.units {
            u.plan.seal().map_err(dwarf_err)?;
            u.plan.place_symbols(|s| place(obj, s));
        }
        // `llc` writes nothing for a unit with neither code nor globals
        // (`spec/dwarf.md` §1.2).
        let written: Vec<usize> = (0..self.units.len())
            .filter(|&i| !self.units[i].code.is_empty() || !globals[i].is_empty())
            .collect();
        if written.is_empty() {
            return Ok(());
        }
        let line_units: Vec<LineUnit<'_>> = written
            .iter()
            .map(|&i| LineUnit {
                header: self.units[i].plan.files().header(),
                program: &self.units[i].program,
            })
            .collect();
        let lines = write_debug_line_units(obj, &line_units).map_err(dwarf_err)?;
        let inputs: Vec<UnitInput<'_, '_>> = written
            .iter()
            .zip(&lines.offsets)
            .filter(|&(&i, _)| self.units[i].dies)
            .map(|(&i, &line_offset)| UnitInput {
                plan: &self.units[i].plan,
                line_offset,
                globals: &globals[i],
                code: &self.units[i].code,
            })
            .collect();
        if inputs.is_empty() {
            return Ok(()); // only `DebugDirectivesOnly` units
        }
        write_debug_info(obj, lines.sections.line, &inputs).map_err(dwarf_err)?;
        Ok(())
    }
}

/// The symbol and addend a variable's address is relocated against, as
/// `llc` writes it: its own symbol, or its section's symbol plus its
/// offset for a local one.
pub(super) fn variable_address(
    obj: &mut Object,
    binding: Binding,
    section: Option<SectionId>,
    value: u64,
    own: SymbolId,
) -> (SymbolId, i64) {
    match (binding, section) {
        (Binding::Local, Some(section)) => (obj.section_symbol(section), value as i64),
        _ => (own, 0),
    }
}
