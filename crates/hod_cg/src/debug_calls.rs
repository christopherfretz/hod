//! The calls of a function whose subprogram describes them (WP-113,
//! `spec/dwarf.md` §6.19, `spec/codegen.md` §9.5): one row per call
//! instruction an IR call or invoke became, with the offset `hod_dwarf`
//! writes as `DW_AT_call_return_pc` (or, for a `musttail` call, the
//! offset of the branch, `DW_AT_call_pc`) and the callee.
//!
//! The engine arms a row before it asks the target for the call
//! ([`Target::emit_call`](crate::Target::emit_call)); the target reports
//! where the call returns with [`FnCx::call_returns_here`], right after
//! the call instruction, and where a tail exit branches with
//! [`FnCx::tail_branch_here`], right before the branch, and where that
//! branch ends with [`FnCx::tail_branch_ends_here`], right after it
//! (WP-163: DWARF 4's GNU call site names the end). A row the target did
//! not complete is an internal error: a return address is never guessed. Calls the code generator makes on its own (the runtime,
//! `memcpy`) get no row, as `llc` writes no call site for an external
//! symbol.
//!
//! A tail exit of such a function also gets a line row at its start,
//! the `musttail` call's location (`llc` gives the exit sequence the
//! call's location): a debugger finds the line of a tail call's frame
//! at its `DW_AT_call_pc`.

use hod_dwarf::{CallOrigin, CallSiteInput};
use hod_ir::{DiFlags, DiId, DiNode, LocId, Module, SymId, SymbolKind};

use crate::call::Callee;
use crate::cx::FnCx;
use crate::error::{CgError, CgResult};
use crate::record::{LineRow, ROW_CONT};
use crate::reg::PReg;
use crate::sym::SymRef;
use crate::target::{CallTarget, Target};

/// A row not completed yet.
const PENDING: u32 = u32::MAX;

/// What a described call calls.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DebugCallee {
    /// A function of the module (its subprogram, if any, is the call
    /// site's origin).
    Sym(SymId),
    /// An indirect call: the DWARF register of the target.
    Reg(u16),
    /// A call that cannot be described exactly: the function is refused
    /// under this stable name.
    Refused(&'static str),
    /// A `musttail` call with no call site (of an alias or a symbol of
    /// another module): its row places the tail exit's line only.
    NoSite,
}

/// One described call (§9.5).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct DebugCallRow {
    /// The offset after the call instruction; for a tail call, of the
    /// branch.
    pub offset: u32,
    /// The callee.
    pub callee: DebugCallee,
    /// Whether the call is a `musttail` call (a branch).
    pub tail: bool,
    /// The offset after the call instruction or the branch (WP-163: a
    /// tail call's as the target reports it, `spec/dwarf.md` §6.21).
    pub end: u32,
    /// The call's location (a tail exit's line row).
    pub loc: LocId,
}

impl<T: Target> FnCx<'_, T> {
    /// Whether the function's subprogram has `DIFlagAllCallsDescribed`.
    fn describes_calls(&self) -> bool {
        if !self.lines {
            return false;
        }
        let SymbolKind::Function { subprogram: Some(sp), .. } =
            self.module.symbol(self.body.symbol()).kind()
        else {
            return false;
        };
        matches!(self.module.debug.node(sp),
            DiNode::Subprogram(s) if s.flags.0 & DiFlags::ALL_CALLS_DESCRIBED.0 != 0)
    }

    /// Arms the row of a call of `callee` through `target`, before the
    /// call is emitted (or, for a `musttail` call, `tail`, before its
    /// branch to the tail exit): `true` when a row was armed.
    pub(crate) fn arm_debug_call(
        &mut self,
        callee: Callee,
        target: CallTarget,
        tail: bool,
    ) -> bool {
        if tail {
            self.key_tail_call();
        }
        let callee = match (callee, target) {
            (Callee::Temp(_), _) => DebugCallee::Refused(hod_dwarf::DEBUG_CALL_SITE_INTRINSIC),
            (Callee::Value(_), CallTarget::Sym(SymRef::Module(sym))) => DebugCallee::Sym(sym),
            (Callee::Value(_), CallTarget::Sym(_)) if tail => DebugCallee::NoSite,
            (Callee::Value(_), CallTarget::Reg(_)) => DebugCallee::Reg(u16::MAX),
            // The runtime, or a symbol that is no function of the module:
            // no call site.
            (Callee::Sym(_) | Callee::Value(_), _) => return false,
        };
        if !self.describes_calls() {
            return false;
        }
        let loc = self.line_loc();
        self.out.debug_calls.push(DebugCallRow {
            offset: PENDING,
            callee,
            tail,
            loc,
            end: PENDING,
        });
        true
    }

    /// Reports that the call instruction the target just emitted for
    /// `target` returns here (`Target::emit_call`, right after the
    /// instruction); `dwarf` numbers an indirect callee's register.
    pub fn call_returns_here(&mut self, target: CallTarget, dwarf: fn(PReg) -> u16) {
        self.key_call_returns(target);
        let offset = self.offset();
        let first = self.mark.debug_calls as usize;
        if let Some(row) = self.out.debug_calls[first..].last_mut()
            && row.offset == PENDING
            && !row.tail
        {
            row.offset = offset;
            row.end = offset;
            if let (DebugCallee::Reg(_), CallTarget::Reg(reg)) = (row.callee, target) {
                row.callee = DebugCallee::Reg(dwarf(reg));
            }
        }
    }

    /// Reports that tail exit `index` (§5.4) branches here (right before
    /// the branch, its sequence emitted from its label on); `dwarf`
    /// numbers an indirect callee's register.
    pub fn tail_branch_here(&mut self, index: usize, dwarf: fn(PReg) -> u16) {
        let offset = self.offset();
        let exit = self.tail_exit(index);
        let start = self.s.labels.offset(exit.label);
        let first = self.mark.debug_calls as usize;
        let Some(row) = self.out.debug_calls[first..].iter_mut().filter(|r| r.tail).nth(index)
        else {
            return;
        };
        if row.offset != PENDING {
            return;
        }
        row.offset = offset;
        if let (DebugCallee::Reg(_), CallTarget::Reg(reg)) = (row.callee, exit.target) {
            row.callee = DebugCallee::Reg(dwarf(reg));
        }
        // The exit's line: rows are in offset order, and every row so far
        // is at or before the exit's start (the body, the epilogue, the
        // exits before this one).
        let loc = row.loc;
        if let Some(start) = start
            && self.lines
            && self.out.lines.last().is_none_or(|r| r.offset < start)
        {
            // §5.9 K6: the exit continues the call's row.
            let stmt = self.row_stmt();
            self.out.lines.push(LineRow {
                stmt,
                marks: ROW_CONT,
                ..LineRow::new(start, loc, false)
            });
            self.last_loc = Some(loc);
        }
    }

    /// Reports that the branch of tail exit `index` the target just wrote
    /// (after [`FnCx::tail_branch_here`]) ends here: its length is the
    /// target's, whatever instruction it chose (WP-163).
    pub fn tail_branch_ends_here(&mut self, index: usize) {
        let end = self.offset();
        let first = self.mark.debug_calls as usize;
        if let Some(row) = self.out.debug_calls[first..].iter_mut().filter(|r| r.tail).nth(index)
            && row.offset != PENDING
            && row.end == PENDING
        {
            row.end = end;
        }
    }

    /// The rows' offsets after the function's code shrank
    /// (`spec/codegen.md` §5.6.15): a call returns, and a tail exit
    /// branches, where its bytes moved to. A row not completed stays so
    /// for [`FnCx::finish_debug_calls`] to report.
    pub(crate) fn map_debug_calls(&mut self, map: impl Fn(u32) -> u32) {
        let first = self.mark.debug_calls as usize;
        for row in &mut self.out.debug_calls[first..] {
            map_row(row, &map);
        }
    }

    /// After `Target::emit_call`: the armed row is complete.
    pub(crate) fn check_debug_call(&self, armed: bool) -> CgResult<()> {
        if armed && self.out.debug_calls.last().is_none_or(|r| r.offset == PENDING) {
            return self.bug("a target's emit_call did not report where its call returns");
        }
        Ok(())
    }

    /// At the end of the function: every row is complete, tail calls'
    /// included.
    pub(crate) fn finish_debug_calls(&self) -> CgResult<()> {
        let first = self.mark.debug_calls as usize;
        let rows = &self.out.debug_calls[first..];
        if rows.iter().any(|r| {
            r.offset == PENDING || r.end == PENDING || r.callee == DebugCallee::Reg(u16::MAX)
        }) {
            return self.bug("a described call without its offset or register");
        }
        Ok(())
    }
}

/// The bridge (`emit::lines`): the call sites `hod_dwarf` writes for the
/// rows `rows` of the function of subprogram `sp`, into `out`. A call of
/// a function without a subprogram, or of what is no function (an
/// alias), has none, as in `llc`.
///
/// # Errors
/// The stable name of a row that cannot be described; for a function
/// that describes its calls, [`hod_dwarf::DEBUG_CALL_SITES`] in a module
/// that is not DWARF 5 and [`hod_dwarf::DEBUG_CALL_SITE_INTRINSIC`] for a
/// coroutine body.
pub(crate) fn call_inputs(
    module: &Module,
    sp: DiId,
    rows: &[DebugCallRow],
    coroutine: bool,
    out: &mut Vec<CallSiteInput>,
) -> CgResult<()> {
    out.clear();
    let describes = matches!(module.debug.node(sp),
        DiNode::Subprogram(s) if s.flags.0 & DiFlags::ALL_CALLS_DESCRIBED.0 != 0);
    if !describes {
        return Ok(());
    }
    // WP-150: version 4 (no flag is version 4, `spec/dwarf.md` §6.20)
    // writes the GNU forms (§6.21).
    if !matches!(module.props.dwarf_version, None | Some(4 | 5)) {
        return Err(CgError::unsupported(hod_dwarf::DEBUG_CALL_SITES));
    }
    if coroutine {
        return Err(CgError::unsupported(hod_dwarf::DEBUG_CALL_SITE_INTRINSIC));
    }
    for row in rows {
        let origin = match row.callee {
            DebugCallee::Refused(name) => return Err(CgError::unsupported(name)),
            DebugCallee::NoSite => continue,
            DebugCallee::Reg(reg) => CallOrigin::Register(reg),
            DebugCallee::Sym(sym) => match module.symbol(sym).kind() {
                SymbolKind::Function { subprogram: Some(callee), .. } => {
                    CallOrigin::Subprogram(callee)
                }
                _ => continue,
            },
        };
        out.push(CallSiteInput { offset: row.offset, origin, tail: row.tail, end: row.end });
    }
    Ok(())
}

/// A row's offsets moved by the shrink pass (`spec/codegen.md` §5.6.15):
/// the call's or branch's, and its end; a pending one stays pending.
fn map_row(row: &mut DebugCallRow, map: &impl Fn(u32) -> u32) {
    for at in [&mut row.offset, &mut row.end] {
        if *at != PENDING {
            *at = map(*at);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// WP-163: a tail call's end is the target's, mapped as its start is:
    /// a branch that keeps its length keeps it when bytes before it go,
    /// and one the pass shortened (bytes removed inside it) ends where
    /// its last byte now is.
    #[test]
    fn a_tail_branch_end_moves_with_the_code() {
        let row = |offset, end| DebugCallRow {
            offset,
            callee: DebugCallee::NoSite,
            tail: true,
            loc: LocId::NONE,
            end,
        };
        // Three bytes removed at 10..13 (a `jmp rel32` relaxed to `rel8`
        // before the exit).
        let map = |at: u32| if at >= 13 { at - 3 } else { at.min(10) };
        let mut r = row(40, 43);
        map_row(&mut r, &map);
        assert_eq!((r.offset, r.end), (37, 40), "a 3-byte `jmp *%r11` stays 3 bytes");
        let mut r = row(40, 45);
        map_row(&mut r, &map);
        assert_eq!(r.end - r.offset, 5, "a `jmp rel32` stays 5 bytes");
        // The branch itself shortened: 3 of its bytes removed.
        let map = |at: u32| if at >= 45 { at - 3 } else { at.min(42) };
        let mut r = row(40, 45);
        map_row(&mut r, &map);
        assert_eq!(r.end - r.offset, 2, "the end follows the bytes written");
        let mut r = row(PENDING, PENDING);
        map_row(&mut r, &map);
        assert_eq!((r.offset, r.end), (PENDING, PENDING));
    }
}
