//! The x86-64 vector group (WP-132, `spec/codegen.md` §5.6.13): random
//! vector functions (the interpreter's vector oracle generator,
//! `common/vector_gen.rs`) compiled by hod plain and stressed and by
//! `llc -O0 -fast-isel=0` of LLVM 22 and 19, run under the emulator and
//! compared with `hod_interp`.

#[path = "../../hod_obj/tests/common/linking.rs"]
#[allow(dead_code, reason = "a shared helper: not every function is used here")]
mod linking;

#[path = "common/aggr_gen.rs"]
mod aggr_gen;

#[path = "common/vector_gen.rs"]
mod vector_gen;

use std::collections::BTreeMap;

use vector_gen::{Tally, assert_agrees, check_batch, function};

/// Generated vector functions agree with the interpreter on hod's legs.
#[test]
fn generated_vectors_agree() {
    let count: u64 =
        std::env::var("HOD_VECTOR_COUNT").ok().and_then(|n| n.parse().ok()).unwrap_or(600);
    let per_file = 200;
    let mut tally = Tally::default();
    let mut start = 0;
    while start < count {
        let end = (start + per_file).min(count);
        check_batch(start..end, &mut tally);
        start = end;
    }
    eprintln!(
        "x64 vectors: {} generated, {} compared on {:?}, {} undefined, {} poison bytes; \
         refused {:?}; llc disagreements {:?}, batches it failed {:?}; text hod/llc22 {}/{} bytes; {} insts in {:?}",
        tally.generated,
        tally.run,
        tally.legs,
        tally.trapped,
        tally.poison_bytes,
        tally.refused,
        tally.llc_wrong,
        tally.llc_crashed,
        tally.text.0,
        tally.text.1,
        tally.insts,
        tally.compile
    );
    assert_agrees(&tally);
    assert!(tally.run * 5 >= tally.generated * 4, "too few functions compared: {tally:?}");
}

/// The generated text is a function of the seed alone.
#[test]
fn functions_are_deterministic() {
    let (mut a, mut b) = (BTreeMap::new(), BTreeMap::new());
    for seed in 0..50 {
        assert_eq!(function(seed, &mut a), function(seed, &mut b));
    }
    assert_eq!(a, b);
}
