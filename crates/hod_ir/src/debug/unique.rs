//! Uniquing (`spec/ir.md` §6.3): two nodes LLVM would unique and whose
//! contents are equal are one node, as LLVM's parsers make them.
//!
//! A reader hands out ids before it knows a node's content (a node may
//! name a later one), so `add`'s hash-consing cannot see that `!17` and
//! `!142` are equal. [`DebugInfo::unique_nodes`] does, once the nodes are
//! defined: equality is judged on the contents with every reference
//! replaced by the class of the node it names, operands before users
//! (Tarjan's strongly connected components, an iterative walk); inside a
//! cycle, rounds repeat until nothing more merges. The lowest id of each
//! class survives; every reference is rewritten to it.

use super::{DISTINCT, DebugInfo, Head, LOC_ATOM, LOC_INTERNED, MERGED, atom_of, loc_hash};
use crate::ids::DiId;
use crate::intern::{Hasher64, InternTable, Layering};

use super::node::{Dec, DiKind, DiNode};

/// Where [`DebugInfo::unique_nodes`] starts: the node and location counts
/// of a moment, from [`DebugInfo::node_mark`]. The default is the start
/// of the tables.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct DiMark {
    nodes: u32,
    locs: u32,
}

/// What [`DebugInfo::unique_nodes`] merged: the id that now stands for
/// each node id.
#[derive(Clone, PartialEq, Eq, Debug, Default)]
pub struct DiMerge {
    from: u32,
    /// For the ids `from..`, the surviving id; empty when nothing was
    /// merged.
    to: Vec<DiId>,
}

impl DiMerge {
    /// Whether nothing was merged: every id stands for itself.
    #[inline]
    pub fn is_empty(&self) -> bool {
        self.to.is_empty()
    }

    /// The id that stands for `id`: `id` itself unless it was merged.
    #[inline]
    pub fn get(&self, id: DiId) -> DiId {
        id.raw()
            .checked_sub(self.from)
            .and_then(|at| self.to.get(at as usize))
            .copied()
            .unwrap_or(id)
    }

    /// The number of nodes merged into another.
    pub fn merged(&self) -> usize {
        self.to.iter().enumerate().filter(|&(at, id)| id.raw() != self.from + at as u32).count()
    }
}

/// Not a number of the walk yet.
const UNSEEN: u32 = u32::MAX;

/// The class `id` is in now: its chain of keys followed to the end. An
/// id below `from`, or not a node, is its own class.
#[inline]
fn root(key: &[u32], from: u32, mut id: u32) -> u32 {
    while let Some(&next) = id.checked_sub(from).and_then(|at| key.get(at as usize)) {
        if next == id {
            break;
        }
        id = next;
    }
    id
}

/// The state of one pass: the nodes `from..from + n`, the cells of each
/// that hold node ids, and the class of each.
struct Pass<'p> {
    from: u32,
    /// Absolute cell indices of the node ids, node by node.
    refs: &'p [u32],
    ref_start: &'p [u32],
    /// The number of cells of each node, its mask included; 0 for one
    /// that takes no part.
    extent: &'p [u32],
}

impl Pass<'_> {
    #[inline]
    fn refs_of(&self, id: u32) -> &[u32] {
        let at = (id - self.from) as usize;
        &self.refs[self.ref_start[at] as usize..self.ref_start[at + 1] as usize]
    }
}

impl<L: Layering> DebugInfo<L> {
    /// Where a later [`DebugInfo::unique_nodes`] starts: after every node
    /// and location there is now.
    pub fn node_mark(&self) -> DiMark {
        DiMark { nodes: self.heads.len() as u32, locs: self.locs.len() as u32 }
    }

    /// Whether node `id` was merged into an equal node by
    /// [`DebugInfo::unique_nodes`]: nothing in the tables names it any
    /// more.
    ///
    /// # Panics
    /// If `id` is not a node of this table.
    #[inline]
    pub fn is_merged(&self, id: DiId) -> bool {
        self.heads[id.index()].flags & MERGED != 0
    }

    /// Makes the nodes from `since` on unique, as LLVM's parsers do
    /// (§6.3): every node that is not distinct, is defined, and names
    /// neither itself nor a node LLVM's would not unique (`DiOther`) is
    /// merged with every node of equal content, references compared by
    /// the class of the node they name. A node that names itself becomes
    /// distinct. The lowest id of each class survives; every reference
    /// in the tables (nodes, locations from `since` on) is rewritten to
    /// it, the others are marked merged, and the survivors are what a
    /// later [`DebugInfo::add`] of equal content returns. The nodes
    /// before `since` must have been made unique already (or be the
    /// first nodes of the table): they are looked up, not merged.
    ///
    /// Returns the merge, for the caller's own references; one walk over
    /// the new nodes when nothing merges.
    pub fn unique_nodes(&mut self, since: DiMark) -> DiMerge {
        let from = since.nodes;
        let count = self.heads.len() as u32;
        if from >= count {
            return DiMerge::default();
        }
        let n = (count - from) as usize;

        // The cells of every node that holds a node id; a node that
        // names itself becomes distinct (LLVM: `storeDistinctInContext`).
        let mut refs: Vec<u32> = Vec::new();
        let mut ref_start: Vec<u32> = Vec::with_capacity(n + 1);
        let mut extent: Vec<u32> = vec![0; n];
        for at in 0..n {
            ref_start.push(refs.len() as u32);
            let id = from + at as u32;
            let head = self.heads[id as usize];
            if head.kind == 0 || head.flags & MERGED != 0 {
                continue;
            }
            let first = refs.len();
            let cells = self.walk_refs(head, &mut refs);
            let candidate = head.flags & DISTINCT == 0 && head.kind != DiKind::Other as u8;
            if candidate && refs[first..].iter().any(|&cell| self.cells[cell as usize].raw() == id)
            {
                self.heads[id as usize].flags |= DISTINCT;
                continue;
            }
            if candidate {
                extent[at] = cells;
            }
        }
        ref_start.push(refs.len() as u32);
        let pass = Pass { from, refs: &refs, ref_start: &ref_start, extent: &extent };

        let mut key: Vec<u32> = (from..count).collect();
        let mut table = InternTable::<crate::intern::NoLayer>::new();
        self.classes(&pass, &mut key, &mut table);

        // The survivor of each class: its lowest id.
        let mut low = vec![UNSEEN; n];
        let mut to: Vec<DiId> = Vec::with_capacity(n);
        let mut merged = false;
        for at in 0..n {
            let id = from + at as u32;
            let class = root(&key, from, id);
            let survivor = match class.checked_sub(from) {
                Some(slot) => {
                    let slot = slot as usize;
                    if low[slot] == UNSEEN {
                        low[slot] = id;
                    }
                    low[slot]
                }
                None => class,
            };
            merged |= survivor != id;
            to.push(DiId::new(survivor));
        }

        if merged {
            for (at, &survivor) in to.iter().enumerate() {
                if survivor.raw() != from + at as u32 {
                    self.heads[from as usize + at].flags |= MERGED;
                }
            }
            let map = |raw: u32| -> u32 {
                raw.checked_sub(from)
                    .and_then(|at| to.get(at as usize))
                    .map_or(raw, |survivor| survivor.raw())
            };
            for &cell in &refs {
                let raw = self.cells[cell as usize].raw();
                let new = map(raw);
                if new != raw {
                    self.cells[cell as usize] = DiId::new(new);
                }
            }
            for at in since.locs as usize..self.locs.len() {
                let record = self.locs[at];
                let scope = map(record.scope.raw());
                if scope == record.scope.raw() {
                    continue;
                }
                self.locs[at].scope = DiId::new(scope);
                if record.kind() == LOC_INTERNED {
                    // Found again under its new content.
                    let atom = (record.state & LOC_ATOM != 0)
                        .then(|| atom_of(&self.locs, &self.atoms, self.atom_index.as_ref(), at as u32));
                    let hash = loc_hash(&self.locs[at], atom);
                    let id = at as u32;
                    self.loc_interner.get_or_insert_with(InternTable::new).insert(hash, id);
                }
            }
            for unit in &mut self.units {
                *unit = DiId::new(map(unit.raw()));
            }
            self.edits = self.edits.wrapping_add(1);
        }

        // The survivors are what `add` finds from now on.
        for at in 0..n {
            let id = from + at as u32;
            if extent[at] == 0 || to[at].raw() != id {
                continue;
            }
            let head = self.heads[id as usize];
            let cells = self.cells.from(head.at as usize);
            let cells = &cells[..extent[at] as usize];
            let mut hasher = Hasher64::new(u64::from(head.kind) | (u64::from(head.tag) << 8));
            for cell in cells {
                hasher.word(u64::from(cell.raw()));
            }
            let hash = hasher.finish();
            let interner = self.interner.get_or_insert_with(InternTable::new);
            if interner.find(hash, |other| other == id).is_none() {
                interner.insert(hash, id);
            }
        }

        if merged { DiMerge { from, to } } else { DiMerge::default() }
    }

    /// The number of cells of the node `head` (its mask included);
    /// pushes the absolute index of every cell that holds a node id.
    fn walk_refs(&self, head: Head, refs: &mut Vec<u32>) -> u32 {
        let at = head.at as usize;
        let Some((mask, cells)) = self.cells.from(at).split_first() else { return 0 };
        let total = cells.len();
        let mut dec = Dec::<L> { cells, strings: None };
        let kind = DiKind::from_code(head.kind);
        DiNode::ref_cells(kind, mask.raw(), &mut dec, &mut |left| {
            refs.push((at + 1 + total - left) as u32);
        });
        (1 + total - dec.cells.len()) as u32
    }

    /// The hash of node `id`'s content with every reference replaced by
    /// its class: what [`DebugInfo::add`] computes for the node once the
    /// references are rewritten to classes.
    fn class_hash(&self, pass: &Pass<'_>, key: &[u32], id: u32) -> u32 {
        let head = self.heads[id as usize];
        let extent = pass.extent[(id - pass.from) as usize] as usize;
        let cells = &self.cells.from(head.at as usize)[..extent];
        let mut refs = pass.refs_of(id).iter().peekable();
        let mut hasher = Hasher64::new(u64::from(head.kind) | (u64::from(head.tag) << 8));
        for (offset, cell) in cells.iter().enumerate() {
            let mut raw = cell.raw();
            if refs.next_if(|&&at| at as usize == head.at as usize + offset).is_some() {
                raw = root(key, pass.from, raw);
            }
            hasher.word(u64::from(raw));
        }
        hasher.finish()
    }

    /// Whether node `id` (of the pass) and node `other` have equal
    /// contents, references compared by class. `other` is a node of the
    /// pass or a live node before it.
    fn same_class(&self, pass: &Pass<'_>, key: &[u32], id: u32, other: u32) -> bool {
        let (mine, theirs) = (self.heads[id as usize], self.heads[other as usize]);
        if mine.kind != theirs.kind
            || mine.tag != theirs.tag
            || theirs.flags & (DISTINCT | MERGED) != 0
            || theirs.kind == 0
        {
            return false;
        }
        let extent = pass.extent[(id - pass.from) as usize] as usize;
        let cells = &self.cells.from(mine.at as usize)[..extent];
        let Some(others) = self.cells.from(theirs.at as usize).get(..extent) else {
            return false;
        };
        // Equal masks and list lengths make equal layouts, so the cells
        // of `id` that hold node ids are those of `other` as long as
        // everything before them is equal.
        let mut refs = pass.refs_of(id).iter().peekable();
        for (offset, (cell, theirs)) in cells.iter().zip(others).enumerate() {
            let is_ref = refs.next_if(|&&at| at as usize == mine.at as usize + offset).is_some();
            let equal = if is_ref {
                root(key, pass.from, cell.raw()) == root(key, pass.from, theirs.raw())
            } else {
                cell == theirs
            };
            if !equal {
                return false;
            }
        }
        true
    }

    /// The class of node `id` among the nodes before the pass (already
    /// unique, in `interner`) and the classes of the pass in `table`.
    fn find_class(
        &self,
        pass: &Pass<'_>,
        key: &[u32],
        tables: &[&InternTable<crate::intern::NoLayer>],
        hash: u32,
        id: u32,
    ) -> Option<u32> {
        let before = self.interner.as_ref().and_then(|interner| {
            interner.find(hash, |other| {
                other < pass.from && self.same_class(pass, key, id, other)
            })
        });
        before.or_else(|| {
            tables.iter().find_map(|table| {
                table.find(hash, |other| other != id && self.same_class(pass, key, id, other))
            })
        })
    }

    /// Classes for the nodes of the pass: `key[id - from]` names the
    /// node `id` was found equal to (a chain [`root`] follows), or `id`.
    fn classes(
        &self,
        pass: &Pass<'_>,
        key: &mut [u32],
        table: &mut InternTable<crate::intern::NoLayer>,
    ) {
        let from = pass.from;
        let n = pass.extent.len();
        let part = |id: u32| -> Option<usize> {
            let at = id.checked_sub(from)? as usize;
            (at < n && pass.extent[at] != 0).then_some(at)
        };
        // Tarjan's components, iteratively: `order[at]` the visit number,
        // `low[at]` the lowest one reachable; components come out
        // operands first.
        let mut order = vec![UNSEEN; n];
        let mut low = vec![0u32; n];
        let mut on_stack = vec![false; n];
        let mut stack: Vec<u32> = Vec::new();
        let mut frames: Vec<(u32, u32)> = Vec::new();
        let mut next = 0u32;
        let mut members: Vec<u32> = Vec::new();
        for start in 0..n {
            if pass.extent[start] == 0 || order[start] != UNSEEN {
                continue;
            }
            frames.push((from + start as u32, 0));
            order[start] = next;
            low[start] = next;
            next += 1;
            stack.push(from + start as u32);
            on_stack[start] = true;
            while let Some(&mut (id, ref mut edge)) = frames.last_mut() {
                let at = (id - from) as usize;
                let edges = pass.refs_of(id);
                if let Some(&cell) = edges.get(*edge as usize) {
                    *edge += 1;
                    let Some(to) = part(self.cells[cell as usize].raw()) else { continue };
                    if order[to] == UNSEEN {
                        order[to] = next;
                        low[to] = next;
                        next += 1;
                        stack.push(from + to as u32);
                        on_stack[to] = true;
                        frames.push((from + to as u32, 0));
                    } else if on_stack[to] {
                        low[at] = low[at].min(order[to]);
                    }
                    continue;
                }
                frames.pop();
                if let Some(&(parent, _)) = frames.last() {
                    let parent = (parent - from) as usize;
                    low[parent] = low[parent].min(low[at]);
                }
                if low[at] != order[at] {
                    continue;
                }
                members.clear();
                while let Some(member) = stack.pop() {
                    on_stack[(member - from) as usize] = false;
                    members.push(member);
                    if member == id {
                        break;
                    }
                }
                self.component(pass, key, table, &mut members);
            }
        }
    }

    /// Classes for one component: a node alone is looked up once; the
    /// nodes of a cycle in rounds, until a round merges nothing, and
    /// then added to `table`.
    fn component(
        &self,
        pass: &Pass<'_>,
        key: &mut [u32],
        table: &mut InternTable<crate::intern::NoLayer>,
        members: &mut [u32],
    ) {
        let from = pass.from;
        if let [id] = *members {
            let hash = self.class_hash(pass, key, id);
            match self.find_class(pass, key, &[table], hash, id) {
                Some(found) => key[(id - from) as usize] = found,
                None => table.insert(hash, id),
            }
            return;
        }
        members.sort_unstable();
        loop {
            let mut round = InternTable::new();
            let mut changed = false;
            for &id in members.iter() {
                if key[(id - from) as usize] != id {
                    continue;
                }
                let hash = self.class_hash(pass, key, id);
                match self.find_class(pass, key, &[table, &round], hash, id) {
                    Some(found) => {
                        key[(id - from) as usize] = root(key, from, found);
                        changed = true;
                    }
                    None => round.insert(hash, id),
                }
            }
            if !changed {
                break;
            }
        }
        for &id in members.iter() {
            if key[(id - from) as usize] == id {
                let hash = self.class_hash(pass, key, id);
                table.insert(hash, id);
            }
        }
    }
}
