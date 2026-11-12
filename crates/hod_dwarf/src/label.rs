//! Labels beyond WP-057's (`spec/dwarf.md` §6.14, WP-098): a `DILabel`
//! that is artificial, marks a coroutine's suspend point
//! (`coroSuspendIdx`, LLVM 21 and later: the `__coro_resume_N` labels
//! `CoroSplit` adds to a coroutine's clones), or has no name. `llc`'s
//! `applyLabelAttributes` writes, in this order: `DW_AT_name` unless the
//! name is empty, `DW_AT_decl_file`/`DW_AT_decl_line` unless the line is
//! 0 (then `DW_AT_decl_column` if the column is not 0),
//! `DW_AT_artificial`, `DW_AT_LLVM_coro_suspend_idx`; a concrete label
//! then has `DW_AT_low_pc`, an abstract one (§6.9) none. A label's
//! concrete copy in inlined code is `abstract_origin` and `low_pc`
//! whatever the label (family 80). Plain labels keep WP-057's families
//! (39, 71); the others take family [`fam::LABEL_ANY`].

use hod_ir::DiLabel;
use hod_obj::SymbolId;

use crate::Error;
use crate::abbrev::{code, fam};
use crate::die::{UnitPlan, Writer};
use crate::leb::write_uleb;

/// Whether WP-057's families write `n`: a named label that is neither
/// artificial nor a suspend point.
pub(crate) fn is_plain(n: &DiLabel<'_>) -> bool {
    !n.name.is_empty() && !n.is_artificial && n.coro_suspend_idx.is_none()
}

impl<'a> Writer<'a> {
    /// A label that is not [`is_plain`]: concrete at `at` (the
    /// function's symbol and the label's offset from it), or abstract
    /// (`None`).
    pub(crate) fn label_any(
        &mut self,
        plan: &UnitPlan<'a>,
        node: hod_ir::DiId,
        n: &DiLabel<'a>,
        at: Option<(SymbolId, i64)>,
    ) -> Result<(), Error> {
        let b = u32::from;
        let decl = Writer::decl_of(plan, node, n.file, n.line)?;
        let column = decl.is_some() && n.column != 0;
        let mask = b(!n.name.is_empty())
            | b(decl.is_some()) << 1
            | b(column) << 2
            | b(n.is_artificial) << 3
            | b(n.coro_suspend_idx.is_some()) << 4
            | b(at.is_some()) << 5;
        self.abbrev(code(fam::LABEL_ANY, mask, false));
        if !n.name.is_empty() {
            self.strx(n.name)?;
        }
        self.decl(decl);
        if column {
            write_uleb(&mut self.info, u64::from(n.column));
        }
        if let Some(idx) = n.coro_suspend_idx {
            write_uleb(&mut self.info, u64::from(idx));
        }
        if let Some((symbol, addend)) = at {
            let index = self.push_addr(symbol, addend);
            write_uleb(&mut self.info, u64::from(index));
        }
        Ok(())
    }
}
