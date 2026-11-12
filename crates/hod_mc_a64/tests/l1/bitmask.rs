//! Logical immediates, `spec/mc-a64.md` "Immediates": the exhaustive
//! tool-free tests, and the sweep of every valid pattern against LLVM.

use std::collections::BTreeSet;

use hod_mc_a64::*;

use crate::harness::Batch;
use crate::text::{Rng, bitmask_patterns, decode_bitmask_reference, rs, rz, s, z};

fn split(encoding: u16) -> (u32, u32, u32) {
    let e = u32::from(encoding);
    (e >> 12, (e >> 6) & 0x3f, e & 0x3f)
}

/// Every valid pattern encodes, the encoding decodes to the pattern (by
/// the crate's decoder and by the reference one), and the encoding is the
/// canonical one.
#[test]
fn every_valid_pattern_round_trips() {
    for (sz, count) in [(Size::X, 5334), (Size::W, 1302)] {
        let patterns = bitmask_patterns(sz.bits());
        assert_eq!(patterns.len(), count, "the number of {sz:?} logical immediates");
        let mut encodings = BTreeSet::new();
        for &value in &patterns {
            let encoding = encode_bitmask(sz, value)
                .unwrap_or_else(|| panic!("{sz:?} {value:#018x} is valid but was rejected"));
            assert_eq!(decode_bitmask(sz, encoding), Some(value), "{sz:?} {value:#018x}");
            let (n, immr, imms) = split(encoding);
            assert_eq!(
                decode_bitmask_reference(sz.bits(), n, immr, imms),
                Some(value),
                "{sz:?} {value:#018x} encoded as N={n} immr={immr} imms={imms}"
            );
            // Canonical: the rotation is below the element size.
            let esize = 64 - u64::from((n << 6) | (!imms & 0x3f)).leading_zeros() - 1;
            assert!(immr < (1 << esize), "{sz:?} {value:#018x}: immr {immr} is not canonical");
            assert!(encodings.insert(encoding), "two patterns share an encoding");
        }
    }
}

/// The crate's decoder agrees with the reference decoder on all 8192
/// encodings (and anything wider is rejected), and the encoder returns the
/// canonical encoding of whatever a valid encoding decodes to.
#[test]
fn every_encoding_decodes_as_the_architecture_says() {
    for sz in Size::ALL {
        let valid: BTreeSet<u64> = bitmask_patterns(sz.bits()).into_iter().collect();
        let mut decodable = 0;
        for encoding in 0..0x2000u16 {
            let (n, immr, imms) = split(encoding);
            let expected = decode_bitmask_reference(sz.bits(), n, immr, imms);
            assert_eq!(decode_bitmask(sz, encoding), expected, "{sz:?} encoding {encoding:#06x}");
            if let Some(value) = expected {
                decodable += 1;
                assert!(valid.contains(&value));
                let canonical = encode_bitmask(sz, value).expect("a decoded value encodes");
                assert_eq!(decode_bitmask(sz, canonical), Some(value));
            }
        }
        assert!(decodable >= valid.len());
        for encoding in [0x2000, 0x4000, 0xffff] {
            assert_eq!(decode_bitmask(sz, encoding), None);
        }
    }
}

/// Values that are not logical immediates are rejected: the fixed corner
/// cases, every 16-bit value replicated, every value one bit away from a
/// valid pattern, and a pseudo-random sample.
#[test]
fn invalid_values_are_rejected() {
    for sz in Size::ALL {
        let valid: BTreeSet<u64> = bitmask_patterns(sz.bits()).into_iter().collect();
        let mask = u64::MAX >> (64 - sz.bits());
        let check = |value: u64| {
            assert_eq!(
                encode_bitmask(sz, value).is_some(),
                valid.contains(&value),
                "{sz:?} {value:#018x}"
            );
        };
        check(0);
        check(mask);
        check(u64::MAX);
        let mut rejected = 0;
        for chunk in 0..=0xffffu64 {
            let value = (chunk * 0x0001_0001_0001_0001) & mask;
            rejected += u32::from(!valid.contains(&value));
            check(value);
        }
        assert!(rejected > 65_000, "the replicated sweep is mostly invalid values");
        for &pattern in &valid {
            for bit in 0..sz.bits() {
                check(pattern ^ (1 << bit));
            }
        }
        let mut rng = Rng::new(0x1065);
        for _ in 0..200_000 {
            check(rng.next() & mask);
            // Sparse and dense values are likelier to be near-misses.
            check(rng.next() & rng.next() & mask);
            check((rng.next() | rng.next()) & mask);
        }
    }
    // A 32-bit operand does not accept a value with bits above bit 31.
    for value in [1 << 32, 0xffff_ffff_0000_00ff, 0x5555_5555_5555_5555, u64::MAX] {
        assert_eq!(encode_bitmask(Size::W, value), None);
    }
}

/// Every valid pattern through ORR, and a third of them through each of
/// AND, EOR, ANDS, against LLVM.
pub fn build(b: &mut Batch) {
    for sz in Size::ALL {
        for (i, imm) in bitmask_patterns(sz.bits()).into_iter().enumerate() {
            let (d, n) = ((i % 32) as u32, (i / 32 % 32) as u32);
            emit!(
                b,
                try_orr_imm(sz, s(d), z(n), imm),
                "orr {}, {}, #{imm:#x}",
                rs(sz, d),
                rz(sz, n)
            );
            match i % 3 {
                0 => emit!(
                    b,
                    try_and_imm(sz, s(d), z(n), imm),
                    "and {}, {}, #{imm:#x}",
                    rs(sz, d),
                    rz(sz, n)
                ),
                1 => emit!(
                    b,
                    try_eor_imm(sz, s(d), z(n), imm),
                    "eor {}, {}, #{imm:#x}",
                    rs(sz, d),
                    rz(sz, n)
                ),
                _ => emit!(
                    b,
                    try_ands_imm(sz, z(d), z(n), imm),
                    "ands {}, {}, #{imm:#x}",
                    rz(sz, d),
                    rz(sz, n)
                ),
            }
        }
    }
}
