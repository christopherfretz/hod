//! The zlib compressor against an independent decoder: Python's `zlib`
//! (`spec/object.md` "Compressed sections", WP-061). Thousands of
//! generated inputs, among them the empty input, one byte, incompressible
//! bytes, long runs, short periods, text-like data and inputs past 64 KiB
//! and past a segment, are compressed by `hod_obj::deflate` and each
//! stream decoded by `zlib.decompressobj`: the stream must end exactly
//! where it ends, hold no trailing bytes, decode to the input, and pass
//! the decoder's Adler-32 check.

use std::process::Command;
use std::time::Duration;

use hod_obj::deflate::{self, Deflater, SEGMENT};
use hod_testutil::{Scratch, run_with, tool};

/// xorshift64*: the inputs depend only on the seed.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        self.0.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }

    fn below(&mut self, n: u64) -> u64 {
        self.next() % n
    }
}

/// The `i`th generated input.
fn input(i: u64) -> Vec<u8> {
    let mut rng = Rng(0x9E37_79B9_7F4A_7C15 ^ i.wrapping_mul(0xD1B5_4A32_D192_ED03) | 1);
    let len = match i {
        0 => 0,
        1..=3 => 1,
        4 => 65_535,
        5 => 65_536,
        6 => 65_537,
        7 => SEGMENT - 1,
        8 => SEGMENT,
        9 => SEGMENT + 1,
        10 => 3 * SEGMENT + 17,
        _ if i % 97 == 0 => 64 * 1024 + rng.below(200_000) as usize,
        _ if i % 5 == 0 => rng.below(8) as usize,
        _ => rng.below(4096) as usize,
    };
    let mut out = Vec::with_capacity(len);
    match i % 7 {
        // Incompressible.
        0 => out.extend((0..len).map(|_| rng.next() as u8)),
        // One byte repeated.
        1 => out.resize(len, rng.next() as u8),
        // A short period.
        2 => {
            let period = 1 + rng.below(40) as usize;
            let pattern: Vec<u8> = (0..period).map(|_| rng.next() as u8).collect();
            out.extend((0..len).map(|k| pattern[k % period]));
        }
        // Words of a small vocabulary: text-like, matches at all
        // distances.
        3 => {
            let words: Vec<Vec<u8>> = (0..50)
                .map(|_| (0..1 + rng.below(9)).map(|_| b'a' + rng.below(26) as u8).collect())
                .collect();
            while out.len() < len {
                out.extend_from_slice(&words[rng.below(50) as usize]);
                out.push(b' ');
            }
            out.truncate(len);
        }
        // Mostly zeros with sparse noise, like DWARF offsets.
        4 => out.extend((0..len).map(|_| if rng.below(8) == 0 { rng.next() as u8 } else { 0 })),
        // Runs of random lengths.
        5 => {
            while out.len() < len {
                let b = rng.next() as u8;
                let run = 1 + rng.below(600) as usize;
                out.extend(std::iter::repeat_n(b, run));
            }
            out.truncate(len);
        }
        // Two alphabets: skewed literal frequencies.
        _ => out.extend((0..len).map(|_| if rng.below(16) == 0 { rng.next() as u8 } else { b'x' })),
    }
    out
}

const DECODE: &str = r#"
import struct, sys, zlib
data = open(sys.argv[1], 'rb').read()
at = n = 0
while at < len(data):
    (a,) = struct.unpack_from('<I', data, at); at += 4
    want = data[at:at + a]; at += a
    (b,) = struct.unpack_from('<I', data, at); at += 4
    stream = data[at:at + b]; at += b
    d = zlib.decompressobj()
    try:
        got = d.decompress(stream) + d.flush()
    except zlib.error as e:
        print(f'input {n}: {e}'); sys.exit(1)
    if not d.eof or d.unused_data or d.unconsumed_tail:
        print(f'input {n}: stream does not end where it ends'); sys.exit(1)
    if got != want:
        print(f'input {n}: decodes to other bytes'); sys.exit(1)
    if zlib.adler32(want) != struct.unpack('>I', stream[-4:])[0]:
        print(f'input {n}: Adler-32 differs'); sys.exit(1)
    n += 1
print(n)
"#;

const INPUTS: u64 = 3000;

#[test]
fn python_zlib_decodes_every_stream() {
    let Some(python) = tool("python3") else { return };
    let scratch = Scratch::new("deflate-roundtrip");
    let mut records = Vec::new();
    let mut deflater = Deflater::new();
    let mut stats = [0usize; 2];
    for i in 0..INPUTS {
        let data = input(i);
        let mut stream = Vec::new();
        deflater.zlib(&data, &mut stream).unwrap();
        stats[0] += data.len();
        stats[1] += stream.len();
        // A stored segment costs 5 bytes per 65 535; nothing grows more.
        let bound = data.len() + 5 * data.len().div_ceil(65_535) + 8 + 5 * data.len().div_ceil(SEGMENT);
        assert!(stream.len() <= bound, "input {i}: {} bytes from {}", stream.len(), data.len());
        records.extend_from_slice(&(data.len() as u32).to_le_bytes());
        records.extend_from_slice(&data);
        records.extend_from_slice(&(stream.len() as u32).to_le_bytes());
        records.extend_from_slice(&stream);
    }
    let file = scratch.write("records.bin", &records);
    let script = scratch.write("decode.py", DECODE);
    let mut command = Command::new(python);
    command.arg("-I").arg(&script).arg(&file);
    let out = run_with(&mut command, &[], Duration::from_mins(5));
    assert!(out.success(), "{}{}", out.stdout_text(), out.stderr_text());
    assert_eq!(out.stdout_text().trim(), INPUTS.to_string());
    eprintln!("{INPUTS} streams decoded; {} bytes into {}", stats[0], stats[1]);
}

/// The stream depends only on the input: a fresh compressor, a reused
/// one (whose hash table holds entries of earlier inputs) and the free
/// function give the same bytes.
#[test]
fn streams_are_deterministic() {
    let mut reused = Deflater::new();
    for i in (0..400).chain([4, 7, 8, 9, 10]) {
        let data = input(i);
        let (mut a, mut b, mut c) = (Vec::new(), Vec::new(), Vec::new());
        Deflater::new().zlib(&data, &mut a).unwrap();
        reused.zlib(&data, &mut b).unwrap();
        deflate::zlib(&data, &mut c).unwrap();
        assert!(a == b && b == c, "input {i}");
    }
}

/// Compressible input shrinks: the matcher and the dynamic block work.
#[test]
fn repetitive_input_shrinks() {
    let data: Vec<u8> = b"DW_TAG_subprogram DW_AT_name ".iter().copied().cycle().take(100_000).collect();
    let mut out = Vec::new();
    deflate::zlib(&data, &mut out).unwrap();
    assert!(out.len() < data.len() / 50, "{} bytes", out.len());
    let zeros = vec![0u8; 1 << 20];
    out.clear();
    deflate::zlib(&zeros, &mut out).unwrap();
    assert!(out.len() < 4096, "{} bytes", out.len());
}
