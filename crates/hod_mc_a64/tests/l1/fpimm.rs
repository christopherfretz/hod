//! FMOV immediates and the floating-point constant materializer,
//! `spec/mc-a64.md` "Immediates" and "Floating-point constant
//! materialization": exactly the architecture's 256 values encode; a
//! materialized sequence computes the constant, has the specified shape and
//! is as short as the specification says; and LLVM agrees about which
//! constants are FMOV immediates.

use std::collections::BTreeSet;

use hod_mc_a64::*;

use crate::harness::Batch;
use crate::movimm;
use crate::text::{FP_FORMATS, FP_SIZES, Rng, fp_bits, fp_imm8_value, g, v};

/// The 256 encodable bit patterns at a precision, from the definition.
fn encodable(sz: FpSize) -> BTreeSet<u64> {
    (0..=255u8).map(|imm8| fp_bits(sz, fp_imm8_value(imm8))).collect()
}

#[test]
fn the_fmov_immediates_are_the_architectures() {
    for sz in FpSize::ALL {
        for imm8 in 0..=255u8 {
            let bits = fp_bits(sz, fp_imm8_value(imm8));
            assert_eq!(decode_fp_imm8(sz, imm8), bits, "{sz:?} {imm8:#04x}");
            assert_eq!(encode_fp_imm8(sz, bits), Some(imm8), "{sz:?} {bits:#x}");
        }
        assert_eq!(encodable(sz).len(), 256, "the 256 values are distinct");
    }
    // The headline values.
    assert_eq!(encode_fp_imm8(FpSize::D, 1.0f64.to_bits()), Some(0x70));
    assert_eq!(encode_fp_imm8(FpSize::S, u64::from(1.0f32.to_bits())), Some(0x70));
    assert_eq!(encode_fp_imm8(FpSize::D, 2.0f64.to_bits()), Some(0x00));
    assert_eq!(encode_fp_imm8(FpSize::D, (-31.0f64).to_bits()), Some(0xbf));
    assert_eq!(encode_fp_imm8(FpSize::S, u64::from(0.125f32.to_bits())), Some(0x40));
}

/// Nothing else encodes. Every pattern that agrees with an encodable one
/// in the bits the field does not determine freely is tried: every sign,
/// exponent and top four fraction bits with the remaining fraction zero
/// (exactly 256 of those encode), and every encodable value with one of
/// the remaining fraction bits set; then a random sample.
#[test]
fn only_the_fmov_immediates_encode() {
    for (sz, low) in [(FpSize::S, 19), (FpSize::D, 48)] {
        let set = encodable(sz);
        let mut count = 0;
        for top in 0..1u64 << (sz.bits() - low) {
            let bits = top << low;
            let field = encode_fp_imm8(sz, bits);
            assert_eq!(field.is_some(), set.contains(&bits), "{sz:?} {bits:#x}");
            count += usize::from(field.is_some());
        }
        assert_eq!(count, 256);
        for &bits in &set {
            for bit in 0..low {
                assert_eq!(encode_fp_imm8(sz, bits | 1 << bit), None, "{sz:?} {bits:#x} bit {bit}");
            }
        }
        let mut rng = Rng::new(0xf8 + u64::from(low));
        for _ in 0..100_000 {
            let bits = rng.next() >> (64 - sz.bits());
            assert_eq!(encode_fp_imm8(sz, bits).is_some(), set.contains(&bits), "{sz:?} {bits:#x}");
        }
    }
    // A single-precision pattern has nothing above bit 31.
    for &bits in &encodable(FpSize::S) {
        for bit in 32..64 {
            assert_eq!(encode_fp_imm8(FpSize::S, bits | 1 << bit), None, "{bits:#x} bit {bit}");
        }
    }
    for bits in [0, 1 << 31, 1 << 63, u64::MAX, 0x7ff0_0000_0000_0000, 0x7f80_0000] {
        assert_eq!(encode_fp_imm8(FpSize::S, bits), None, "{bits:#x}");
        assert_eq!(encode_fp_imm8(FpSize::D, bits), None, "{bits:#x}");
    }
}

/// The constructor asserts what the predicate says
/// (`spec/mc-a64.md` "Checked and unchecked forms").
#[test]
#[cfg(debug_assertions)]
#[should_panic(expected = "fmov_imm: operand precondition violated")]
fn fmov_imm_asserts_its_precondition() {
    let _ = fmov_imm(FpSize::D, VReg::V0, std::hint::black_box(0.1f64.to_bits()));
}

/// The integer register size the materializer goes through for a format.
fn gpr_size(fmt: FpFormat) -> Size {
    if fmt == FpFormat::D { Size::X } else { Size::W }
}

/// The scalar FMOV precision of a format, where it has one without
/// `FEAT_FP16`.
fn fp_size(fmt: FpFormat) -> Option<FpSize> {
    match fmt {
        FpFormat::H => None,
        FpFormat::S => Some(FpSize::S),
        FpFormat::D => Some(FpSize::D),
    }
}

/// What a materialization sequence does, by a model that shares nothing
/// with the encoder: the value it leaves in the low 64 bits of `Vrd` (the
/// upper 64 are zero after each of the three shapes), and the value it
/// leaves in the scratch register, if it writes it.
fn execute(words: &[u32], rd: u32, scratch: u32) -> (u64, Option<u64>) {
    let (&last, prefix) = words.split_last().expect("a sequence is not empty");
    assert_eq!(last & 0x1f, rd, "{last:#010x} writes another register");
    if prefix.is_empty() {
        // `movi d<rd>, #0`, as LLVM assembles it.
        if last == 0x2f00_e400 | rd {
            return (0, None);
        }
        // FMOV (scalar, immediate): 0001 1110 ft1 imm8 100 00000 Rd.
        assert_eq!(last & 0xffa0_1fe0, 0x1e20_1000, "{last:#010x} is not FMOV (immediate)");
        let sz = FpSize::ALL[(last >> 22 & 1) as usize];
        let imm8 = (last >> 13) as u8;
        return (fp_bits(sz, fp_imm8_value(imm8)), None);
    }
    // A general-purpose materialization, then FMOV from that register:
    // `fmov s<rd>, w<scratch>` or `fmov d<rd>, x<scratch>`.
    let sz = Size::ALL[(last >> 31) as usize];
    let fmov = match sz {
        Size::W => 0x1e27_0000,
        Size::X => 0x9e67_0000,
    };
    assert_eq!(last, fmov | scratch << 5 | rd, "{last:#010x} is not FMOV from the scratch");
    let (_, value) = movimm::execute(prefix, sz, scratch);
    (value, Some(value))
}

fn check(fmt: FpFormat, rd: u32, scratch: u32, bits: u64) {
    let constant = bits & (u64::MAX >> (64 - fmt.bits()));
    let seq = fmov_const(fmt, v(rd), g(scratch), bits);
    let words = seq.as_slice();
    assert_eq!(words.len(), seq.len());
    let (value, scratch_value) = execute(words, rd, scratch);
    assert_eq!(value, constant, "{fmt:?} {bits:#x}: {words:x?} computes {value:#x}");

    // One instruction exactly for zero and for the FMOV immediates;
    // otherwise the general-purpose materialization and one FMOV.
    let immediate = fp_size(fmt).is_some_and(|sz| encodable(sz).contains(&constant));
    if constant == 0 || immediate {
        assert_eq!(words.len(), 1, "{fmt:?} {bits:#x}: {words:x?}");
        assert_eq!(scratch_value, None, "{fmt:?} {bits:#x} must not touch the scratch");
        // Zero is the MOVI idiom, not `fmov #imm` (which cannot say it).
        assert_eq!(words[0] == 0x2f00_e400 | rd, constant == 0);
    } else {
        let mov = mov_imm(gpr_size(fmt), g(scratch), constant);
        assert_eq!(&words[..words.len() - 1], mov.as_slice(), "{fmt:?} {bits:#x}");
        assert_eq!(scratch_value, Some(constant));
    }
    assert!(words.len() <= 5);
}

/// Bit patterns of every kind, for a format `bits` wide: zeros, the
/// extremes, infinities, NaNs quiet and signalling with payloads,
/// denormals, everyday constants, and a seeded random sample.
fn patterns(bits: u32, count: usize, seed: u64) -> Vec<u64> {
    let mask = u64::MAX >> (64 - bits);
    let sign = 1u64 << (bits - 1);
    let fraction_bits = match bits {
        16 => 10,
        32 => 23,
        _ => 52,
    };
    let exponent = (sign - 1) & !((1 << fraction_bits) - 1);
    let mut out = vec![0, sign, mask, 1, sign | 1, sign - 1, exponent, sign | exponent];
    // NaNs: quiet, signalling, with payloads, either sign.
    let quiet = 1u64 << (fraction_bits - 1);
    out.extend([exponent | quiet, exponent | 1, exponent | quiet | 0x155, sign | exponent | quiet]);
    out.extend([exponent | (quiet - 1), sign | exponent | 1]);
    // Denormals: smallest, largest, one bit in each position.
    out.extend([(1 << fraction_bits) - 1, sign | ((1 << fraction_bits) - 1)]);
    out.extend((0..fraction_bits).map(|bit| 1 << bit));
    // The smallest normal, the largest finite, values around one.
    out.extend([1 << fraction_bits, exponent - 1, sign | (exponent - 1)]);
    for value in [0.1, 0.5, 1.0, 1.5, 2.0, 3.0, 10.0, 100.0, 255.0, 1e10, 1e-10, 3.25e7] {
        for value in [value, -value] {
            out.push(match bits {
                64 => f64::to_bits(value),
                32 => u64::from(f32::to_bits(value as f32)),
                // A half with the single's top bits: not the conversion,
                // just one more pattern.
                _ => u64::from(f32::to_bits(value as f32) >> 16),
            });
        }
    }
    let mut rng = Rng::new(seed);
    while out.len() < count {
        let value = rng.next() & mask;
        out.push(match rng.below(4) {
            // Few set bits; few distinct 16-bit chunks; uniformly random.
            0 => value & rng.next() & rng.next(),
            1 => {
                let palette = [0, 0xffff, value & 0xffff, value >> 16 & 0xffff];
                (0..4).fold(0, |v, _| v << 16 | rng.pick(&palette)) & mask
            }
            _ => value,
        });
    }
    out
}

#[test]
fn sequences_compute_the_constant_and_have_the_specified_length() {
    for (fmt, _) in FP_FORMATS {
        let mut values = patterns(fmt.bits(), 20_000, 0xf0 + u64::from(fmt.bits()));
        if let Some(sz) = fp_size(fmt) {
            values.extend(encodable(sz));
        }
        for (i, bits) in values.into_iter().enumerate() {
            let (rd, scratch) = ((i % 32) as u32, (i * 7 % 31) as u32);
            check(fmt, rd, scratch, bits);
            // The bits above the format's width are ignored.
            if fmt != FpFormat::D {
                let above = (i as u64 + 1) << fmt.bits();
                assert_eq!(
                    fmov_const(fmt, v(rd), g(scratch), bits | above),
                    fmov_const(fmt, v(rd), g(scratch), bits)
                );
            }
        }
    }
    // Every half-precision pattern.
    for bits in 0..=0xffff {
        check(FpFormat::H, 3, 9, bits);
    }
}

/// The worst case is five instructions: a 64-bit pattern with four
/// different 16-bit chunks that no shorter general-purpose sequence
/// reaches.
#[test]
fn the_longest_sequence_is_five_instructions() {
    let seq = fmov_const(FpFormat::D, VReg::V0, Reg::X9, 0x3fb9_9999_9999_999a);
    assert_eq!(seq.len(), 4, "0.1 is an ORR pattern, two MOVKs and the FMOV");
    let seq = fmov_const(FpFormat::D, VReg::V0, Reg::X9, 0x4009_21fb_5444_2d18);
    assert_eq!(seq.len(), 5, "pi");
    assert_eq!(fmov_const(FpFormat::S, VReg::V0, Reg::X9, 0x4049_0fdb).len(), 3);
    assert_eq!(fmov_const(FpFormat::D, VReg::V0, Reg::X9, 1 << 63).len(), 2, "-0.0");
}

/// With LLVM: each one-instruction result and each two-instruction result
/// is what LLVM assembles for the same text (`mov` to the scratch register
/// takes one instruction exactly when LLVM's `mov` accepts the constant);
/// the last instruction of a longer one is the FMOV from the scratch
/// register; and `fmov <reg>, #value` is accepted exactly for the values
/// the predicate accepts.
pub fn build(b: &mut Batch) {
    for (fmt, p) in FP_FORMATS {
        let mut values = patterns(fmt.bits(), 1500, 0xb0 + u64::from(fmt.bits()));
        if let Some(sz) = fp_size(fmt) {
            values.extend(encodable(sz));
        }
        let gsz = gpr_size(fmt);
        let (gp, fp) = match fmt {
            FpFormat::D => ('x', 'd'),
            _ => ('w', 's'),
        };
        for (i, bits) in values.into_iter().enumerate() {
            let (rd, scratch) = ((i % 32) as u32, (i * 7 % 31) as u32);
            let seq = fmov_const(fmt, v(rd), g(scratch), bits);
            let words = seq.as_slice();
            let from_scratch = format!("fmov {fp}{rd}, {gp}{scratch}");
            match (words, fp_size(fmt)) {
                ([word], _) if bits == 0 => {
                    b.push("fmov_const", *word, None, format!("movi d{rd}, #0"));
                }
                ([word], Some(sz)) => {
                    let value = match sz {
                        FpSize::S => f64::from(f32::from_bits(bits as u32)),
                        FpSize::D => f64::from_bits(bits),
                    };
                    b.push("fmov_const", *word, None, format!("fmov {p}{rd}, #{value:?}"));
                }
                ([mov, fmov], _) => {
                    b.push("fmov_const", *mov, None, format!("mov {gp}{scratch}, #{bits:#x}"));
                    b.push("fmov_const", *fmov, None, from_scratch);
                }
                ([.., fmov], _) => {
                    assert_eq!(words.len(), 1 + mov_imm(gsz, g(scratch), bits).len());
                    b.push("fmov_const", *fmov, None, from_scratch);
                }
                ([], _) => unreachable!("a sequence is not empty"),
            }
        }
    }

    // The predicate against LLVM's, over single-precision patterns (LLVM
    // rounds the text to single precision before it looks, so only values
    // exact in single precision can be asked about): random finite ones,
    // and every encodable value with one bit flipped.
    let set = encodable(FpSize::S);
    let mut singles: Vec<u32> = Vec::new();
    let mut rng = Rng::new(0xf32);
    while singles.len() < 1000 {
        singles.push(rng.next() as u32);
    }
    for &bits in &set {
        singles.extend((0..32).map(|bit| bits as u32 ^ 1 << bit));
    }
    for (i, single) in singles.into_iter().enumerate() {
        let value = f32::from_bits(single);
        if !value.is_finite() || value == 0.0 {
            continue;
        }
        let value = f64::from(value);
        let d = (i % 32) as u32;
        for (sz, p) in FP_SIZES {
            let bits = fp_bits(sz, value);
            let asm = format!("fmov {p}{d}, #{value:?}");
            if set.contains(&u64::from(single)) {
                b.push("fmov_imm", try_fmov_imm(sz, v(d), bits), None, asm);
            } else {
                b.reject(
                    "fmov_imm",
                    try_fmov_imm(sz, v(d), bits),
                    &format!("({sz:?}, {bits:#x})"),
                    Some(asm),
                );
            }
        }
    }
}
