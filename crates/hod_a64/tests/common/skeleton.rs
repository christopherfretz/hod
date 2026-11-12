//! Programs of what the skeleton lowers by itself, for the differential
//! harness until the integer group lands (`common::groups`): an entry
//! function `i64 entry(i64, i64)` whose blocks are a chain of `br`s in a
//! shuffled layout order, with blocks without predecessors that end in
//! `ret` or `unreachable`, and a `ret` of a constant or an argument.
//!
//! A [`Mutation`] makes the program compute something else, so that the
//! harness can be shown to notice.

use hod_interp::Val;
use hod_interp::generate::{Coverage, Generated};
use hod_ir::{BlockId, FunctionBuilder, FunctionDesc, Module, TargetSpec, TypeId, Value};

/// A deliberate miscompilation of a program.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Mutation {
    /// The returned value differs in its lowest bit.
    WrongConstant,
    /// The entry block branches to the decoy block, which returns a
    /// different value.
    WrongTarget,
}

/// `SplitMix64`.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9e37_79b9_7f4a_7c15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
        z ^ (z >> 31)
    }

    fn below(&mut self, n: u64) -> u64 {
        self.next() % n
    }

    /// A 64-bit value of one of the shapes `mov_imm` handles differently:
    /// small, `movn`, bitmask, one to four `movk`s.
    fn constant(&mut self) -> u64 {
        match self.below(8) {
            0 => self.below(0x1_0000),
            1 => !self.below(0x1_0000),
            2 => 0x00ff_00ff_00ff_00ff,
            3 => self.below(0x1_0000) << (16 * self.below(4)),
            4 => 0,
            5 => u64::MAX,
            _ => self.next(),
        }
    }
}

/// The program of `seed`, mutated by `mutation`.
pub fn generate(seed: u64, mutation: Option<Mutation>) -> Generated {
    let mut rng = Rng(seed ^ 0x5eed_a64a_64a6_4a64);
    let mut m = Module::new(TargetSpec::aarch64_linux_gnu());
    let ty = m.types.function(TypeId::I64, &[TypeId::I64, TypeId::I64], false).unwrap();
    let desc = FunctionDesc { has_body: true, ..FunctionDesc::new(ty) };
    let entry = m.add_function(b"entry", &desc).unwrap();
    let args = [rng.next(), rng.next()];
    // What the chain returns: a constant or an argument.
    let from_arg = rng.below(3) == 0;
    let arg_index = rng.below(2) as u32;
    let mut value = if from_arg { args[arg_index as usize] } else { rng.constant() };
    let chain = 1 + rng.below(6) as usize;
    let decoys = 1 + rng.below(3) as usize;
    let blocks = chain + decoys;
    // The chain visits block 0 (the entry), then the others in a
    // shuffled order; the first decoy returns something else.
    let mut order: Vec<usize> = (1..blocks).collect();
    for i in (1..order.len()).rev() {
        order.swap(i, rng.below(i as u64 + 1) as usize);
    }
    let mut path = vec![0];
    path.extend(order.iter().copied().filter(|&i| i < chain));
    let decoy = order.iter().copied().find(|&i| i >= chain).expect("one decoy at least");
    let int = |m: &mut Module, v: u64| {
        Value::constant(m.consts.int(&m.types, TypeId::I64, v).unwrap()).unwrap()
    };
    // What each block ends with, the constants interned before the
    // builder borrows the module.
    enum End {
        Br(usize),
        Ret(Value),
        Unreachable,
    }
    let decoy_value = int(&mut m, value ^ 0x100);
    if mutation == Some(Mutation::WrongConstant) {
        value ^= 1;
    }
    let mut ends = Vec::with_capacity(blocks);
    for index in 0..blocks {
        let end = if let Some(at) = path.iter().position(|&p| p == index) {
            if at + 1 < path.len() {
                let wrong = at == 0 && mutation == Some(Mutation::WrongTarget);
                End::Br(if wrong { decoy } else { path[at + 1] })
            } else if from_arg && mutation != Some(Mutation::WrongConstant) {
                End::Ret(Value::arg(arg_index).unwrap())
            } else {
                End::Ret(int(&mut m, value))
            }
        } else if index == decoy {
            End::Ret(decoy_value)
        } else if rng.below(2) == 0 {
            End::Unreachable
        } else {
            let other = rng.constant();
            End::Ret(int(&mut m, other))
        };
        ends.push(end);
    }
    let mut b = FunctionBuilder::new(&m, entry, ty).unwrap();
    let ids: Vec<BlockId> = (0..blocks).map(|_| b.create_block().unwrap()).collect();
    // Blocks are filled in layout order (creation order), so that a `br`
    // goes forward or backward.
    for (&block, end) in ids.iter().zip(&ends) {
        b.begin_block(block).unwrap();
        match *end {
            End::Br(to) => b.br(ids[to]).unwrap(),
            End::Ret(v) => b.ret(Some(v)).unwrap(),
            End::Unreachable => b.unreachable().unwrap(),
        };
    }
    let body = b.finish().unwrap();
    body.verify(&m).unwrap();
    Generated {
        module: m,
        bodies: vec![body],
        entry,
        args: args.iter().map(|&a| Val::i64(a)).collect(),
        fuel_bound: 1000,
        hosts: None,
        coverage: Coverage::default(),
    }
}
