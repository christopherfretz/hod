//! A measurement, not a gate (`spec/abi-x64.md` §11): the time of one
//! `assign` call. The numbers are printed (`cargo test --release -p
//! hod_x64 --test abi perf -- --nocapture`); what the test asserts is only
//! that the calls succeed.

use std::hint::black_box;
use std::time::Instant;

use hod_cg::abi::Assignment;
use hod_x64::abi::{Features, assign};

use crate::sigs::{Attr, Case, byval, p, pa, table};
use crate::ty::{I8, I32, I64, I128, Ty, arr, st, vec};

const CALLS: u32 = 200_000;

/// `spec/abi-x64.md` §11: nanoseconds per `assign` call for 0, 2, 6 and
/// 12 scalar arguments and for a signature of aggregates.
#[test]
fn perf_of_assign() {
    let (d, f) = (Ty::Double, Ty::Float);
    let cases = [
        ("0 arguments", Case::of(&[])),
        ("2 scalars", Case::of(&[Ty::Ptr, I32]).ret(I32)),
        ("6 scalars", Case::of(&[Ty::Ptr, I64, I32, d.clone(), Ty::Ptr, I8]).ret(Ty::Ptr)),
        (
            "12 scalars",
            Case::of(&[
                Ty::Ptr,
                I64,
                I32,
                d.clone(),
                Ty::Ptr,
                I8,
                f.clone(),
                I64,
                I64,
                I128,
                Ty::Ptr,
                d.clone(),
            ])
            .ret(I64),
        ),
        (
            "aggregates",
            Case::new(vec![
                pa(Ty::Ptr, Attr::Sret),
                p(arr(I64, 2)),
                pa(arr(f.clone(), 3), Attr::StackAlign(8)),
                p(st(&[I64, d.clone()])),
                p(vec(I32, 4)),
                byval(arr(I64, 6), Some(8), None),
                p(arr(d.clone(), 4)),
                p(st(&[I32, I32, st(&[f, I8])])),
            ]),
        ),
        ("aggregate return", Case::of(&[Ty::Ptr]).ret(st(&[I64, d.clone(), arr(d, 1)]))),
    ];
    let mut types = table();
    let mut out = Assignment::default();
    for (name, case) in &cases {
        let lowered = case.lower(&mut types);
        let sig = lowered.sig();
        assign(&types, &sig, Features::default(), &mut out).expect("supported");
        let mut parts = 0;
        let start = Instant::now();
        for _ in 0..CALLS {
            assign(black_box(&types), black_box(&sig), Features::default(), &mut out)
                .expect("supported");
            parts += black_box(&out).args.len();
        }
        let nanos = start.elapsed().as_secs_f64() * 1e9 / f64::from(CALLS);
        eprintln!(
            "assign, {name:<16} {nanos:7.1} ns/call  ({} argument parts)  `{}`",
            parts / CALLS as usize,
            case.text()
        );
    }
}
