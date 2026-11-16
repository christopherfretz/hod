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
    /// The hash of each candidate's content, as `add` computes it.
    content: &'p [u32],
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
    /// (§6.3): every node from `since` on that is defined, not distinct,
    /// not merged, not a `DiOther` (whose content hod does not know) and
    /// does not name itself is merged with every node of equal content,
    /// references compared by the class of the node they name. A node
    /// that names itself becomes distinct. The lowest id of each class survives; every reference
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

        // The cells of every candidate that hold a node id (the other
        // nodes are walked only if something merges); a node that names
        // itself becomes distinct (LLVM: `storeDistinctInContext`).
        let mut refs: Vec<u32> = Vec::new();
        let mut ref_start: Vec<u32> = Vec::with_capacity(n + 1);
        let mut extent: Vec<u32> = vec![0; n];
        let mut content: Vec<u32> = vec![0; n];
        for at in 0..n {
            ref_start.push(refs.len() as u32);
            let id = from + at as u32;
            let head = self.heads[id as usize];
            if head.kind == 0
                || head.flags & (MERGED | DISTINCT) != 0
                || head.kind == DiKind::Other as u8
            {
                continue;
            }
            let first = refs.len();
            let cells = self.walk_refs(head, &mut refs);
            if refs[first..].iter().any(|&cell| self.cells[cell as usize].raw() == id) {
                // Its cells stay in `refs`, to be rewritten.
                self.heads[id as usize].flags |= DISTINCT;
                continue;
            }
            extent[at] = cells;
            content[at] = self.content_hash(head, cells);
        }
        ref_start.push(refs.len() as u32);
        let pass =
            Pass { from, refs: &refs, ref_start: &ref_start, extent: &extent, content: &content };

        let mut key: Vec<u32> = (from..count).collect();
        let mut table = InternTable::<crate::intern::NoLayer>::new();
        self.classes(&pass, &mut key, &mut table);

        // The survivor of each class: its lowest id. Usually every node
        // is its own class.
        let merged = key.iter().zip(from..).any(|(&class, id)| class != id);
        let mut to: Vec<DiId> = Vec::new();
        if merged {
            let mut low = vec![UNSEEN; n];
            to.reserve_exact(n);
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
                to.push(DiId::new(survivor));
            }
        }

        if merged {
            self.apply(since, &to, &pass);
        }

        // The survivors are what `add` finds from now on. When nothing
        // merged, every class is one node named by its own id, so the
        // hash of the walk is the hash of its content.
        self.interner.get_or_insert_with(InternTable::new).reserve(n);
        for at in 0..n {
            let id = from + at as u32;
            if extent[at] == 0 || to.get(at).is_some_and(|survivor| survivor.raw() != id) {
                continue;
            }
            let hash = if merged {
                self.content_hash(self.heads[id as usize], extent[at])
            } else {
                content[at]
            };
            let interner = self.interner.get_or_insert_with(InternTable::new);
            if interner.find(hash, |other| other == id).is_none() {
                interner.insert(hash, id);
            }
        }

        if merged { DiMerge { from, to } } else { DiMerge::default() }
    }

    /// Applies a merge: `to[at]` is the survivor of node `from + at`.
    /// Marks the others merged, and rewrites every reference to a node
    /// of the pass: the cells of the nodes from `since` on, the scopes of
    /// the locations from `since` on, the compile-unit list.
    fn apply(&mut self, since: DiMark, to: &[DiId], pass: &Pass<'_>) {
        let from = pass.from;
        for (at, &survivor) in to.iter().enumerate() {
            if survivor.raw() != from + at as u32 {
                self.heads[from as usize + at].flags |= MERGED;
            }
        }
        let map = |raw: u32| -> u32 {
            raw.checked_sub(from).and_then(|at| to.get(at as usize)).map_or(raw, |s| s.raw())
        };
        // The candidates' cells are in `refs` (a merged node's too:
        // nothing names it, but it names only survivors); the distinct
        // and `DiOther` nodes are walked now.
        let mut more: Vec<u32> = Vec::new();
        for at in 0..to.len() {
            let head = self.heads[from as usize + at];
            let walked = pass.ref_start[at] != pass.ref_start[at + 1] || pass.extent[at] != 0;
            if head.kind != 0 && !walked {
                self.walk_refs(head, &mut more);
            }
        }
        for &cell in pass.refs.iter().chain(&more) {
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
                self.loc_interner.get_or_insert_with(InternTable::new).insert(hash, at as u32);
            }
        }
        for unit in &mut self.units {
            *unit = DiId::new(map(unit.raw()));
        }
        self.edits = self.edits.wrapping_add(1);
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

    /// The hash of the first `extent` cells of the node `head`: what
    /// [`DebugInfo::add`] computes for its content.
    fn content_hash(&self, head: Head, extent: u32) -> u32 {
        let cells = &self.cells.from(head.at as usize)[..extent as usize];
        let mut hasher = Hasher64::new(u64::from(head.kind) | (u64::from(head.tag) << 8));
        for cell in cells {
            hasher.word(u64::from(cell.raw()));
        }
        hasher.finish()
    }

    /// The hash of node `id`'s content with every reference replaced by
    /// its class: what [`DebugInfo::add`] computes for the node once the
    /// references are rewritten to classes.
    fn class_hash(&self, pass: &Pass<'_>, key: &[u32], id: u32) -> u32 {
        let from = pass.from;
        let refs = pass.refs_of(id);
        if refs.iter().all(|&cell| {
            let raw = self.cells[cell as usize].raw();
            root(key, from, raw) == raw
        }) {
            // Every node it names is its own class.
            return pass.content[(id - from) as usize];
        }
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
            interner.find(hash, |other| other < pass.from && self.same_class(pass, key, id, other))
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
        // The roots of this round with their hashes, searched in a small
        // cycle (most are a class and its members); a large one is
        // searched in a table of them too.
        let small = members.len() <= 16;
        let mut listed: Vec<(u32, u32)> = Vec::new();
        let mut round: Option<InternTable<crate::intern::NoLayer>> = None;
        loop {
            listed.clear();
            if !small {
                round = Some(InternTable::new());
            }
            let mut changed = false;
            for &id in members.iter() {
                if key[(id - from) as usize] != id {
                    continue;
                }
                let hash = self.class_hash(pass, key, id);
                let found = match &round {
                    None => self.find_class(pass, key, &[table], hash, id).or_else(|| {
                        listed.iter().find_map(|&(theirs, other)| {
                            (theirs == hash && self.same_class(pass, key, id, other))
                                .then_some(other)
                        })
                    }),
                    Some(round) => self.find_class(pass, key, &[table, round], hash, id),
                };
                match (found, &mut round) {
                    (Some(found), _) => {
                        key[(id - from) as usize] = root(key, from, found);
                        changed = true;
                    }
                    (None, round) => {
                        listed.push((hash, id));
                        if let Some(round) = round {
                            round.insert(hash, id);
                        }
                    }
                }
            }
            if !changed {
                break;
            }
        }
        // The last round merged nothing: its roots are the classes, under
        // their final hashes.
        for &(hash, id) in &listed {
            table.insert(hash, id);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::names::{DW_TAG_POINTER_TYPE, DW_TAG_STRUCTURE_TYPE, DiFlags};
    use super::super::node::{
        DiBasicType, DiBound, DiCompositeType, DiDerivedType, DiLexicalBlock, DiSubrange,
        DiSubroutineType, DiTuple,
    };
    use super::*;
    use crate::ids::LocId;
    use crate::intern::NoLayer;
    use crate::{DiLocation, IrErrorKind};

    fn int() -> DiNode<'static> {
        DiBasicType { name: b"int", size: 32, encoding: 5, ..Default::default() }.into()
    }

    fn pointer(to: DiId) -> DiNode<'static> {
        DiDerivedType {
            tag: DW_TAG_POINTER_TYPE,
            base_type: (!to.is_none()).then_some(to),
            size: 64,
            ..Default::default()
        }
        .into()
    }

    fn tuple(elements: &[DiId]) -> DiNode<'_> {
        DiTuple { elements }.into()
    }

    fn reserve(d: &mut DebugInfo, count: usize) -> Vec<DiId> {
        (0..count).map(|_| d.reserve().unwrap()).collect()
    }

    fn base(d: &DebugInfo, id: DiId) -> Option<DiId> {
        match d.node(id) {
            DiNode::DerivedType(t) => t.base_type,
            other => panic!("{id:?} is {other:?}"),
        }
    }

    fn elements(d: &DebugInfo, id: DiId) -> Vec<DiId> {
        match d.node(id) {
            DiNode::Tuple(t) => t.elements.to_vec(),
            other => panic!("{id:?} is {other:?}"),
        }
    }

    /// The nodes the reference cells of every node hold are the nodes
    /// `DiNode::refs` reports, in order: the cells the pass rewrites are
    /// exactly the references.
    fn check_ref_cells(d: &DebugInfo) {
        for raw in 0..d.heads.len() as u32 {
            let head = d.heads[raw as usize];
            if head.kind == 0 {
                continue;
            }
            let mut cells = Vec::new();
            let extent = d.walk_refs(head, &mut cells);
            let by_cells: Vec<DiId> = cells.iter().map(|&cell| d.cells[cell as usize]).collect();
            let mut by_node = Vec::new();
            d.node(DiId::new(raw)).refs(&mut |field, class, id| by_node.push((field, class, id)));
            // WP-290: the walk `verify` makes over the stored cells.
            let mut stored = Vec::new();
            let all = d.cells.from(head.at as usize);
            let mut dec = Dec::<NoLayer> { cells: &all[1..], strings: None };
            let kind = DiKind::from_code(head.kind);
            DiNode::stored_refs(kind, all[0].raw(), &mut dec, &mut |field, class, id| {
                stored.push((field, class, id));
            });
            assert_eq!(stored, by_node, "node {raw}");
            let by_node: Vec<DiId> = by_node.iter().map(|&(_, _, id)| id).collect();
            assert_eq!(by_cells, by_node, "node {raw}");
            // The walk ends where the node's cells end: at the next
            // node's, for nodes stored one after another.
            let next = d.heads.iter().map(|h| h.at).filter(|&at| at > head.at).min();
            if let Some(next) = next {
                assert_eq!(head.at + extent, next, "node {raw}");
            }
        }
    }

    /// Plain duplicates, and duplicates that name duplicates by a
    /// forward reference: the lowest id survives, every reference names
    /// it, and `add` finds it afterwards.
    #[test]
    fn duplicates_and_chains() {
        let mut d = DebugInfo::default();
        let ids = reserve(&mut d, 4);
        let [p1, p2, i1, i2] = ids[..] else { unreachable!() };
        d.define(p1, &pointer(i1)).unwrap();
        d.define(p2, &pointer(i2)).unwrap();
        d.define(i1, &int()).unwrap();
        d.define(i2, &int()).unwrap();
        let user = d.add(&tuple(&[p2, i2])).unwrap();
        check_ref_cells(&d);
        let merge = d.unique_nodes(DiMark::default());
        assert_eq!(merge.merged(), 2);
        assert_eq!(
            (merge.get(p2), merge.get(i2), merge.get(p1), merge.get(user)),
            (p1, i1, p1, user)
        );
        assert!(d.is_merged(p2) && d.is_merged(i2) && !d.is_merged(p1) && !d.is_merged(i1));
        assert_eq!(base(&d, p1), Some(i1));
        assert_eq!(elements(&d, user), [p1, i1]);
        assert_eq!(d.add(&int()).unwrap(), i1);
        assert_eq!(d.add(&pointer(i1)).unwrap(), p1);
        assert_eq!(d.add(&tuple(&[p1, i1])).unwrap(), user);
        check_ref_cells(&d);
        d.verify().unwrap();
        // Nothing left to merge: an empty merge.
        assert!(d.unique_nodes(DiMark::default()).is_empty());
    }

    /// A node that names itself is distinct (LLVM's
    /// `storeDistinctInContext`); the node with its content, which
    /// names it, is not merged with it.
    #[test]
    fn a_self_reference_is_distinct() {
        let mut d = DebugInfo::default();
        let ids = reserve(&mut d, 2);
        d.define(ids[0], &tuple(&[ids[0]])).unwrap();
        d.define(ids[1], &tuple(&[ids[0]])).unwrap();
        assert!(d.unique_nodes(DiMark::default()).is_empty());
        assert!(d.is_distinct(ids[0]) && !d.is_distinct(ids[1]));
        assert!(!d.is_merged(ids[0]) && !d.is_merged(ids[1]));
    }

    /// Inside one cycle, merging repeats: `!2 = !4` first, which makes
    /// `!1 = !3`. Cycles that are alike and share no node stay apart.
    #[test]
    fn cycles() {
        let mut d = DebugInfo::default();
        let n = reserve(&mut d, 9);
        d.define(n[0], &tuple(&[n[1], n[3]])).unwrap();
        d.define(n[1], &tuple(&[n[0], n[2]])).unwrap();
        d.define(n[2], &tuple(&[n[0]])).unwrap();
        d.define(n[3], &tuple(&[n[0], n[4]])).unwrap();
        d.define(n[4], &tuple(&[n[0]])).unwrap();
        d.define(n[5], &tuple(&[n[6]])).unwrap();
        d.define(n[6], &tuple(&[n[5]])).unwrap();
        d.define(n[7], &tuple(&[n[8]])).unwrap();
        d.define(n[8], &tuple(&[n[7]])).unwrap();
        let merge = d.unique_nodes(DiMark::default());
        assert_eq!(merge.merged(), 2);
        assert_eq!((merge.get(n[3]), merge.get(n[4])), (n[1], n[2]));
        assert_eq!(elements(&d, n[0]), [n[1], n[1]]);
        assert_eq!(elements(&d, n[1]), [n[0], n[2]]);
        assert!((5..9).all(|at| !d.is_merged(n[at])));
        check_ref_cells(&d);
    }

    /// A large cycle (compared through a table, not a list), in two
    /// rounds: a hub, twenty equal leaves naming it, and twenty equal
    /// nodes naming the hub and a leaf.
    #[test]
    fn a_large_cycle() {
        let mut d = DebugInfo::default();
        let hub = d.reserve().unwrap();
        let leaves = reserve(&mut d, 20);
        let pairs = reserve(&mut d, 20);
        let all: Vec<DiId> = leaves.iter().chain(&pairs).copied().collect();
        d.define(hub, &tuple(&all)).unwrap();
        for &leaf in &leaves {
            d.define(leaf, &tuple(&[hub])).unwrap();
        }
        for (&pair, &leaf) in pairs.iter().zip(leaves.iter().rev()) {
            d.define(pair, &tuple(&[hub, leaf])).unwrap();
        }
        let merge = d.unique_nodes(DiMark::default());
        assert_eq!(merge.merged(), 38);
        let want: Vec<DiId> = [leaves[0]; 20].into_iter().chain([pairs[0]; 20]).collect();
        assert_eq!(elements(&d, hub), want);
        assert_eq!(elements(&d, pairs[0]), [hub, leaves[0]]);
        check_ref_cells(&d);
    }

    /// `distinct` nodes are never merged, with each other or with an
    /// equal uniqued node, and never found by `add`.
    #[test]
    fn distinct_nodes_stay_apart() {
        let mut d = DebugInfo::default();
        let n = reserve(&mut d, 5);
        d.define_distinct(n[0], &int()).unwrap();
        d.define_distinct(n[1], &int()).unwrap();
        d.define(n[2], &int()).unwrap();
        d.define_distinct(n[3], &pointer(n[4])).unwrap();
        d.define(n[4], &int()).unwrap();
        let merge = d.unique_nodes(DiMark::default());
        assert_eq!(merge.merged(), 1);
        assert_eq!(merge.get(n[4]), n[2]);
        assert_eq!(base(&d, n[3]), Some(n[2]), "a distinct node's reference is rewritten");
        assert_eq!(d.add(&int()).unwrap(), n[2]);
        assert_ne!(d.add(&pointer(n[2])).unwrap(), n[3]);
        check_ref_cells(&d);
    }

    /// A later pass from a mark: the nodes before it are looked up, not
    /// merged, and the new ones merge into them.
    #[test]
    fn a_later_pass_finds_earlier_nodes() {
        let mut d = DebugInfo::default();
        let i1 = d.reserve().unwrap();
        d.define(i1, &int()).unwrap();
        let p1 = d.add(&pointer(i1)).unwrap();
        assert!(d.unique_nodes(DiMark::default()).is_empty());
        let mark = d.node_mark();
        let n = reserve(&mut d, 3);
        d.define(n[0], &pointer(n[1])).unwrap();
        d.define(n[1], &int()).unwrap();
        d.define(n[2], &tuple(&[n[0]])).unwrap();
        let merge = d.unique_nodes(mark);
        assert_eq!((merge.get(n[0]), merge.get(n[1]), merge.get(n[2])), (p1, i1, n[2]));
        assert_eq!(merge.get(p1), p1, "an id before the mark stands for itself");
        assert_eq!(elements(&d, n[2]), [p1]);
        assert!(!d.is_merged(p1) && !d.is_merged(i1));
    }

    /// `complete` refuses a merged forward declaration and completes its
    /// survivor; equal declarations with one identifier are one node,
    /// distinct definitions with it are not.
    #[test]
    fn complete_and_identifiers() {
        let declaration = DiCompositeType {
            tag: DW_TAG_STRUCTURE_TYPE,
            name: b"S",
            flags: DiFlags::FWD_DECL,
            identifier: b"_ZTS1S",
            ..Default::default()
        };
        let definition = DiCompositeType { flags: DiFlags::default(), size: 32, ..declaration };
        let mut d = DebugInfo::default();
        let n = reserve(&mut d, 4);
        d.define(n[0], &declaration.into()).unwrap();
        d.define(n[1], &declaration.into()).unwrap();
        d.define_distinct(n[2], &definition.into()).unwrap();
        d.define_distinct(n[3], &definition.into()).unwrap();
        let merge = d.unique_nodes(DiMark::default());
        assert_eq!(merge.merged(), 1);
        assert!(d.is_merged(n[1]) && !d.is_merged(n[2]) && !d.is_merged(n[3]));
        let refused = d.complete(n[1], &definition.into()).unwrap_err();
        assert_eq!(refused.kind, IrErrorKind::InvalidDebugInfo);
        d.complete(n[0], &definition.into()).unwrap();
    }

    /// A location's scope is rewritten, and the location is found again
    /// under the survivor.
    #[test]
    fn locations_name_the_survivor() {
        let block = DiLexicalBlock { scope: None, file: None, line: 4, column: 2 };
        let mut d = DebugInfo::default();
        let n = reserve(&mut d, 2);
        d.define(n[0], &block.into()).unwrap();
        d.define(n[1], &block.into()).unwrap();
        let at = DiLocation { line: 7, column: 1, scope: n[1], inlined_at: LocId::NONE };
        let loc = d.loc(at).unwrap();
        let merge = d.unique_nodes(DiMark::default());
        assert_eq!(merge.get(n[1]), n[0]);
        assert_eq!(d.location(loc).map(|l| l.scope), Some(n[0]));
        assert_eq!(d.loc(DiLocation { scope: n[0], ..at }).unwrap(), loc);
    }

    /// Every field shape that holds a node id: options, lists with null
    /// elements, and a bound that names a node.
    #[test]
    fn reference_cells_of_every_shape() {
        let mut d = DebugInfo::default();
        let n = reserve(&mut d, 6);
        d.define(n[0], &int()).unwrap();
        d.define(n[1], &int()).unwrap();
        let types = [DiId::NONE, n[0], n[1]];
        d.define(n[2], &DiSubroutineType { types: &types, ..Default::default() }.into()).unwrap();
        let subrange = |count| DiSubrange { count: DiBound::Node(count), ..Default::default() };
        d.define(n[3], &subrange(n[0]).into()).unwrap();
        d.define(n[4], &subrange(n[1]).into()).unwrap();
        d.define(n[5], &tuple(&[n[3], DiId::NONE, n[4]])).unwrap();
        check_ref_cells(&d);
        let merge = d.unique_nodes(DiMark::default());
        assert_eq!(merge.merged(), 2);
        assert_eq!(elements(&d, n[5]), [n[3], DiId::NONE, n[3]]);
        match d.node(n[2]) {
            DiNode::SubroutineType(t) => assert_eq!(t.types, [DiId::NONE, n[0], n[0]]),
            other => panic!("{other:?}"),
        }
        check_ref_cells(&d);
    }
}
