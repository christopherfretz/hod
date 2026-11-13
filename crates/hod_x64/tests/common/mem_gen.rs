//! A generator of straight-line functions of the x86-64 memory group
//! (WP-123) for `tests/mem.rs`: what the memory group and the skeleton
//! lower and nothing else (arguments, constants, stack slots, globals,
//! `alloca`, `load`, `store`, `ptradd`, `br`, `ret`).
//!
//! AArch64's generator (`hod_a64/tests/common/mem_gen.rs`, WP-036)
//! extended with the `Fpr` class, which `mem.rs` lowers on x86-64:
//! `float`, `double` and the vectors of 64
//! and 128 bits, loaded, stored and passed as values, `float` and
//! `double` also as arguments, constants of the scalar ones stored as
//! immediates.
//!
//! A function is `iR @<name>(ptr %buf, <params>)`: `%buf` points at 256
//! bytes whose last byte ends a mapped page, every other parameter is a
//! value of some type used as a stored value, an index or an `alloca`
//! count. The generator simulates memory as it goes, so that every load
//! reads written bytes with zero padding bits and every access is in
//! bounds and as aligned as it says: the program is defined, and
//! `hod_interp`, not this simulation, is the oracle.

use std::fmt::Write as _;

/// `SplitMix64`.
pub struct Rng(pub u64);

impl Rng {
    pub fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9e37_79b9_7f4a_7c15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
        z ^ (z >> 31)
    }

    pub fn below(&mut self, n: u64) -> u64 {
        self.next() % n
    }

    pub fn chance(&mut self, num: u64, den: u64) -> bool {
        self.below(den) < num
    }

    pub fn pick<T: Copy>(&mut self, items: &[T]) -> T {
        items[self.below(items.len() as u64) as usize]
    }

    /// A small signed value in `-range..=range`.
    pub fn small(&mut self, range: i64) -> i64 {
        self.below(2 * range as u64 + 1) as i64 - range
    }
}

/// The class of a value's type.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Kind {
    Int,
    Ptr,
    /// A floating-point or vector type of class `Fpr`, by its name.
    Fp(&'static str),
}

/// A `Gpr`- or `Fpr`-class type: `bits` is the integer width or the
/// store size in bits.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Ty {
    pub kind: Kind,
    pub bits: u32,
}

/// The `Fpr` types: `(name, store bytes)`. `half`, `bfloat` and `fp128`,
/// which `hod_interp` does not run, are `tests/mem.rs`'s
/// `the_interpreterless_floats_move_their_bits`.
pub const FPRS: [(&str, u32); 7] = [
    ("float", 4),
    ("double", 8),
    ("<2 x float>", 8),
    ("<4 x i16>", 8),
    ("<4 x float>", 16),
    ("<2 x i64>", 16),
    ("<16 x i8>", 16),
];

impl Ty {
    pub const fn int(bits: u32) -> Ty {
        Ty { kind: Kind::Int, bits }
    }

    pub const PTR: Ty = Ty { kind: Kind::Ptr, bits: 64 };

    pub const fn fp(name: &'static str, bytes: u32) -> Ty {
        Ty { kind: Kind::Fp(name), bits: bytes * 8 }
    }

    pub fn is_ptr(self) -> bool {
        self.kind == Kind::Ptr
    }

    pub fn is_fp(self) -> bool {
        matches!(self.kind, Kind::Fp(_))
    }

    pub fn name(self) -> String {
        match self.kind {
            Kind::Int => format!("i{}", self.bits),
            Kind::Ptr => "ptr".into(),
            Kind::Fp(name) => name.into(),
        }
    }

    /// The C type of a parameter of this type.
    pub fn c_name(self) -> &'static str {
        match self.kind {
            Kind::Fp("float") => "float",
            Kind::Fp("double") => "double",
            _ => "uint64_t",
        }
    }

    /// The store size.
    pub fn store(self) -> u64 {
        u64::from(self.bits.div_ceil(8))
    }

    /// The ABI alignment of the x86-64 data layout.
    pub fn natural_align(self) -> u64 {
        match self.store() {
            1 => 1,
            2 => 2,
            3 | 4 => 4,
            5..=8 => 8,
            _ => 16,
        }
    }

    /// The meaningful bits of the low word (the integer width; every bit
    /// of a floating-point or vector word).
    pub fn mask(self) -> u64 {
        if self.bits >= 64 || self.is_fp() { u64::MAX } else { (1 << self.bits) - 1 }
    }
}

/// Every integer width the memory group is tested at: each store size,
/// the odd widths around each, the encodings' boundaries.
pub const WIDTHS: [u32; 18] = [1, 7, 8, 9, 15, 16, 17, 24, 31, 32, 33, 40, 47, 48, 55, 56, 63, 64];

/// Index widths: `sext` from each, including the free `movsxd` of 32.
const INDEX_WIDTHS: [u32; 9] = [1, 7, 8, 16, 17, 32, 33, 63, 64];

/// Element types of `getelementptr`: `(text, alloc size)`. The sizes
/// that are no factor of an addressing mode (3, 10, 12, 24) take a
/// multiply; 32 a shift.
const ELEMS: [(&str, u64); 12] = [
    ("i8", 1),
    ("i16", 2),
    ("i32", 4),
    ("i64", 8),
    ("[3 x i8]", 3),
    ("[12 x i8]", 12),
    ("[5 x i16]", 10),
    ("i24", 4),
    ("{ i8, i32, i16 }", 12),
    ("[2 x i64]", 16),
    ("[4 x i64]", 32),
    ("[3 x i64]", 24),
];

/// The size of `%buf`.
pub const BUF: u64 = 256;
/// The size of each global of a function.
pub const GLOBAL: u64 = 64;

/// A parameter after `%buf`.
#[derive(Clone, Copy, Debug)]
pub struct Param {
    pub ty: Ty,
    /// The value, the bits above the width clear (a `float`'s or
    /// `double`'s encoding).
    pub value: u64,
    /// What the native caller puts above the width (§2.2: the callee may
    /// not rely on those bits).
    pub garbage: u64,
}

impl Param {
    /// The 64 bits the native caller passes.
    pub fn native(&self) -> u64 {
        (self.value & self.ty.mask()) | (self.garbage & !self.ty.mask())
    }

    /// The argument in C.
    pub fn c_arg(&self) -> String {
        match self.ty.kind {
            Kind::Fp("float") => format!("f32({}u)", self.value as u32),
            Kind::Fp("double") => format!("f64({}ULL)", self.value),
            _ => format!("{}ULL", self.native()),
        }
    }

    /// The argument as an `.ll` constant.
    pub fn ll_arg(&self) -> String {
        format!("{} {}", self.ty.name(), const_text(self.ty, self.value, 0))
    }
}

/// A variable of the function's module.
#[derive(Clone, Debug)]
pub struct Global {
    pub name: String,
    pub init: Vec<u8>,
    pub dso_local: bool,
}

/// A generated function.
#[derive(Clone, Debug)]
pub struct Func {
    pub name: String,
    pub ret: Ty,
    pub params: Vec<Param>,
    /// The `define`.
    pub text: String,
    pub globals: Vec<Global>,
    /// The initial bytes of `%buf`.
    pub buf: Vec<u8>,
}

impl Func {
    /// The definitions of the function's variables.
    pub fn globals_text(&self) -> String {
        let mut out = String::new();
        for g in &self.globals {
            let local = if g.dso_local { "dso_local " } else { "" };
            writeln!(
                out,
                "@{} = {local}global [{} x i8] {}, align 16",
                g.name,
                g.init.len(),
                bytes(&g.init)
            )
            .unwrap();
        }
        out
    }
}

/// An integer constant of `ty` as LLVM prints it: `true`/`false` for
/// `i1`, else signed.
pub fn int_text(ty: Ty, value: u64) -> String {
    if ty.bits == 1 {
        return if value & 1 == 1 { "true".into() } else { "false".into() };
    }
    let shift = 64 - ty.bits;
    (((value << shift) as i64) >> shift).to_string()
}

/// A constant of `ty` whose image is `lo` (and `hi` above 64 bits) as
/// LLVM's text writes it: hexadecimal for the floating-point types (a
/// `float` as the `double` of the same value).
pub fn const_text(ty: Ty, lo: u64, hi: u64) -> String {
    match ty.kind {
        Kind::Fp("half") => format!("0xH{:04X}", lo as u16),
        Kind::Fp("bfloat") => format!("0xR{:04X}", lo as u16),
        Kind::Fp("float") => {
            format!("0x{:016X}", f64::from(f32::from_bits(lo as u32)).to_bits())
        }
        Kind::Fp("double") => format!("0x{lo:016X}"),
        Kind::Fp("fp128") => format!("0xL{lo:016X}{hi:016X}"),
        Kind::Fp(_) => panic!("no constants of {}", ty.name()),
        Kind::Ptr => "null".into(),
        Kind::Int => int_text(ty, lo),
    }
}

/// `c"\00\01..."`.
pub fn bytes(data: &[u8]) -> String {
    let mut out = String::from("c\"");
    for b in data {
        write!(out, "\\{b:02X}").unwrap();
    }
    out.push('"');
    out
}

/// A memory region the function may access.
struct Region {
    /// The base address operand.
    base: String,
    size: u64,
    align: u64,
    /// `None` for a byte not written yet.
    bytes: Vec<Option<u8>>,
    /// A variable: also addressable by a constant expression.
    global: bool,
}

/// A value available to be stored.
struct Avail {
    ty: Ty,
    name: String,
    /// The image, low 16 bytes.
    image: [u8; 16],
    uses: u32,
}

struct Gen<'r> {
    rng: &'r mut Rng,
    params: Vec<Param>,
    entry: Vec<String>,
    body: Vec<String>,
    regions: Vec<Region>,
    values: Vec<Avail>,
    next: u32,
    allocas: u32,
}

/// A `float` or `double` image that is no NaN (a `float` constant is
/// written as a `double`), with the edges among them.
fn fp_value(rng: &mut Rng, bytes: u32) -> u64 {
    let edges32 = [0u64, 0x8000_0000, 0x3f80_0000, 0x7f80_0000, 0xff80_0000, 1, 0x7f7f_ffff];
    let edges64 =
        [0u64, 1 << 63, 0x3ff0_0000_0000_0000, 0x7ff0_0000_0000_0000, 1, 0x7fef_ffff_ffff_ffff];
    if bytes == 4 {
        if rng.chance(1, 3) {
            return rng.pick(&edges32);
        }
        let v = rng.next() as u32;
        if f32::from_bits(v).is_nan() { u64::from(v & 0xbfff_ffff) } else { u64::from(v) }
    } else {
        if rng.chance(1, 3) {
            return rng.pick(&edges64);
        }
        let v = rng.next();
        if f64::from_bits(v).is_nan() { v & 0xbfff_ffff_ffff_ffff } else { v }
    }
}

impl Gen<'_> {
    fn temp(&mut self, prefix: &str) -> String {
        self.next += 1;
        format!("%{prefix}{}", self.next)
    }

    fn param(&mut self, ty: Ty, value: u64) -> String {
        let garbage = self.rng.next();
        self.params.push(Param { ty, value: value & ty.mask(), garbage });
        format!("%a{}", self.params.len())
    }

    /// A value of `ty` at one of the edges, or random.
    fn int_value(&mut self, ty: Ty) -> u64 {
        let m = ty.mask();
        let v = match self.rng.below(7) {
            0 => 0,
            1 => 1,
            2 => m,
            3 => m >> 1,
            4 => (m >> 1).wrapping_add(1),
            _ => self.rng.next(),
        };
        v & m
    }

    fn width(&mut self) -> Ty {
        match self.rng.below(16) {
            0 => Ty::PTR,
            1..=4 => {
                let (name, bytes) = self.rng.pick(&FPRS);
                Ty::fp(name, bytes)
            }
            _ => Ty::int(self.rng.pick(&WIDTHS)),
        }
    }

    /// An alignment for an access of `ty`: 1, the natural one, or between.
    fn align_for(&mut self, ty: Ty) -> u64 {
        let natural = ty.natural_align();
        match self.rng.below(3) {
            0 => 1,
            1 => natural,
            _ => 1 << self.rng.below(u64::from(natural.trailing_zeros()) + 1),
        }
    }

    /// A location of `size` bytes aligned to `align` in a region, with
    /// `ok` true of its bytes: `(region, offset)`.
    fn location(
        &mut self,
        size: u64,
        align: u64,
        ok: impl Fn(&[Option<u8>]) -> bool,
    ) -> Option<(usize, u64)> {
        for _ in 0..12 {
            let candidates: Vec<usize> = (0..self.regions.len())
                .filter(|&r| self.regions[r].size >= size && self.regions[r].align >= align)
                .collect();
            if candidates.is_empty() {
                return None;
            }
            let r = self.rng.pick(&candidates);
            let slots = (self.regions[r].size - size) / align;
            let k = if self.rng.chance(1, 3) { slots } else { self.rng.below(slots + 1) };
            let off = k * align;
            if ok(&self.regions[r].bytes[off as usize..(off + size) as usize]) {
                return Some((r, off));
            }
        }
        None
    }

    /// An index parameter of some width whose value is `v`, if `v` fits.
    fn index(&mut self, v: i64) -> Option<(String, u32)> {
        let w = self.rng.pick(&INDEX_WIDTHS);
        let fits = w == 64 || (-(1i64 << (w - 1))..(1i64 << (w - 1))).contains(&v);
        if !fits {
            return None;
        }
        let name = self.param(Ty::int(w), v as u64);
        Some((name, w))
    }

    /// `getelementptr i8, ptr <base>, i64 <off>` into a new temporary.
    fn const_gep(&mut self, base: &str, off: i64) -> String {
        let t = self.temp("p");
        self.body.push(format!("{t} = getelementptr i8, ptr {base}, i64 {off}"));
        t
    }

    /// The address of `offset` in region `r` as an operand, by one of the
    /// forms `ptradd` takes; sometimes with an instruction between it and
    /// its use, so that it is not fused.
    #[allow(clippy::many_single_char_names)]
    fn address(&mut self, r: usize, offset: u64) -> String {
        let base = self.regions[r].base.clone();
        let off = offset as i64;
        let form = self.rng.below(8);
        let addr = match form {
            0 if off == 0 => return base,
            1 if self.regions[r].global => {
                return format!("getelementptr (i8, ptr {base}, i64 {off})");
            }
            2 => {
                // One variable index, after a constant step when the
                // offset is not a multiple of the element.
                let (elem, size) = self.rng.pick(&ELEMS);
                let size = size as i64;
                let (v, rem) = if off % size == 0 && self.rng.chance(1, 2) {
                    (off / size, 0)
                } else {
                    let v = self.rng.small(3);
                    (v, off - v * size)
                };
                let Some((index, w)) = self.index(v) else { return self.const_gep(&base, off) };
                let from = if rem == 0 { base } else { self.const_gep(&base, rem) };
                let t = self.temp("p");
                self.body.push(format!("{t} = getelementptr {elem}, ptr {from}, i{w} {index}"));
                t
            }
            3 => {
                // A constant and a variable index in one `ptradd`.
                let (mut elem, mut size) = self.rng.pick(&ELEMS);
                let v = self.rng.small(4);
                if (off - v * size as i64) % size as i64 != 0 {
                    (elem, size) = ("i8", 1);
                }
                let c = (off - v * size as i64) / size as i64;
                let Some((index, w)) = self.index(v) else { return self.const_gep(&base, off) };
                let t = self.temp("p");
                self.body.push(format!(
                    "{t} = getelementptr [1 x {elem}], ptr {base}, i64 {c}, i{w} {index}"
                ));
                t
            }
            4 => {
                // Two variable indices.
                let (elem, size) = self.rng.pick(&ELEMS);
                let rows = self.rng.pick(&[2i64, 3, 5, 4]);
                let size = size as i64;
                let vi = self.rng.small(2);
                let rem = off - vi * rows * size;
                if rem % size != 0 {
                    return self.const_gep(&base, off);
                }
                let (Some((i, wi)), Some((j, wj))) = (self.index(vi), self.index(rem / size))
                else {
                    return self.const_gep(&base, off);
                };
                let t = self.temp("p");
                self.body.push(format!(
                    "{t} = getelementptr [{rows} x {elem}], ptr {base}, i{wi} {i}, i{wj} {j}"
                ));
                t
            }
            5 => {
                // A chain: the second's base is the first's result.
                let first = self.rng.small(16);
                let step = self.const_gep(&base, first);
                self.const_gep(&step, off - first)
            }
            6 => {
                // Three variable indices, the last of them a byte step.
                let (elem, size) = self.rng.pick(&ELEMS);
                let size = size as i64;
                let (a, b) = (self.rng.small(2), self.rng.small(2));
                let rest = off - a * 3 * size - b * size;
                let (Some((i, wi)), Some((j, wj)), Some((k, wk))) =
                    (self.index(a), self.index(b), self.index(rest))
                else {
                    return self.const_gep(&base, off);
                };
                let t = self.temp("p");
                self.body.push(format!(
                    "{t} = getelementptr [3 x {elem}], ptr {base}, i{wi} {i}, i{wj} {j}"
                ));
                let u = self.temp("p");
                self.body.push(format!("{u} = getelementptr i8, ptr {t}, i{wk} {k}"));
                u
            }
            _ => {
                if off == 0 {
                    return base;
                }
                self.const_gep(&base, off)
            }
        };
        if self.rng.chance(1, 3) {
            // Not the next instruction: the `ptradd` is computed by itself.
            self.body.push("store i8 0, ptr %junk".into());
        }
        addr
    }

    fn store(&mut self) {
        let ty = self.width();
        let same: Vec<usize> =
            (0..self.values.len()).filter(|&v| self.values[v].ty == ty).collect();
        let fp_const = matches!(ty.kind, Kind::Fp(n) if !n.starts_with('<'));
        let fp_param = matches!(ty.kind, Kind::Fp("float" | "double"));
        let (text, image) = if ty.is_ptr()
            || (ty.is_fp() && !fp_const)
            || (!same.is_empty() && self.rng.chance(1, 2))
        {
            if same.is_empty() {
                return;
            }
            let v = self.rng.pick(&same);
            self.values[v].uses += 1;
            (self.values[v].name.clone(), self.values[v].image)
        } else if ty.is_fp() {
            let bytes = ty.store() as u32;
            let (lo, hi) = match bytes {
                4 | 8 => (fp_value(self.rng, bytes), 0),
                2 => (self.rng.next() & 0xffff, 0),
                _ => (self.rng.next(), self.rng.next()),
            };
            let (lo, hi) = if self.rng.chance(1, 6) { (0, 0) } else { (lo, hi) };
            let text = if fp_param && self.rng.chance(1, 2) {
                self.param(ty, lo)
            } else if lo == 0 && hi == 0 && self.rng.chance(1, 2) {
                "zeroinitializer".into()
            } else {
                const_text(ty, lo, hi)
            };
            let mut image = [0u8; 16];
            image[..8].copy_from_slice(&lo.to_le_bytes());
            image[8..].copy_from_slice(&hi.to_le_bytes());
            (text, image)
        } else {
            let value = self.int_value(ty);
            let text =
                if self.rng.chance(2, 3) { self.param(ty, value) } else { int_text(ty, value) };
            let mut image = [0u8; 16];
            image[..8].copy_from_slice(&(value & ty.mask()).to_le_bytes());
            (text, image)
        };
        let align = self.align_for(ty);
        let Some((r, off)) = self.location(ty.store(), align, |_| true) else { return };
        let addr = self.address(r, off);
        let volatile = if self.rng.chance(1, 4) { " volatile" } else { "" };
        self.body.push(format!("store{volatile} {} {text}, ptr {addr}, align {align}", ty.name()));
        let at = off as usize;
        for (k, byte) in image.iter().take(ty.store() as usize).enumerate() {
            self.regions[r].bytes[at + k] = Some(*byte);
        }
    }

    fn load(&mut self) {
        let ty = self.width();
        let align = self.align_for(ty);
        let spare = (ty.store() * 8) as u32 - ty.bits;
        let ok = |bytes: &[Option<u8>]| {
            bytes.iter().all(Option::is_some)
                && bytes
                    .last()
                    .copied()
                    .flatten()
                    .is_some_and(|top| spare == 0 || u32::from(top) >> (8 - spare) == 0)
        };
        let Some((r, off)) = self.location(ty.store(), align, ok) else { return };
        let mut image = [0u8; 16];
        for (k, byte) in image.iter_mut().take(ty.store() as usize).enumerate() {
            *byte = self.regions[r].bytes[off as usize + k].expect("checked");
        }
        let addr = self.address(r, off);
        let volatile = if self.rng.chance(1, 4) { " volatile" } else { "" };
        let name = self.temp("v");
        self.body.push(format!("{name} = load{volatile} {}, ptr {addr}, align {align}", ty.name()));
        self.values.push(Avail { ty, name, image, uses: 0 });
    }

    fn alloca(&mut self) {
        let (elem, size) = self.rng.pick(&ELEMS);
        let align = self.rng.pick(&[1u64, 2, 4, 8, 16, 32, 64, 128]);
        let w = self.rng.pick(&INDEX_WIDTHS);
        let max = if w == 1 { 1 } else { 9 };
        let count = self.rng.below(max + 1);
        let name = format!("%d{}", self.allocas);
        self.allocas += 1;
        let count_text =
            if self.rng.chance(1, 4) { format!("{count}") } else { self.param(Ty::int(w), count) };
        self.body.push(format!("{name} = alloca {elem}, i{w} {count_text}, align {align}"));
        let bytes = count * size;
        self.regions.push(Region {
            base: name,
            size: bytes,
            align,
            bytes: vec![None; bytes as usize],
            global: false,
        });
    }
}

/// The function `name` of `seed`.
pub fn generate(name: &str, seed: u64) -> Func {
    let mut rng = Rng(seed);
    let buf: Vec<u8> = (0..BUF).map(|_| rng.next() as u8).collect();
    let mut g = Gen {
        rng: &mut rng,
        params: Vec::new(),
        entry: vec!["%junk = alloca i8, align 1".into()],
        body: Vec::new(),
        regions: vec![Region {
            base: "%buf".into(),
            size: BUF,
            align: 16,
            bytes: buf.iter().copied().map(Some).collect(),
            global: false,
        }],
        values: Vec::new(),
        next: 0,
        allocas: 0,
    };
    let mut globals = Vec::new();
    for k in 0..g.rng.below(3) {
        let init: Vec<u8> = (0..GLOBAL).map(|_| g.rng.next() as u8).collect();
        let gname = format!("{name}_g{k}");
        g.regions.push(Region {
            base: format!("@{gname}"),
            size: GLOBAL,
            align: 16,
            bytes: init.iter().copied().map(Some).collect(),
            global: true,
        });
        globals.push(Global { name: gname, init, dso_local: g.rng.chance(1, 2) });
    }
    for k in 0..g.rng.below(4) {
        let size = if g.rng.chance(1, 20) {
            g.rng.pick(&[0u64, 40_000, 70_000])
        } else {
            g.rng.pick(&[1u64, 3, 8, 16, 24, 64, 100])
        };
        let align = g.rng.pick(&[1u64, 2, 4, 8, 16, 32, 64]);
        g.entry.push(format!("%s{k} = alloca [{size} x i8], align {align}"));
        g.regions.push(Region {
            base: format!("%s{k}"),
            size,
            align,
            bytes: vec![None; size as usize],
            global: false,
        });
    }
    let ops = 3 + g.rng.below(18);
    for _ in 0..ops {
        match g.rng.below(20) {
            0 | 1 => g.alloca(),
            2..=10 => g.store(),
            _ => g.load(),
        }
    }
    // Every load is observed: stored into `%buf`, or returned.
    let ret = g.values.iter().position(|v| v.ty.kind == Kind::Int);
    for v in 0..g.values.len() {
        if g.values[v].uses == 0 && Some(v) != ret {
            let ty = g.values[v].ty;
            let off = g.rng.below(BUF - ty.store() + 1);
            let addr = g.address(0, off);
            let text = g.values[v].name.clone();
            g.body.push(format!("store {} {text}, ptr {addr}, align 1", ty.name()));
        }
    }
    let (ret_ty, ret_text) = match ret {
        Some(v) => (g.values[v].ty, g.values[v].name.clone()),
        None => (Ty::int(64), "7".into()),
    };
    let mut text = format!("define {} @{name}(ptr %buf", ret_ty.name());
    for (k, p) in g.params.iter().enumerate() {
        write!(text, ", {} %a{}", p.ty.name(), k + 1).unwrap();
    }
    text.push_str(") {\nentry:\n");
    for line in &g.entry {
        writeln!(text, "  {line}").unwrap();
    }
    text.push_str("  br label %body\nbody:\n");
    for line in &g.body {
        writeln!(text, "  {line}").unwrap();
    }
    writeln!(text, "  ret {} {ret_text}\n}}", ret_ty.name()).unwrap();
    Func { name: name.into(), ret: ret_ty, params: g.params, text, globals, buf }
}
