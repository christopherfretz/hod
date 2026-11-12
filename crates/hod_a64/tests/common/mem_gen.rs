//! A generator of straight-line functions of the memory group (WP-036)
//! for `tests/mem.rs`: what the memory group and the skeleton lower and
//! nothing else (arguments, constants, stack slots, globals, `alloca`,
//! `load`, `store`, `ptradd`, `br`, `ret`).
//!
//! A function is `iR @<name>(ptr %buf, <params>)`: `%buf` points at 256
//! bytes whose last byte ends a mapped page, every other parameter is an
//! integer of some width used as a stored value, an index or an `alloca`
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

/// A `Gpr`-class type: an integer of `bits` bits or a pointer.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Ty {
    pub bits: u32,
    pub ptr: bool,
}

impl Ty {
    pub const fn int(bits: u32) -> Ty {
        Ty { bits, ptr: false }
    }

    pub const PTR: Ty = Ty { bits: 64, ptr: true };

    pub fn name(self) -> String {
        if self.ptr { "ptr".into() } else { format!("i{}", self.bits) }
    }

    /// The store size.
    pub fn store(self) -> u64 {
        u64::from(self.bits.div_ceil(8))
    }

    /// The ABI alignment of the AArch64 data layout.
    pub fn natural_align(self) -> u64 {
        match self.store() {
            1 => 1,
            2 => 2,
            3 | 4 => 4,
            _ => 8,
        }
    }

    pub fn mask(self) -> u64 {
        if self.bits >= 64 { u64::MAX } else { (1 << self.bits) - 1 }
    }
}

/// Every width the memory group is tested at: each store size, the odd
/// widths around each, the encodings' boundaries.
pub const WIDTHS: [u32; 18] = [1, 7, 8, 9, 15, 16, 17, 24, 31, 32, 33, 40, 47, 48, 55, 56, 63, 64];

/// Index widths: `sext` from each, including the free `sxtw` of 32.
const INDEX_WIDTHS: [u32; 9] = [1, 7, 8, 16, 17, 32, 33, 63, 64];

/// Element types of `getelementptr`: `(text, alloc size)`.
const ELEMS: [(&str, u64); 10] = [
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
];

/// The size of `%buf`.
pub const BUF: u64 = 256;
/// The size of each global of a function.
pub const GLOBAL: u64 = 64;

/// A parameter after `%buf`.
#[derive(Clone, Copy, Debug)]
pub struct Param {
    pub ty: Ty,
    /// The value, the bits above the width clear.
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
    value: u64,
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
        if self.rng.chance(1, 12) { Ty::PTR } else { Ty::int(self.rng.pick(&WIDTHS)) }
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
        let form = self.rng.below(7);
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
                let rows = self.rng.pick(&[2i64, 3, 5]);
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
        let (text, value) = if ty.ptr || (!same.is_empty() && self.rng.chance(1, 2)) {
            if same.is_empty() {
                return;
            }
            let v = self.rng.pick(&same);
            self.values[v].uses += 1;
            (self.values[v].name.clone(), self.values[v].value)
        } else if self.rng.chance(2, 3) {
            let value = self.int_value(ty);
            (self.param(ty, value), value)
        } else {
            let value = self.int_value(ty);
            (int_text(ty, value), value)
        };
        let align = self.align_for(ty);
        let Some((r, off)) = self.location(ty.store(), align, |_| true) else { return };
        let addr = self.address(r, off);
        let volatile = if self.rng.chance(1, 4) { " volatile" } else { "" };
        self.body.push(format!("store{volatile} {} {text}, ptr {addr}, align {align}", ty.name()));
        let image = (value & ty.mask()).to_le_bytes();
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
        let mut image = [0u8; 8];
        for (k, byte) in image.iter_mut().take(ty.store() as usize).enumerate() {
            *byte = self.regions[r].bytes[off as usize + k].expect("checked");
        }
        let value = u64::from_le_bytes(image);
        let addr = self.address(r, off);
        let volatile = if self.rng.chance(1, 4) { " volatile" } else { "" };
        let name = self.temp("v");
        self.body.push(format!("{name} = load{volatile} {}, ptr {addr}, align {align}", ty.name()));
        self.values.push(Avail { ty, name, value, uses: 0 });
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
    let ret = g.values.iter().position(|v| !v.ty.ptr);
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
