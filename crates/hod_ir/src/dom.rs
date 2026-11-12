//! Dominance over a body's blocks (`spec/ir.md` §5.8), for `Body::verify`.
//!
//! Lengauer and Tarjan's algorithm with path compression (the "simple"
//! version, O(E log B)), without recursion, then a numbering of the
//! dominator tree that answers "does `a` dominate `b`" with two compares.
//! Built only when a body has a use that needs it.

use crate::body::Body;
use crate::ids::BlockId;

const NONE: u32 = u32::MAX;

/// The control-flow graph and dominator tree of one body.
pub(crate) struct Dominators {
    /// Predecessor edges of each block, repeats included: block `b`'s are
    /// `preds[pred_start[b]..pred_start[b + 1]]`.
    pred_start: Vec<u32>,
    preds: Vec<u32>,
    /// Depth-first number of each block from the entry; `NONE` for a
    /// block control does not reach.
    number: Vec<u32>,
    /// By depth-first number: the interval of the node in a preorder walk
    /// of the dominator tree. `a` dominates `b` exactly when `a`'s
    /// interval contains `b`'s.
    enter: Vec<u32>,
    leave: Vec<u32>,
}

impl Dominators {
    pub(crate) fn new(body: &Body) -> Dominators {
        let count = body.blocks.len();
        let (succ_start, succs, pred_start, preds) = edges(body);

        // Depth-first numbering from the entry.
        let mut number = vec![NONE; count];
        let mut vertex = Vec::with_capacity(count);
        let mut parent = Vec::with_capacity(count);
        let entry = body.entry().index();
        let mut stack: Vec<(u32, u32)> = vec![(entry as u32, succ_start[entry])];
        number[entry] = 0;
        vertex.push(entry as u32);
        parent.push(NONE);
        while let Some(top) = stack.last_mut() {
            let (block, next) = *top;
            if next == succ_start[block as usize + 1] {
                stack.pop();
                continue;
            }
            top.1 += 1;
            let to = succs[next as usize];
            if number[to as usize] == NONE {
                number[to as usize] = vertex.len() as u32;
                parent.push(number[block as usize]);
                vertex.push(to);
                stack.push((to, succ_start[to as usize]));
            }
        }
        drop(succs);

        // Semidominators and immediate dominators, by number.
        let reached = vertex.len();
        let mut semi: Vec<u32> = (0..reached as u32).collect();
        let mut label = semi.clone();
        let mut ancestor = vec![NONE; reached];
        let mut idom = vec![0u32; reached];
        let mut bucket_head = vec![NONE; reached];
        let mut bucket_next = vec![NONE; reached];
        let mut chain = Vec::new();
        for w in (1..reached).rev() {
            let block = vertex[w] as usize;
            for &pred in &preds[pred_start[block] as usize..pred_start[block + 1] as usize] {
                let v = number[pred as usize];
                if v == NONE {
                    continue;
                }
                let u = eval(v, &mut ancestor, &mut label, &semi, &mut chain);
                if semi[u as usize] < semi[w] {
                    semi[w] = semi[u as usize];
                }
            }
            let s = semi[w] as usize;
            bucket_next[w] = bucket_head[s];
            bucket_head[s] = w as u32;
            let p = parent[w];
            ancestor[w] = p;
            let mut v = std::mem::replace(&mut bucket_head[p as usize], NONE);
            while v != NONE {
                let u = eval(v, &mut ancestor, &mut label, &semi, &mut chain);
                idom[v as usize] = if semi[u as usize] < semi[v as usize] { u } else { p };
                v = bucket_next[v as usize];
            }
        }
        for w in 1..reached {
            if idom[w] != semi[w] {
                idom[w] = idom[idom[w] as usize];
            }
        }
        drop((semi, label, ancestor, bucket_head, bucket_next));

        // Preorder intervals of the dominator tree. A child's number is
        // above its parent's, so children are listed by counting.
        let mut child_start = vec![0u32; reached + 1];
        for &dominator in &idom[1..] {
            child_start[dominator as usize + 1] += 1;
        }
        for index in 0..reached {
            child_start[index + 1] += child_start[index];
        }
        let mut children = vec![0u32; reached.saturating_sub(1)];
        let mut fill = child_start.clone();
        for (w, &dominator) in idom.iter().enumerate().skip(1) {
            children[fill[dominator as usize] as usize] = w as u32;
            fill[dominator as usize] += 1;
        }
        let (mut enter, mut leave) = (vec![0u32; reached], vec![0u32; reached]);
        let mut clock = 0u32;
        let mut walk: Vec<(u32, u32)> = vec![(0, child_start[0])];
        enter[0] = 0;
        while let Some(top) = walk.last_mut() {
            let (node, next) = *top;
            if next == child_start[node as usize + 1] {
                leave[node as usize] = clock;
                walk.pop();
                continue;
            }
            top.1 += 1;
            let child = children[next as usize];
            clock += 1;
            enter[child as usize] = clock;
            walk.push((child, child_start[child as usize]));
        }
        Dominators { pred_start, preds, number, enter, leave }
    }

    /// Whether control reaches `block` from the entry.
    #[inline]
    pub(crate) fn reachable(&self, block: BlockId) -> bool {
        self.number[block.index()] != NONE
    }

    /// Whether `a` dominates `b` (every block dominates itself). As in
    /// LLVM, a block control does not reach is dominated by every block
    /// and dominates none but itself.
    #[inline]
    pub(crate) fn dominates(&self, a: BlockId, b: BlockId) -> bool {
        if a == b {
            return true;
        }
        let (a, b) = (self.number[a.index()], self.number[b.index()]);
        if b == NONE {
            return true;
        }
        if a == NONE {
            return false;
        }
        let (a, b) = (a as usize, b as usize);
        self.enter[a] <= self.enter[b] && self.leave[b] <= self.leave[a]
    }

    /// The predecessor edges of `block`, one entry per edge.
    #[inline]
    pub(crate) fn preds(&self, block: BlockId) -> &[u32] {
        let index = block.index();
        &self.preds[self.pred_start[index] as usize..self.pred_start[index + 1] as usize]
    }
}

/// The successor and predecessor edges of `body`'s blocks, repeats
/// included, as offsets into flat arrays: `(succ_start, succs,
/// pred_start, preds)`.
fn edges(body: &Body) -> (Vec<u32>, Vec<u32>, Vec<u32>, Vec<u32>) {
    let count = body.blocks.len();
    let mut succ_start = vec![0u32; count + 1];
    let mut pred_start = vec![0u32; count + 1];
    for block in body.blocks() {
        for to in block.successors() {
            succ_start[block.id().index() + 1] += 1;
            pred_start[to.index() + 1] += 1;
        }
    }
    for index in 0..count {
        succ_start[index + 1] += succ_start[index];
        pred_start[index + 1] += pred_start[index];
    }
    let edges = succ_start[count] as usize;
    let (mut succs, mut preds) = (vec![0u32; edges], vec![0u32; edges]);
    let mut succ_fill = succ_start.clone();
    let mut pred_fill = pred_start.clone();
    for block in body.blocks() {
        let from = block.id().index();
        for to in block.successors() {
            succs[succ_fill[from] as usize] = to.raw();
            succ_fill[from] += 1;
            preds[pred_fill[to.index()] as usize] = from as u32;
            pred_fill[to.index()] += 1;
        }
    }
    (succ_start, succs, pred_start, preds)
}

/// The node of least semidominator on the compressed path from `v` to the
/// root of its tree in the forest, compressing the path on the way.
fn eval(
    v: u32,
    ancestor: &mut [u32],
    label: &mut [u32],
    semi: &[u32],
    chain: &mut Vec<u32>,
) -> u32 {
    if ancestor[v as usize] == NONE {
        return v;
    }
    // The nodes whose ancestor is not a root, nearest first.
    chain.clear();
    let mut x = v;
    while ancestor[ancestor[x as usize] as usize] != NONE {
        chain.push(x);
        x = ancestor[x as usize];
    }
    // From the one nearest the root down, as the recursive version does
    // on its way back.
    while let Some(x) = chain.pop() {
        let a = ancestor[x as usize] as usize;
        if semi[label[a] as usize] < semi[label[x as usize] as usize] {
            label[x as usize] = label[a];
        }
        ancestor[x as usize] = ancestor[a];
    }
    label[v as usize]
}

#[cfg(test)]
mod tests {
    use super::Dominators;
    use crate::{BlockId, FunctionBuilder, Module, SymId, TargetSpec, TypeId};

    /// `spec/ir.md` §5.8: on random graphs, `dominates` is the
    /// definition: `a` dominates a reachable `b` when every path from the
    /// entry to `b` passes `a`, so `b` is not reached with `a` removed.
    #[test]
    fn dominance_is_the_definition_on_random_graphs() {
        let mut state = 0x2545_f491_4f6c_dd1d_u64;
        let mut next = |n: usize| {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            (state % n as u64) as usize
        };
        let mut m = Module::new(TargetSpec::x86_64_linux_gnu());
        let fn_ty = m.types.function(TypeId::VOID, &[TypeId::I1], false).unwrap();
        for _ in 0..300 {
            let count = 1 + next(12);
            let mut b = FunctionBuilder::new(&m, SymId::new(0), fn_ty).unwrap();
            let blocks: Vec<BlockId> = (0..count).map(|_| b.create_block().unwrap()).collect();
            let cond = b.arg(0).unwrap();
            let mut succs = vec![Vec::new(); count];
            for (index, &block) in blocks.iter().enumerate() {
                b.begin_block(block).unwrap();
                let (x, y) = (next(count), next(count));
                match next(4) {
                    0 => {
                        b.ret(None).unwrap();
                    }
                    1 => {
                        b.br(blocks[x]).unwrap();
                        succs[index] = vec![x];
                    }
                    _ => {
                        b.cond_br(cond, blocks[x], blocks[y]).unwrap();
                        succs[index] = vec![x, y];
                    }
                }
            }
            let body = b.finish().unwrap();
            let dom = Dominators::new(&body);
            // The blocks reached from the entry without passing `without`.
            let reached = |without: usize| {
                let mut seen = vec![false; count];
                let mut stack = if without == 0 { vec![] } else { vec![0] };
                while let Some(block) = stack.pop() {
                    if seen[block] || block == without {
                        continue;
                    }
                    seen[block] = true;
                    stack.extend(&succs[block]);
                }
                seen
            };
            let all = reached(usize::MAX);
            for a in 0..count {
                let without = reached(a);
                assert_eq!(dom.reachable(blocks[a]), all[a]);
                for bb in 0..count {
                    let want = a == bb || !all[bb] || (all[a] && !without[bb]);
                    assert_eq!(dom.dominates(blocks[a], blocks[bb]), want, "{succs:?} {a} {bb}");
                }
            }
        }
    }
}
