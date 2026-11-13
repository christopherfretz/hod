//! Call-site entries (`spec/dwarf.md` §6.19, WP-113).
//!
//! clang sets `DIFlagAllCallsDescribed` on every definition of an
//! optimized unit (`-O1` and up with `-g`, DWARF 5). `llc` then writes,
//! in `DwarfDebug::constructCallSiteEntryDIEs`, after the function's
//! scopes:
//!
//! - `DW_AT_call_all_calls` on the subprogram (after its frame base and
//!   object pointer, before what `finishSubprogramDefinitions` adds: the
//!   flag gets families of its own, [`family`]);
//! - one `DW_TAG_call_site` per call instruction, the subprogram's last
//!   children, in code order, whatever block or inlined code the call is
//!   in: `DW_AT_call_origin`, the callee's DIE, for a direct call of a
//!   function with a subprogram; `DW_AT_call_target_clobbered` (`llc`
//!   22; `DW_AT_call_target` in `llc` 19), `DW_OP_reg<n>` of the
//!   register that held the target, for an indirect call; then
//!   `DW_AT_call_return_pc`, the address after the call, or, for a tail
//!   call, `DW_AT_call_tail_call` and `DW_AT_call_pc`, the address of the
//!   branch. No entry for a call of a function without a subprogram, of
//!   an alias, of a library function the code generator calls on its own
//!   (`memcpy`, the runtime), and no call-site parameters at `-O0`.
//!
//! The callee's DIE is `getOrCreateSubprogramDIE`'s, made when the call
//! site is if it is new: a definition's concrete DIE, empty until its
//! function is added (the early DIE of [`crate::inline`]), even when the
//! function also has an abstract instance; a declaration's DIE in its
//! class, namespace or the unit, with its attributes and parameters.
//!
//! The code generator reports each call's offset and callee
//! ([`CallSiteInput`]); a return address that is not exact is worse than
//! none, so whatever is not known exactly is refused by name.

use hod_ir::{DiFlags, DiId, DiKind, DiNode, DiSubprogram, SpFlags};

use crate::Error;
use crate::abbrev::{code, fam};
use crate::die::{Kind, NONE, UnitPlan, Writer, bad, unsupported};
use crate::leb::write_uleb;

/// Call sites `hod_dwarf` does not write: those of a unit that is not
/// DWARF 5 (`llc` writes the GNU forms there, `DW_TAG_GNU_call_site`).
pub const DEBUG_CALL_SITES: &str = "debug:call-sites";
/// A call whose callee's DIE is not one `llc` makes the same way: a
/// definition of another unit, or one whose function the module has no
/// code of (`llc` writes an empty DIE for it).
pub const DEBUG_CALL_SITE_ORIGIN: &str = "debug:call-site-origin";
/// A call whose offset is not in its function's code.
pub const DEBUG_CALL_SITE_PC: &str = "debug:call-site-pc";
/// A call the code generator makes for an intrinsic through a computed
/// address (`llvm.coro.resume`, `llvm.coro.destroy`), or a coroutine
/// body's calls: `llc` describes the calls of the split functions.
pub const DEBUG_CALL_SITE_INTRINSIC: &str = "debug:call-site-intrinsic";
/// A `LineTablesOnly` function with inlined code that describes its
/// calls: `llc`'s minimal call sites are not written yet.
pub const DEBUG_CALL_SITES_LINE_TABLES: &str = "debug:call-sites-line-tables";

/// What a call calls.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CallOrigin {
    /// A function with this `DISubprogram` (a definition or a
    /// declaration).
    Subprogram(DiId),
    /// An indirect call: the DWARF register that held the target.
    Register(u16),
}

/// One call instruction of a placed function that describes its calls.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CallSiteInput {
    /// For a call, the offset of the instruction after it (the return
    /// address); for a tail call, of the branch.
    pub offset: u32,
    /// The callee.
    pub origin: CallOrigin,
    /// Whether the call is a tail call (a branch that does not return).
    pub tail: bool,
}

/// A call-site DIE's facts for the writer.
#[derive(Clone, Copy, Debug)]
struct Site {
    die: u32,
    func: u32,
    offset: u32,
    /// The callee's DIE, or `NONE` for an indirect call.
    origin: u32,
    reg: u16,
    tail: bool,
}

/// The unit's call sites.
#[derive(Clone, Debug, Default)]
pub(crate) struct Calls {
    /// By DIE (made in that order).
    sites: Vec<Site>,
    /// The definitions' DIEs made early for a call, to be filled by their
    /// functions.
    early: Vec<u32>,
}

impl Calls {
    fn site(&self, die: u32) -> Option<Site> {
        let i = self.sites.binary_search_by_key(&die, |s| s.die).ok()?;
        Some(self.sites[i])
    }
}

/// Whether `sp` describes its calls.
pub(crate) const fn describes(sp: &DiSubprogram<'_>) -> bool {
    sp.flags.0 & DiFlags::ALL_CALLS_DESCRIBED.0 != 0
}

/// The check before any code is generated: a definition that describes
/// its calls in a unit that is not DWARF 5 is refused (`llc`'s GNU
/// forms are not written).
pub(crate) fn check(dwarf_version: Option<u16>, sp: &DiSubprogram<'_>) -> Result<(), &'static str> {
    check_flags(dwarf_version, sp.flags)
}

/// [`check`] of a subprogram's `flags`.
fn check_flags(dwarf_version: Option<u16>, flags: DiFlags) -> Result<(), &'static str> {
    if flags.0 & DiFlags::ALL_CALLS_DESCRIBED.0 != 0 && dwarf_version != Some(5) {
        return Err(DEBUG_CALL_SITES);
    }
    Ok(())
}

/// The family of a definition: `base`'s, or with `DW_AT_call_all_calls`
/// when the function describes its calls.
pub(crate) const fn family(base: usize, calls: bool) -> usize {
    if !calls {
        return base;
    }
    match base {
        fam::SUBPROGRAM => fam::SUBPROGRAM_CALLS,
        fam::SUBPROGRAM_ANY => fam::SUBPROGRAM_ANY_CALLS,
        fam::SUBPROGRAM_MEMBER => fam::SUBPROGRAM_MEMBER_CALLS,
        fam::SUBPROGRAM_SPEC_TYPED => fam::SUBPROGRAM_SPEC_TYPED_CALLS,
        fam::SUBPROGRAM_SPEC_ANY => fam::SUBPROGRAM_SPEC_ANY_CALLS,
        fam::SUBPROGRAM_OF_ABSTRACT => fam::SUBPROGRAM_OF_ABSTRACT_CALLS,
        other => other,
    }
}

/// The family and mask of a C function's declaration (`DW_AT_prototyped`
/// in a C unit), or `None` for any other declaration: the family of
/// [`fam::SUBPROGRAM_DECL`] with the flag.
pub(crate) fn c_declaration(plan: &UnitPlan<'_>, die: u32) -> Option<(usize, u32)> {
    let b = u32::from;
    let DiNode::Subprogram(sp) = plan.debug.node(plan.dies[die as usize].node) else {
        return None;
    };
    let only = DiFlags::PROTOTYPED.0 | DiFlags::NO_RETURN.0;
    let c = crate::func::is_c(plan.cu.language);
    let sp_only = SpFlags::LOCAL_TO_UNIT.0 | SpFlags::OPTIMIZED.0;
    if !c
        || !sp.flags.contains(DiFlags::PROTOTYPED)
        || sp.flags.0 & !only != 0
        || sp.sp_flags.0 & !sp_only != 0
        || sp.name.is_empty()
    {
        return None;
    }
    let ty = match sp.ty.map(|t| plan.debug.node(t)) {
        Some(DiNode::SubroutineType(s)) => {
            if s.cc != 0 && s.cc != 1 {
                return None;
            }
            s.types.first().is_some_and(|&t| t != DiId::NONE)
        }
        _ => false,
    };
    let mask = b(!sp.linkage_name.is_empty())
        | b(sp.line != 0) << 1
        | b(ty) << 2
        | b(!sp.sp_flags.contains(SpFlags::LOCAL_TO_UNIT)) << 3
        | b(sp.flags.contains(DiFlags::NO_RETURN)) << 4;
    Some((fam::SUBPROGRAM_DECL_C, mask))
}

impl UnitPlan<'_> {
    /// The call sites of the function just added, whose subprogram DIE is
    /// `die` (`constructCallSiteEntryDIEs`): the children after its
    /// scopes, and the DIEs of their callees made if new. Returns `die`.
    pub(crate) fn add_call_sites(
        &mut self,
        sp_id: DiId,
        sp: &DiSubprogram<'_>,
        die: u32,
        calls: &[CallSiteInput],
        size: u64,
    ) -> Result<u32, Error> {
        if !describes(sp) {
            return if calls.is_empty() { Ok(die) } else { Err(bad(sp_id, "calls")) };
        }
        let func = self.funcs.len().checked_sub(1).ok_or(bad(sp_id, "subprogram"))?;
        self.funcs[func].calls = true;
        let func = func as u32;
        let mut last = 0;
        for c in calls {
            // Inside the code (a return address may be its end: a call
            // that is the last instruction); the returning calls in code
            // order (a tail call's row keeps its place among them, while
            // its branch is at a tail exit after the body).
            let inside =
                if c.tail { u64::from(c.offset) < size } else { u64::from(c.offset) <= size };
            if !inside || (!c.tail && (c.offset < last || c.offset == 0)) {
                return Err(unsupported(sp_id, DEBUG_CALL_SITE_PC));
            }
            if !c.tail {
                last = c.offset;
            }
            let entry = self.push_anon(sp_id, die, Kind::CallSite);
            self.code_addrs += 1;
            let (origin, reg) = match c.origin {
                CallOrigin::Subprogram(callee) => (self.call_origin(sp_id, callee)?, 0),
                CallOrigin::Register(reg) => (NONE, reg),
            };
            self.calls.sites.push(Site {
                die: entry,
                func,
                offset: c.offset,
                origin,
                reg,
                tail: c.tail,
            });
        }
        Ok(die)
    }

    /// `getOrCreateSubprogramDIE` of a callee.
    fn call_origin(&mut self, from: DiId, callee: DiId) -> Result<u32, Error> {
        if self.debug.find_kind(callee) != Some(DiKind::Subprogram) {
            return Err(bad(from, "callee"));
        }
        let DiNode::Subprogram(c) = self.debug.node(callee) else {
            return Err(bad(from, "callee"));
        };
        if !c.sp_flags.contains(SpFlags::DEFINITION) {
            return self.declaration_die(from, callee);
        }
        let seen = self.slot[callee.index()];
        if seen != NONE {
            return Ok(seen);
        }
        if c.unit != Some(self.unit) || !self.inline.has_body(callee) {
            return Err(unsupported(callee, DEBUG_CALL_SITE_ORIGIN));
        }
        let parent = self.subprogram_parent(callee, &c)?;
        let die = self.push(callee, parent, Kind::Subprogram);
        self.inline.mark_early(die);
        self.calls.early.push(die);
        Ok(die)
    }

    /// At sealing: every callee DIE made early has its function.
    pub(crate) fn seal_call_sites(&self) -> Result<(), Error> {
        if self.calls.early.is_empty() {
            return Ok(());
        }
        let mut filled: Vec<u32> = self.funcs.iter().map(|f| f.die).collect();
        filled.sort_unstable();
        for &die in &self.calls.early {
            if filled.binary_search(&die).is_err() {
                return Err(unsupported(self.dies[die as usize].node, DEBUG_CALL_SITE_ORIGIN));
            }
        }
        Ok(())
    }
}

/// `DW_OP_reg0`; `DW_OP_regx` beyond 31.
const DW_OP_REG0: u8 = 0x50;
const DW_OP_REGX: u8 = 0x90;

impl Writer<'_> {
    /// Writes call-site DIE `die` (§6.19).
    pub(crate) fn call_site_die(&mut self, plan: &UnitPlan<'_>, die: u32) -> Result<(), Error> {
        let b = u32::from;
        let s = plan.calls.site(die).ok_or(bad(plan.dies[die as usize].node, "call site"))?;
        let fd = plan.funcs[s.func as usize];
        let direct = s.origin != NONE;
        let mask = b(direct) | b(!direct) << 1 | b(s.tail) << 2 | b(!s.tail) << 3;
        self.abbrev(code(fam::CALL_SITE, mask, false));
        if direct {
            self.ref4(s.origin);
        } else if s.reg < 32 {
            self.info.extend_from_slice(&[1, DW_OP_REG0 + s.reg as u8]);
        } else {
            let mut op = vec![DW_OP_REGX];
            write_uleb(&mut op, u64::from(s.reg));
            write_uleb(&mut self.info, op.len() as u64);
            self.info.extend_from_slice(&op);
        }
        let index = self.push_addr(fd.code.symbol, fd.code.addend + i64::from(s.offset));
        write_uleb(&mut self.info, u64::from(index));
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::abbrev::{FAMILIES, at};

    /// Each `calls` family is its base with `DW_AT_call_all_calls` after
    /// the frame base (and the object pointer), the same tag, children
    /// and option bits.
    #[test]
    fn calls_families_are_their_bases_with_the_flag() {
        for base in [
            fam::SUBPROGRAM,
            fam::SUBPROGRAM_ANY,
            fam::SUBPROGRAM_MEMBER,
            fam::SUBPROGRAM_SPEC_TYPED,
            fam::SUBPROGRAM_SPEC_ANY,
            fam::SUBPROGRAM_OF_ABSTRACT,
        ] {
            let calls = family(base, true);
            assert_ne!(calls, base);
            assert_eq!(family(base, false), base);
            let (b, c) = (&FAMILIES[base], &FAMILIES[calls]);
            assert_eq!((b.tag, b.children, b.block), (c.tag, c.children, c.block), "{}", c.name);
            let flag = c.slots.iter().position(|s| s.at == at::CALL_ALL_CALLS).unwrap();
            assert_eq!(c.slots[flag].bit, 0, "{}", c.name);
            let mut without = c.slots.to_vec();
            without.remove(flag);
            assert_eq!(without.as_slice(), b.slots, "{}", c.name);
            let before = c.slots[flag - 1].at;
            assert!(before == at::FRAME_BASE || before == at::OBJECT_POINTER, "{}", c.name);
        }
        assert_eq!(family(fam::SUBPROGRAM_DECL, true), fam::SUBPROGRAM_DECL);
    }

    /// Only DWARF 5 units describe calls; a subprogram without the flag
    /// is never refused here.
    #[test]
    fn dwarf_version() {
        let plain = DiFlags::PROTOTYPED;
        for v in [None, Some(2), Some(3), Some(4), Some(5)] {
            assert_eq!(check_flags(v, plain), Ok(()));
        }
        let calls = DiFlags(DiFlags::ALL_CALLS_DESCRIBED.0 | DiFlags::PROTOTYPED.0);
        assert_eq!(check_flags(Some(5), calls), Ok(()));
        for v in [None, Some(4)] {
            assert_eq!(check_flags(v, calls), Err(DEBUG_CALL_SITES));
        }
    }
}
