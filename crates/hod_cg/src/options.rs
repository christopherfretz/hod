//! Options of code generation: `spec/codegen.md` §8.1, §12.

/// What a function compile is asked for besides the module and the body.
#[derive(Clone, Copy, PartialEq, Eq, Default, Debug)]
pub struct Options {
    /// The stress options of §12. All off in a product build.
    pub stress: Stress,
}

/// The stress options of §12, for tests only. A program must behave the
/// same under all of them.
#[derive(Clone, Copy, PartialEq, Eq, Default, Debug)]
pub struct Stress {
    /// Restrict the pool to its minimum (§2.1): the four lowest general
    /// and the two lowest vector registers of the target's pool.
    pub min_pool: bool,
    /// Compile every function in far-branch mode from the start (§6.3).
    pub far: bool,
    /// Displace every register value to a spill slot after each
    /// instruction (§4.3).
    pub displace: bool,
}

impl Stress {
    /// Every stress option at once.
    pub const ALL: Stress = Stress { min_pool: true, far: true, displace: true };
}
