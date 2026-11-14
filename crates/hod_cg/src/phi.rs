//! The `phi` protocol: `spec/codegen.md` §4.5.
//!
//! A `phi` has, besides its location as a value, an **incoming slot**.
//! Every terminator stores, for every successor, the value each of the
//! successor's `phi`s lists for the terminator's block into that `phi`'s
//! incoming slot; at the head of its block a `phi` is a load from its
//! incoming slot. The values read are value locations and the slots
//! written are incoming slots, and the two are disjoint: no edge is
//! split, no parallel copy is sequenced, and a swap or a self-loop is
//! not a special case.
//!
//! The copies go through scratch registers only, so they change no
//! register's contents that the policy knows about: a terminator that
//! was fused with a deferred compare still finds the compare's operands
//! where they were.

use hod_ir::{Block, BlockId, InstKind, Value, ValueKind};

use crate::cx::FnCx;
use crate::error::CgResult;
use crate::frame::FrameRef;
use crate::loc::{Class, Key, Kind, Loc, NO_SLOT};
use crate::target::Target;

impl<'a, T: Target> FnCx<'a, T> {
    /// The incoming slot of `phi`, allocated at its first need.
    fn incoming_slot(&mut self, key: Key) -> CgResult<FrameRef> {
        let state = self.ensure(key)?;
        if state.incoming != NO_SLOT {
            return Ok(FrameRef::Local(state.incoming));
        }
        let ty = self.body.inst(hod_ir::InstId::new(key.0)).ty();
        let slot = self.home_slot(state.class, ty, false)?;
        let FrameRef::Local(at) = slot else {
            return self.bug("an incoming slot outside the local area");
        };
        if let Some(entry) = self.state_mut(key) {
            entry.incoming = at;
        }
        Ok(slot)
    }

    /// Writes the incoming slots of the `phi`s of `succ` for the edge
    /// from the block being emitted (§4.5), and releases the values
    /// read. Does nothing the second time the same terminator asks for
    /// the same successor.
    ///
    /// The driver calls this for every successor of a terminator before
    /// the terminator is lowered, and the call sequence calls it for the
    /// two destinations of an `invoke` at their places (§7.1), so a
    /// lowering does not normally call it.
    ///
    /// # Errors
    /// `Internal` for a `phi` that does not list the block, and what
    /// the copies report.
    pub fn phi_stores(&mut self, succ: BlockId) -> CgResult<()> {
        let Some(mark) = self.s.phi_mark.get_mut(succ.index()) else {
            return self.bug("a successor that is not a block of this body");
        };
        let stamp = self.cur.raw() + 1;
        if *mark == stamp {
            return Ok(());
        }
        *mark = stamp;
        self.phi_stores_edge(succ)
    }

    /// [`FnCx::phi_stores`] for one edge, every time it is asked: the
    /// suspend switch of a coroutine writes each edge's slots on that
    /// edge (WP-106, §7.6), and two of its edges may go to one block.
    pub(crate) fn phi_stores_edge(&mut self, succ: BlockId) -> CgResult<()> {
        if succ.index() >= self.body.block_count() {
            return self.bug("a successor that is not a block of this body");
        }
        let block = self.body.block(succ);
        if !block.has_phis() {
            return Ok(());
        }
        let from = self.block;
        for phi in block.phis() {
            let InstKind::Phi(list) = phi.kind() else { continue };
            let Some(value) = list.value_for(from) else {
                return self.bug("a phi of a successor does not list this block");
            };
            let key = Key::inst(phi.id().raw());
            let state = self.ensure(key)?;
            if state.kind != Kind::Dead {
                let slot = self.incoming_slot(key)?;
                self.store_incoming(value, slot, state.class, phi.ty())?;
            }
            self.release(value);
        }
        Ok(())
    }

    /// Copies `value` to the incoming slot `slot` of a `phi` of `class`
    /// and type `ty`, through scratch registers only.
    fn store_incoming(
        &mut self,
        value: Value,
        slot: FrameRef,
        class: Class,
        ty: hod_ir::TypeId,
    ) -> CgResult<()> {
        let t = self.target;
        let bytes = self.home_bytes(class, ty);
        if Key::of(value).is_none() {
            if !matches!(value.kind(), ValueKind::Const(_) | ValueKind::Sym(_) | ValueKind::Slot(_))
            {
                return self.bug("an incoming value that is no value");
            }
            if class == Class::Mem {
                return t.materialize_mem(self, slot, value);
            }
            let scratch = self.scratch_reg(class);
            t.materialize(self, scratch, value)?;
            t.store(self, scratch, slot, bytes);
            return Ok(());
        }
        // A deferred value is emitted here (it cannot be one for valid
        // facts: a use by a phi is never "the next instruction").
        let loc = match self.loc(value)? {
            Loc::Deferred => Loc::Reg(self.use_reg(value)?),
            loc => loc,
        };
        match loc {
            Loc::Reg(reg) => t.store(self, reg, slot, bytes),
            Loc::Frame(from) if class == Class::Mem => {
                let size = self.types().alloc_size_of(ty);
                let Ok(size) = u32::try_from(size) else {
                    return self.bug("a value larger than a frame");
                };
                t.copy(self, slot, from, size);
            }
            Loc::Frame(from) => t.copy(self, slot, from, bytes),
            Loc::None | Loc::Deferred => {
                return self.bug("an incoming value that has no location");
            }
        }
        Ok(())
    }

    /// At the head of `block`: gives each `phi` its value from its
    /// incoming slot, placed by §4.1. A home value is copied to its
    /// home, a register value is loaded into a pool register, a dead
    /// `phi` costs nothing.
    pub(crate) fn phi_loads(&mut self, block: Block<'a>) -> CgResult<()> {
        let t = self.target;
        for phi in block.phis() {
            let key = Key::inst(phi.id().raw());
            let state = self.ensure(key)?;
            match (state.kind, state.loc) {
                (Kind::Dead, _) => {}
                (Kind::Home, Loc::Frame(home)) => {
                    let slot = self.incoming_slot(key)?;
                    let bytes = if state.class == Class::Mem {
                        let size = self.types().alloc_size_of(phi.ty());
                        let Ok(size) = u32::try_from(size) else {
                            return self.bug("a value larger than a frame");
                        };
                        size
                    } else {
                        self.home_bytes(state.class, phi.ty())
                    };
                    t.copy(self, home, slot, bytes);
                }
                (Kind::Reg, _) => {
                    let slot = self.incoming_slot(key)?;
                    let reg = self.alloc(state.class)?;
                    let bytes = self.home_bytes(state.class, phi.ty());
                    t.load(self, reg, slot, bytes);
                    self.own(reg, key);
                }
                _ => return self.bug("a phi without a kind"),
            }
        }
        Ok(())
    }
}
