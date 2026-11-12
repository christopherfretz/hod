//! A zlib (RFC 1950) compressor of DEFLATE (RFC 1951) data, written for
//! speed first and ratio second: what `SHF_COMPRESSED` sections with
//! `ELFCOMPRESS_ZLIB` hold (`spec/object.md` "Compressed sections").
//!
//! The format produced, for any input:
//!
//! - the zlib header `78 01` (deflate, 32 KiB window, no dictionary,
//!   "fastest" level);
//! - the input cut into segments of [`SEGMENT`] bytes (the last one
//!   shorter), each compressed **independently** (no match reaches into
//!   an earlier segment) into one non-final block, dynamic Huffman, fixed
//!   Huffman or stored, whichever is smallest (a stored segment is
//!   several stored blocks of at most 65 535 bytes), then padded to a
//!   byte boundary by an empty non-final stored block when it does not
//!   end on one;
//! - an empty final fixed-Huffman block (`03 00`);
//! - the Adler-32 of the input, big-endian.
//!
//! So the stream of an input depends only on the input (not on threads,
//! addresses or time), and the segments could be compressed in parallel
//! and concatenated.
//!
//! The matcher is greedy LZ77 with one candidate per position: a hash
//! table of [`HASH_SIZE`] positions indexed by the next four bytes, no
//! chains, matches of 4 to 258 bytes within 32 KiB, compared eight bytes
//! at a time. Positions inside a match are not hashed, except the two
//! after its start and the last one. After 32 consecutive misses the
//! search steps over more bytes (one more per 32 misses), so that
//! incompressible data is scanned quickly; those bytes are literals.
//!
//! Memory: a [`Deflater`] holds the hash table (128 KiB) and one token
//! (4 bytes) per input byte of a segment (at most 1 MiB), allocated once
//! and reused; nothing is allocated per match or per block beyond the
//! output. The table is not cleared between segments or inputs: its
//! entries carry a base that marks those of earlier segments stale, so a
//! small input costs no 128 KiB clear.

/// The bytes of input compressed as one independent segment.
pub const SEGMENT: usize = 1 << 18;
/// Entries of the hash table.
pub const HASH_SIZE: usize = 1 << HASH_BITS;
const HASH_BITS: u32 = 15;
const WINDOW: usize = 32 * 1024;
const MIN_MATCH: usize = 4;
const MAX_MATCH: usize = 258;
/// The largest input [`zlib`] takes: 4 GiB less one byte. Nothing in the
/// format limits the length; a larger input is refused because it has
/// never been tested.
pub const MAX_INPUT: u64 = u32::MAX as u64;

/// A match token has this bit; else the token is a literal byte.
const MATCH: u32 = 1 << 31;

const LEN_BASE: [u16; 29] = [
    3, 4, 5, 6, 7, 8, 9, 10, 11, 13, 15, 17, 19, 23, 27, 31, 35, 43, 51, 59, 67, 83, 99, 115, 131,
    163, 195, 227, 258,
];
const LEN_EXTRA: [u8; 29] =
    [0, 0, 0, 0, 0, 0, 0, 0, 1, 1, 1, 1, 2, 2, 2, 2, 3, 3, 3, 3, 4, 4, 4, 4, 5, 5, 5, 5, 0];
const DIST_BASE: [u16; 30] = [
    1, 2, 3, 4, 5, 7, 9, 13, 17, 25, 33, 49, 65, 97, 129, 193, 257, 385, 513, 769, 1025, 1537,
    2049, 3073, 4097, 6145, 8193, 12289, 16385, 24577,
];
const DIST_EXTRA: [u8; 30] =
    [0, 0, 0, 0, 1, 1, 2, 2, 3, 3, 4, 4, 5, 5, 6, 6, 7, 7, 8, 8, 9, 9, 10, 10, 11, 11, 12, 12, 13, 13];
/// The order in which the code-length code's lengths are sent.
const CL_ORDER: [usize; 19] = [16, 17, 18, 0, 8, 7, 9, 6, 10, 5, 11, 4, 12, 3, 13, 2, 14, 1, 15];

/// Length code (0..29) of each match length less 3.
const LEN_CODE: [u8; 256] = {
    let mut t = [0u8; 256];
    let mut code = 0;
    while code < 29 {
        let base = LEN_BASE[code] as usize - 3;
        let n = if code == 28 { 1 } else { 1 << LEN_EXTRA[code] };
        let mut i = 0;
        while i < n && base + i < 256 {
            t[base + i] = code as u8;
            i += 1;
        }
        code += 1;
    }
    // 258 has its own code (28); 227..=257 are code 27.
    t[255] = 28;
    t
};

/// Distance code of a distance less one: `d < 256` at `d`, else at
/// `256 + (d >> 7)`.
const DIST_CODE: [u8; 512] = {
    let mut t = [0u8; 512];
    let mut code = 0;
    while code < 30 {
        let base = DIST_BASE[code] as usize - 1;
        let n = 1usize << DIST_EXTRA[code];
        let mut d = base;
        while d < base + n {
            if d < 256 {
                t[d] = code as u8;
            } else {
                t[256 + (d >> 7)] = code as u8;
            }
            d += 1;
        }
        code += 1;
    }
    t
};

const fn dist_code(d: usize) -> usize {
    (if d < 256 { DIST_CODE[d] } else { DIST_CODE[256 + (d >> 7)] }) as usize
}

const ADLER_MOD: u32 = 65521;

/// The Adler-32 checksum of `data` (RFC 1950 §8.2).
pub fn adler32(data: &[u8]) -> u32 {
    adler32_update(1, data)
}

fn adler32_update(adler: u32, data: &[u8]) -> u32 {
    let (mut a, mut b) = (adler & 0xffff, adler >> 16);
    // 5552 is the largest n with 255 n (n + 1) / 2 + (n + 1)(65520) < 2^32.
    for chunk in data.chunks(5552) {
        let mut quads = chunk.chunks_exact(4);
        for q in &mut quads {
            a += u32::from(q[0]);
            b += a;
            a += u32::from(q[1]);
            b += a;
            a += u32::from(q[2]);
            b += a;
            a += u32::from(q[3]);
            b += a;
        }
        for &x in quads.remainder() {
            a += u32::from(x);
            b += a;
        }
        a %= ADLER_MOD;
        b %= ADLER_MOD;
    }
    b << 16 | a
}

/// Appends the zlib stream of `input` to `out`, with a fresh
/// [`Deflater`]. A caller compressing many inputs keeps one and calls
/// [`Deflater::zlib`].
///
/// # Errors
/// `Err(())` for an input longer than [`MAX_INPUT`]; `out` is unchanged.
#[allow(clippy::result_unit_err, reason = "the one error has no detail")]
pub fn zlib(input: &[u8], out: &mut Vec<u8>) -> Result<(), ()> {
    Deflater::new().zlib(input, out)
}

/// The compressor's reusable state.
#[derive(Debug)]
pub struct Deflater {
    /// Per hash: `base` plus a position of the segment plus one, or a
    /// value not above `base` (an entry of an earlier segment: no
    /// candidate). So the table is cleared only when `base` would wrap.
    table: Vec<u32>,
    base: u32,
    tokens: Vec<u32>,
    ll_freq: [u32; 286],
    d_freq: [u32; 30],
    /// The extra bits the tokens take.
    extra_bits: u64,
}

impl Default for Deflater {
    fn default() -> Self {
        Self::new()
    }
}

/// A Huffman code: per symbol its length and its bits, reversed (DEFLATE
/// sends codes from their first bit).
#[derive(Clone, Copy)]
struct Code<const N: usize> {
    len: [u8; N],
    bits: [u16; N],
}

impl<const N: usize> Code<N> {
    const fn empty() -> Self {
        Self { len: [0; N], bits: [0; N] }
    }

    /// The canonical code of the lengths `self.len` (RFC 1951 §3.2.2).
    fn assign(&mut self) {
        let mut count = [0u16; 16];
        for &l in &self.len {
            count[l as usize] += 1;
        }
        count[0] = 0;
        let mut next = [0u16; 16];
        let mut code = 0u16;
        for bits in 1..16 {
            code = (code + count[bits - 1]) << 1;
            next[bits] = code;
        }
        for (s, &l) in self.len.iter().enumerate() {
            if l != 0 {
                let c = next[l as usize];
                next[l as usize] += 1;
                self.bits[s] = c.reverse_bits() >> (16 - l);
            }
        }
    }
}

/// The fixed code (RFC 1951 §3.2.6).
fn fixed_codes() -> (Code<288>, Code<30>) {
    let mut ll = Code::<288>::empty();
    for (s, l) in ll.len.iter_mut().enumerate() {
        *l = match s {
            0..=143 => 8,
            144..=255 => 9,
            256..=279 => 7,
            _ => 8,
        };
    }
    ll.assign();
    let mut d = Code::<30>::empty();
    d.len = [5; 30];
    d.assign();
    (ll, d)
}

/// Code lengths for `freq`, none longer than `limit`: Moffat and
/// Katajainen's in-place minimum-redundancy lengths over the symbols
/// sorted by frequency (ties by symbol), then lengths over the limit
/// folded back as zlib and miniz do, longest codes to the least frequent
/// symbols. Symbols of frequency 0 get length 0. At least two symbols
/// must have a frequency (the caller ensures it).
fn lengths(freq: &[u32], limit: u32, len: &mut [u8]) {
    let mut syms = [(0u32, 0u16); 288];
    let mut n = 0;
    for (s, &f) in freq.iter().enumerate() {
        if f != 0 {
            syms[n] = (f, s as u16);
            n += 1;
        }
    }
    len.fill(0);
    let syms = &mut syms[..n];
    syms.sort_unstable();
    if n == 1 {
        len[syms[0].1 as usize] = 1;
        return;
    }
    let mut a = [0u32; 288];
    for (x, s) in a.iter_mut().zip(syms.iter()) {
        *x = s.0;
    }
    let a = &mut a[..n];
    // Pass 1: the tree's internal weights, parents in place.
    a[0] += a[1];
    let (mut root, mut leaf) = (0usize, 2usize);
    for next in 1..n - 1 {
        if leaf >= n || a[root] < a[leaf] {
            a[next] = a[root];
            a[root] = next as u32;
            root += 1;
        } else {
            a[next] = a[leaf];
            leaf += 1;
        }
        if leaf >= n || (root < next && a[root] < a[leaf]) {
            a[next] += a[root];
            a[root] = next as u32;
            root += 1;
        } else {
            a[next] += a[leaf];
            leaf += 1;
        }
    }
    // Pass 2: internal depths.
    a[n - 2] = 0;
    for next in (0..n - 2).rev() {
        a[next] = a[a[next] as usize] + 1;
    }
    // Pass 3: leaf depths, deepest first.
    let (mut avail, mut used, mut depth) = (1i64, 0i64, 0u32);
    let (mut root, mut next) = (n as i64 - 2, n as i64 - 1);
    while avail > 0 {
        while root >= 0 && a[root as usize] == depth {
            used += 1;
            root -= 1;
        }
        while avail > used {
            a[next as usize] = depth;
            next -= 1;
            avail -= 1;
        }
        avail = 2 * used;
        depth += 1;
        used = 0;
    }
    // `a[i]` is now the length of the i-th least frequent symbol
    // (non-increasing in i). Count by length, fold into the limit.
    let mut count = [0u32; 33];
    for &l in a.iter() {
        count[(l as usize).min(32)] += 1;
    }
    let limit = limit as usize;
    for l in limit + 1..33 {
        count[limit] += count[l];
        count[l] = 0;
    }
    let mut total: u64 = (1..=limit).map(|l| u64::from(count[l]) << (limit - l)).sum();
    while total != 1 << limit {
        count[limit] -= 1;
        for l in (1..limit).rev() {
            if count[l] != 0 {
                count[l] -= 1;
                count[l + 1] += 2;
                break;
            }
        }
        total -= 1;
    }
    // Shortest lengths to the most frequent symbols.
    let mut j = n;
    for (l, &c) in count.iter().enumerate().take(limit + 1).skip(1) {
        for _ in 0..c {
            j -= 1;
            len[syms[j].1 as usize] = l as u8;
        }
    }
}

/// Makes sure at least two symbols of `freq` have a frequency, so that
/// every code is complete (as zlib's `trees.c` does).
fn at_least_two(freq: &mut [u32]) {
    let used = freq.iter().filter(|&&f| f != 0).count();
    for s in 0..2 {
        if used + s < 2 {
            let free = freq.iter().position(|&f| f == 0).unwrap_or(0);
            freq[free] = 1;
        }
    }
}

/// Bits written from the least significant end, into a vector.
struct Bits<'o> {
    out: &'o mut Vec<u8>,
    buf: u64,
    n: u32,
}

impl Bits<'_> {
    /// `value` holds at most `len` bits; `len` is at most 32.
    #[inline(always)]
    fn put(&mut self, value: u32, len: u32) {
        self.buf |= u64::from(value) << self.n;
        self.n += len;
        if self.n >= 32 {
            self.out.extend_from_slice(&(self.buf as u32).to_le_bytes());
            self.buf >>= 32;
            self.n -= 32;
        }
    }

    /// Pads with zero bits to a byte boundary and writes what is held.
    fn align(&mut self) {
        while self.n > 0 {
            self.out.push(self.buf as u8);
            self.buf >>= 8;
            self.n = self.n.saturating_sub(8);
        }
        self.buf = 0;
    }
}

/// The dynamic block's header, made ahead of writing it.
struct Header {
    ll: Code<288>,
    d: Code<30>,
    cl: Code<19>,
    hlit: usize,
    hdist: usize,
    hclen: usize,
    /// The run-length coded lengths: symbol and extra-bit value.
    runs: [(u8, u8); 286 + 30],
    nruns: usize,
    bits: u64,
}

impl Deflater {
    /// A compressor; allocates its tables once.
    pub fn new() -> Self {
        Self {
            table: vec![0; HASH_SIZE],
            base: 0,
            tokens: Vec::new(),
            ll_freq: [0; 286],
            d_freq: [0; 30],
            extra_bits: 0,
        }
    }

    /// Appends the zlib stream of `input` to `out` (the format of the
    /// module's documentation).
    ///
    /// # Errors
    /// `Err(())` for an input longer than [`MAX_INPUT`]; `out` is
    /// unchanged.
    #[allow(clippy::result_unit_err, reason = "the one error has no detail")]
    pub fn zlib(&mut self, input: &[u8], out: &mut Vec<u8>) -> Result<(), ()> {
        if input.len() as u64 > MAX_INPUT {
            return Err(());
        }
        out.reserve(input.len() / 3 + 64);
        out.extend_from_slice(&[0x78, 0x01]);
        let mut adler = 1;
        for segment in input.chunks(SEGMENT) {
            self.segment(segment, out);
            adler = adler32_update(adler, segment);
        }
        // An empty final fixed block: BFINAL 1, BTYPE 01, end of block.
        out.extend_from_slice(&[0x03, 0x00]);
        out.extend_from_slice(&adler.to_be_bytes());
        Ok(())
    }

    /// Appends the non-final blocks of one segment, ending on a byte
    /// boundary. `out` starts on one.
    fn segment(&mut self, s: &[u8], out: &mut Vec<u8>) {
        self.matches(s);
        self.ll_freq[256] = 1;
        let data_bits = |ll: &[u8], d: &[u8], lf: &[u32], df: &[u32]| -> u64 {
            let a: u64 = lf.iter().zip(ll).map(|(&f, &l)| u64::from(f) * u64::from(l)).sum();
            let b: u64 = df.iter().zip(d).map(|(&f, &l)| u64::from(f) * u64::from(l)).sum();
            a + b
        };
        let (fixed_ll, fixed_d) = fixed_codes();
        let fixed =
            3 + data_bits(&fixed_ll.len[..286], &fixed_d.len, &self.ll_freq, &self.d_freq)
                + self.extra_bits;
        let header = self.header();
        let dynamic = header.bits
            + data_bits(&header.ll.len[..286], &header.d.len, &self.ll_freq, &self.d_freq)
            + self.extra_bits;
        let stored = 8 * (s.len() as u64 + 5 * s.len().div_ceil(65535) as u64);
        if stored < dynamic.min(fixed) {
            for block in s.chunks(65535) {
                let n = block.len() as u16;
                out.push(0); // BFINAL 0, BTYPE 00, padding
                out.extend_from_slice(&n.to_le_bytes());
                out.extend_from_slice(&(!n).to_le_bytes());
                out.extend_from_slice(block);
            }
            return;
        }
        let use_dynamic = dynamic <= fixed;
        out.reserve((dynamic.min(fixed) / 8) as usize + 16);
        let mut bits = Bits { out, buf: 0, n: 0 };
        let (ll, d) = if use_dynamic {
            bits.put(0b100, 3); // BFINAL 0, BTYPE 10
            bits.put((header.hlit - 257) as u32, 5);
            bits.put((header.hdist - 1) as u32, 5);
            bits.put((header.hclen - 4) as u32, 4);
            for &s in &CL_ORDER[..header.hclen] {
                bits.put(u32::from(header.cl.len[s]), 3);
            }
            for &(sym, extra) in &header.runs[..header.nruns] {
                let s = sym as usize;
                bits.put(u32::from(header.cl.bits[s]), u32::from(header.cl.len[s]));
                match sym {
                    16 => bits.put(u32::from(extra), 2),
                    17 => bits.put(u32::from(extra), 3),
                    18 => bits.put(u32::from(extra), 7),
                    _ => {}
                }
            }
            (&header.ll, &header.d)
        } else {
            bits.put(0b010, 3); // BFINAL 0, BTYPE 01
            (&fixed_ll, &fixed_d)
        };
        for &t in &self.tokens {
            if t & MATCH == 0 {
                let s = t as usize;
                bits.put(u32::from(ll.bits[s]), u32::from(ll.len[s]));
            } else {
                let len = ((t >> 16) & 0xff) as usize;
                let dist = (t & 0xffff) as usize;
                let lc = LEN_CODE[len] as usize;
                let ls = 257 + lc;
                let le = u32::from(LEN_EXTRA[lc]);
                let lv = (len + 3 - LEN_BASE[lc] as usize) as u32;
                let ln = u32::from(ll.len[ls]);
                bits.put(u32::from(ll.bits[ls]) | lv << ln, ln + le);
                let dc = dist_code(dist);
                let de = u32::from(DIST_EXTRA[dc]);
                let dv = (dist + 1 - DIST_BASE[dc] as usize) as u32;
                let dn = u32::from(d.len[dc]);
                bits.put(u32::from(d.bits[dc]) | dv << dn, dn + de);
            }
        }
        bits.put(u32::from(ll.bits[256]), u32::from(ll.len[256]));
        if bits.n % 8 != 0 {
            // An empty stored block brings the stream to a byte boundary.
            bits.put(0, 3);
            bits.align();
            bits.out.extend_from_slice(&[0x00, 0x00, 0xff, 0xff]);
        } else {
            bits.align();
        }
    }

    /// Greedy LZ77 over one segment into `tokens`, counting symbols.
    fn matches(&mut self, s: &[u8]) {
        if u64::from(self.base) + s.len() as u64 + 2 > u64::from(u32::MAX) {
            self.table.fill(0);
            self.base = 0;
        }
        let base = self.base;
        self.base += s.len() as u32 + 1;
        self.tokens.clear();
        self.tokens.reserve(s.len());
        self.ll_freq = [0; 286];
        self.d_freq = [0; 30];
        self.extra_bits = 0;
        let n = s.len();
        let read4 = |at: usize| u32::from_le_bytes([s[at], s[at + 1], s[at + 2], s[at + 3]]);
        let hash = |v: u32| (v.wrapping_mul(0x9E37_79B1) >> (32 - HASH_BITS)) as usize;
        let mut pos = 0;
        let mut misses = 0u32;
        while pos + MIN_MATCH <= n {
            let cur = read4(pos);
            let h = hash(cur);
            let entry = self.table[h];
            self.table[h] = base + pos as u32 + 1;
            let from = entry.wrapping_sub(base + 1) as usize;
            if entry > base && pos - from <= WINDOW && read4(from) == cur {
                let max = (n - pos).min(MAX_MATCH);
                let mut len = MIN_MATCH;
                while len + 8 <= max {
                    let a = u64::from_le_bytes(s[from + len..from + len + 8].try_into().unwrap());
                    let b = u64::from_le_bytes(s[pos + len..pos + len + 8].try_into().unwrap());
                    let x = a ^ b;
                    if x != 0 {
                        len += (x.trailing_zeros() / 8) as usize;
                        break;
                    }
                    len += 8;
                }
                if len + 8 > max {
                    while len < max && s[from + len] == s[pos + len] {
                        len += 1;
                    }
                }
                let dist = pos - from;
                self.tokens.push(MATCH | ((len - 3) as u32) << 16 | (dist - 1) as u32);
                let lc = LEN_CODE[len - 3] as usize;
                let dc = dist_code(dist - 1);
                self.ll_freq[257 + lc] += 1;
                self.d_freq[dc] += 1;
                self.extra_bits += u64::from(LEN_EXTRA[lc]) + u64::from(DIST_EXTRA[dc]);
                // A few positions inside the match keep the table useful.
                let end = pos + len;
                for at in [pos + 1, pos + 2, end - 1] {
                    if at + MIN_MATCH <= n && at < end {
                        self.table[hash(read4(at))] = base + at as u32 + 1;
                    }
                }
                pos = end;
                misses = 0;
                continue;
            }
            misses += 1;
            let step = 1 + (misses >> 5) as usize;
            let stop = (pos + step).min(n);
            for &b in &s[pos..stop] {
                self.tokens.push(u32::from(b));
                self.ll_freq[b as usize] += 1;
            }
            pos = stop;
        }
        for &b in &s[pos.min(n)..] {
            self.tokens.push(u32::from(b));
            self.ll_freq[b as usize] += 1;
        }
    }

    /// The dynamic block's codes and header, from the frequencies.
    fn header(&mut self) -> Header {
        let mut ll_freq = self.ll_freq;
        let mut d_freq = self.d_freq;
        at_least_two(&mut ll_freq);
        at_least_two(&mut d_freq);
        let mut ll = Code::<288>::empty();
        lengths(&ll_freq, 15, &mut ll.len[..286]);
        ll.assign();
        let mut d = Code::<30>::empty();
        lengths(&d_freq, 15, &mut d.len);
        d.assign();
        let hlit = 257 + ll.len[257..286].iter().rposition(|&l| l != 0).map_or(0, |p| p + 1);
        let hdist = 1 + d.len.iter().rposition(|&l| l != 0).unwrap_or(0);

        // The lengths, run-length coded (RFC 1951 §3.2.7).
        let mut all = [0u8; 286 + 30];
        all[..hlit].copy_from_slice(&ll.len[..hlit]);
        all[hlit..hlit + hdist].copy_from_slice(&d.len[..hdist]);
        let all = &all[..hlit + hdist];
        let mut runs = [(0u8, 0u8); 286 + 30];
        let mut nruns = 0;
        let mut push = |sym: u8, extra: u8| {
            runs[nruns] = (sym, extra);
            nruns += 1;
        };
        let mut i = 0;
        while i < all.len() {
            let l = all[i];
            let mut run = 1;
            while i + run < all.len() && all[i + run] == l {
                run += 1;
            }
            i += run;
            if l == 0 {
                while run >= 11 {
                    let r = run.min(138);
                    push(18, (r - 11) as u8);
                    run -= r;
                }
                if run >= 3 {
                    push(17, (run - 3) as u8);
                    run = 0;
                }
            } else {
                push(l, 0);
                run -= 1;
                while run >= 3 {
                    let r = run.min(6);
                    push(16, (r - 3) as u8);
                    run -= r;
                }
            }
            for _ in 0..run {
                push(l, 0);
            }
        }
        let mut cl_freq = [0u32; 19];
        for &(sym, _) in &runs[..nruns] {
            cl_freq[sym as usize] += 1;
        }
        at_least_two(&mut cl_freq);
        let mut cl = Code::<19>::empty();
        lengths(&cl_freq, 7, &mut cl.len);
        cl.assign();
        let hclen = 4.max(1 + CL_ORDER.iter().rposition(|&s| cl.len[s] != 0).unwrap_or(0));
        let mut bits = 3 + 5 + 5 + 4 + 3 * hclen as u64;
        for &(sym, _) in &runs[..nruns] {
            bits += u64::from(cl.len[sym as usize])
                + match sym {
                    16 => 2,
                    17 => 3,
                    18 => 7,
                    _ => 0,
                };
        }
        Header { ll, d, cl, hlit, hdist, hclen, runs, nruns, bits }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// RFC 1950 §9's example and the empty input.
    #[test]
    fn adler32_values() {
        assert_eq!(adler32(b""), 1);
        assert_eq!(adler32(b"Wikipedia"), 0x11E6_0398);
        let big = vec![0xffu8; 100_000];
        // Computed byte by byte, without the deferred reduction.
        let (mut a, mut b) = (1u32, 0u32);
        for &x in &big {
            a = (a + u32::from(x)) % ADLER_MOD;
            b = (b + a) % ADLER_MOD;
        }
        assert_eq!(adler32(&big), b << 16 | a);
    }

    /// The lookup tables against the RFC's ranges.
    #[test]
    fn code_tables() {
        for len in 3..=258usize {
            let c = LEN_CODE[len - 3] as usize;
            let base = LEN_BASE[c] as usize;
            assert!(base <= len, "{len}");
            let span = if c == 28 { 1 } else { 1 << LEN_EXTRA[c] };
            assert!(len < base + span, "{len}");
        }
        assert_eq!(LEN_CODE[258 - 3], 28);
        assert_eq!(LEN_CODE[257 - 3], 27);
        for dist in 1..=32768usize {
            let c = dist_code(dist - 1);
            let base = DIST_BASE[c] as usize;
            assert!(base <= dist && dist < base + (1 << DIST_EXTRA[c]), "{dist}");
        }
    }

    /// Lengths are a complete prefix code within the limit, and the
    /// fixed code is the RFC's.
    #[test]
    fn huffman_lengths() {
        let mut freq = [0u32; 286];
        for (i, f) in freq.iter_mut().enumerate() {
            *f = 1 + (i as u32 % 7) * (1 << (i % 20));
        }
        let mut len = [0u8; 286];
        lengths(&freq, 15, &mut len);
        let kraft: f64 = len.iter().map(|&l| 0.5f64.powi(i32::from(l))).sum();
        assert!((kraft - 1.0).abs() < 1e-12, "{kraft}");
        assert!(len.iter().all(|&l| (1..=15).contains(&l)));
        let mut seven = [0u32; 19];
        for (i, f) in seven.iter_mut().enumerate() {
            *f = 1 << i;
        }
        let mut len = [0u8; 19];
        lengths(&seven, 7, &mut len);
        assert!(len.iter().all(|&l| (1..=7).contains(&l)));
        let kraft: f64 = len.iter().map(|&l| 0.5f64.powi(i32::from(l))).sum();
        assert!((kraft - 1.0).abs() < 1e-12, "{kraft}");
        let (ll, _) = fixed_codes();
        // RFC 1951 §3.2.6: 256 is 0000000, 0 is 00110000, 144 is 110010000.
        assert_eq!((ll.bits[256], ll.len[256]), (0, 7));
        assert_eq!((ll.bits[0], ll.len[0]), (0b0011_0000u16.reverse_bits() >> 8, 8));
        assert_eq!((ll.bits[144], ll.len[144]), (0b1_1001_0000u16.reverse_bits() >> 7, 9));
    }

    #[test]
    fn empty_input() {
        let mut out = Vec::new();
        zlib(b"", &mut out).unwrap();
        assert_eq!(out, [0x78, 0x01, 0x03, 0x00, 0, 0, 0, 1]);
    }
}
