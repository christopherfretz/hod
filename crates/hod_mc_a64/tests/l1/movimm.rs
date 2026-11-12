//! The constant materializer, `spec/mc-a64.md` "Constant materialization":
//! the sequence computes the constant, its length is minimal, the choice
//! among minimal sequences is the specified one; and LLVM agrees about
//! which constants take one instruction.

use hod_mc_a64::*;

use crate::harness::Batch;
use crate::text::{Rng, bitmask_patterns, decode_bitmask_reference, g, rz};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum First {
    Movz,
    Movn,
    Orr,
}

/// Executes a materialization sequence: MOVZ, MOVN or ORR-from-ZR first,
/// then MOVKs, all on register `rd` at one operand size. Returns the kind
/// of the first instruction and the register's final value.
fn execute(words: &[u32], sz: Size, rd: u32) -> (First, u64) {
    let mask = u64::MAX >> (64 - sz.bits());
    let mut first = None;
    let mut value = 0u64;
    for &word in words {
        assert_eq!(word & 0x1f, rd, "{word:#010x} writes another register");
        assert_eq!(word >> 31, sz.bits() / 64, "{word:#010x} has the wrong operand size");
        if word & 0x1f80_0000 == 0x1280_0000 {
            // Move wide: opc at 30:29, hw at 22:21, imm16 at 20:5.
            let hw = (word >> 21) & 3;
            assert!(hw * 16 < sz.bits(), "{word:#010x} shifts out of the register");
            let imm = u64::from((word >> 5) & 0xffff) << (hw * 16);
            let kind = match (word >> 29) & 3 {
                0 => {
                    value = !imm & mask;
                    First::Movn
                }
                2 => {
                    value = imm;
                    First::Movz
                }
                3 => {
                    assert!(first.is_some(), "MOVK cannot come first");
                    value = value & !(0xffff << (hw * 16)) | imm;
                    continue;
                }
                _ => panic!("{word:#010x} is not a move wide instruction"),
            };
            assert!(first.replace(kind).is_none(), "only MOVK may follow the first instruction");
        } else if word & 0x7f80_0000 == 0x3200_0000 {
            // ORR (immediate): Rn at 9:5 must be the zero register.
            assert_eq!((word >> 5) & 0x1f, 31, "{word:#010x} does not read ZR");
            let (n, immr, imms) = ((word >> 22) & 1, (word >> 16) & 0x3f, (word >> 10) & 0x3f);
            value = decode_bitmask_reference(sz.bits(), n, immr, imms)
                .unwrap_or_else(|| panic!("{word:#010x} has a reserved immediate"));
            assert!(first.replace(First::Orr).is_none(), "ORR must come first");
        } else {
            panic!("{word:#010x} is not MOVZ, MOVN, MOVK or ORR (immediate)");
        }
    }
    (first.expect("a sequence is not empty"), value)
}

fn differing_chunks(a: u64, b: u64, chunks: u32) -> usize {
    let diff = a ^ b;
    let mut count = 0;
    for i in 0..chunks {
        count += usize::from((diff >> (i * 16)) & 0xffff != 0);
    }
    count
}

/// The shortest lengths of a MOVZ-first, a MOVN-first and an ORR-first
/// sequence for `value`, by definition: a wide move sets one chunk and
/// fixes the rest at the background; an ORR sets all chunks from some valid
/// logical immediate (every one is tried); each remaining wrong chunk
/// costs one MOVK.
fn optimal(sz: Size, value: u64, patterns: &[u64]) -> [usize; 3] {
    let chunks = sz.bits() / 16;
    let mask = u64::MAX >> (64 - sz.bits());
    let movz = differing_chunks(value, 0, chunks).max(1);
    let movn = differing_chunks(value, mask, chunks).max(1);
    let mut orr = usize::MAX;
    if movz.min(movn) > 1 {
        for &pattern in patterns {
            orr = orr.min(1 + differing_chunks(value, pattern, chunks));
        }
    }
    [movz, movn, orr]
}

fn edge_cases() -> Vec<u64> {
    let mut values = vec![0, 1, 2, 0xffff, 0x1_0000, 0xffff_ffff, 0x1_0000_0000, u64::MAX];
    for bit in 0..64 {
        values.extend([1 << bit, (1 << bit) - 1, !(1u64 << bit), u64::MAX << bit]);
    }
    for chunk in 0..4 {
        for imm in [1u64, 0x1234, 0x8000, 0xffff] {
            values.extend([imm << (chunk * 16), !(imm << (chunk * 16))]);
        }
    }
    // Floating-point bit patterns and other everyday constants.
    values.extend([
        0x3ff0_0000_0000_0000,
        0x3fb9_9999_9999_999a,
        0x4009_21fb_5444_2d18,
        0x7ff8_0000_0000_0000,
        0x3f80_0000,
        0x7fc0_0000,
        0x0123_4567_89ab_cdef,
        0x5555_5555_5555_5555,
        0x5555_5555_5555_1234,
        0x5555_1234_5555_abcd,
        0xaaaa_aaaa_0000_aaaa,
        0xffff_0000_ffff_0000,
        0x1234_ffff_ffff_5678,
        0x0000_1234_5678_0000,
        0x00ff_00ff_00ff_1234,
        0x8000_0000_8000_0000,
        0x7fff_ffff_ffff_ffff,
        0x8000_0000_0000_0000,
    ]);
    values
}

/// A deterministic sample mixing uniformly random values with the shapes
/// each strategy is good at: values built from few distinct chunks, and
/// logical immediates with zero, one or two chunks overwritten.
fn sample(count: usize, seed: u64) -> Vec<u64> {
    let mut rng = Rng::new(seed);
    let patterns = bitmask_patterns(64);
    let mut values = edge_cases();
    while values.len() < count {
        let value = match rng.below(4) {
            0 => rng.next(),
            1 => {
                let palette = [0, 0xffff, rng.below(0x1_0000), rng.below(0x1_0000)];
                (0..4).fold(0, |v, _| v << 16 | rng.pick(&palette))
            }
            2 => {
                let mut value = rng.pick(&patterns);
                for _ in 0..rng.below(3) {
                    let shift = rng.below(4) * 16;
                    let fill = [0, 0xffff, rng.below(0x1_0000)][rng.below(3) as usize];
                    value = value & !(0xffff << shift) | fill << shift;
                }
                value
            }
            _ => (rng.next() as i64 >> rng.below(64)) as u64,
        };
        values.push(value);
    }
    values
}

fn check(sz: Size, rd: u32, value: u64, patterns: &[u64]) {
    let constant = value & (u64::MAX >> (64 - sz.bits()));
    let seq = mov_imm(sz, g(rd), value);
    let words = seq.as_slice();
    assert_eq!(words.len(), seq.len());
    let (first, result) = execute(words, sz, rd);
    assert_eq!(result, constant, "{sz:?} {value:#x}: {words:x?} computes {result:#x}");
    let [movz, movn, orr] = optimal(sz, constant, patterns);
    let best = movz.min(movn).min(orr);
    assert_eq!(
        words.len(),
        best,
        "{sz:?} {value:#x}: {words:x?} is not minimal (MOVZ {movz}, MOVN {movn}, ORR {orr})"
    );
    let expected = if movz == best {
        First::Movz
    } else if movn == best {
        First::Movn
    } else {
        First::Orr
    };
    assert_eq!(first, expected, "{sz:?} {value:#x}: {words:x?} breaks the tie the wrong way");
}

#[test]
fn sequences_compute_the_constant_and_are_minimal() {
    let patterns = bitmask_patterns(64);
    for (i, value) in sample(24_000, 0xa64).into_iter().enumerate() {
        check(Size::X, (i % 31) as u32, value, &patterns);
    }
    let patterns = bitmask_patterns(32);
    for (i, value) in sample(24_000, 0xa32).into_iter().enumerate() {
        // The upper 32 bits are ignored, whatever they are.
        check(Size::W, (i % 31) as u32, value, &patterns);
        check(Size::W, (i % 31) as u32, value & 0xffff_ffff, &patterns);
    }
}

/// Every 16-bit value in every chunk position, alone and against a
/// background of ones: the single-instruction wide moves.
#[test]
fn single_chunk_constants_take_one_instruction() {
    for imm in 0..=0xffffu64 {
        for chunk in 0..4 {
            let value = imm << (chunk * 16);
            assert_eq!(mov_imm(Size::X, Reg::X0, value).len(), 1, "{value:#x}");
            assert_eq!(mov_imm(Size::X, Reg::X0, !value).len(), 1, "{:#x}", !value);
            if chunk < 2 {
                assert_eq!(mov_imm(Size::W, Reg::X0, value).len(), 1, "{value:#x}");
                assert_eq!(mov_imm(Size::W, Reg::X0, !value).len(), 1, "{:#x}", !value);
            }
        }
    }
}

/// LLVM's `mov Rd, #imm` accepts exactly the constants that take one
/// instruction, and picks the same instruction.
pub fn build(b: &mut Batch) {
    for (sz, seed) in [(Size::X, 0x11), (Size::W, 0x12)] {
        let mask = u64::MAX >> (64 - sz.bits());
        for (i, value) in sample(3000, seed).into_iter().enumerate() {
            let value = value & mask;
            let rd = (i % 31) as u32;
            let seq = mov_imm(sz, g(rd), value);
            let asm = format!("mov {}, #{value:#x}", rz(sz, rd));
            if let [word] = seq.as_slice() {
                b.push("mov_imm", *word, None, asm);
            } else {
                b.reject("mov_imm", None, &format!("({sz:?}, {value:#x})"), Some(asm));
            }
        }
    }
}
