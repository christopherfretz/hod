//! Constants for the differential tests, each produced twice from one
//! description: as LLVM text for `llc`, and as a `ConstId` built through
//! `hod_ir`'s constructors and folds.

use std::fmt::Write as _;

use hod_ir::{ConstId, ConstOp, ConstPool, SymId, TypeId, TypeTable};

use super::{Config, Rng, Ty, random_type};

/// The symbols every test module declares besides its test globals:
/// `@a` and `@b` are defined data, `@x` is external data, `@f` is a defined
/// function and `@h` an external one.
pub const PRELUDE: &str = "\
@a = global i32 7
@b = global [4 x i32] [i32 1, i32 2, i32 3, i32 4]
@x = external global i32
declare void @h()
define void @f() {
  ret void
}
";

/// The tables a test builds constants in, and the names of its symbols.
pub struct Ctx {
    pub types: TypeTable,
    pub consts: ConstPool,
    pub rng: Rng,
    /// LLVM's major version, for syntax only some versions accept.
    pub llvm: u32,
    names: Vec<String>,
    /// The global being defined: `{self}` in a case's text.
    pub this: SymId,
}

impl Ctx {
    pub fn new(config: &Config, seed: u64) -> Ctx {
        Ctx {
            types: config.type_table(),
            consts: ConstPool::new(),
            rng: Rng::new(seed),
            llvm: config.llvm,
            names: Vec::new(),
            this: SymId::new(0),
        }
    }

    /// The id of the symbol called `name`, new on first use.
    pub fn sym(&mut self, name: &str) -> SymId {
        let at = self.names.iter().position(|known| known == name).unwrap_or_else(|| {
            self.names.push(name.to_string());
            self.names.len() - 1
        });
        SymId::new(at as u32)
    }

    pub fn name(&self, sym: SymId) -> &str {
        &self.names[sym.index()]
    }

    // ---- builders the hand-written cases use ----

    /// `@name` plus `addend` bytes.
    pub fn addr(&mut self, name: &str, addend: i64) -> ConstId {
        let sym = self.sym(name);
        self.consts.addr(sym, addend).unwrap()
    }

    /// The address of the global being defined.
    pub fn this_addr(&mut self) -> ConstId {
        self.consts.addr(self.this, 0).unwrap()
    }

    /// `dso_local_equivalent @name`.
    pub fn dso(&mut self, name: &str) -> ConstId {
        let sym = self.sym(name);
        self.consts.dso_local_equivalent(sym).unwrap()
    }

    /// The integer `value` of `bits` bits.
    pub fn lit(&mut self, bits: u32, value: i64) -> ConstId {
        let ty = self.types.int(bits).unwrap();
        self.consts.int_signed(&self.types, ty, value).unwrap()
    }

    pub fn cast(&mut self, op: ConstOp, value: ConstId, to: &Ty) -> ConstId {
        let to = to.intern(&mut self.types);
        self.consts.fold_cast(&self.types, op, value, to).unwrap()
    }

    /// `ptrtoint (ptr to i<bits>)`.
    pub fn ptr_to_int(&mut self, ptr: ConstId, bits: u32) -> ConstId {
        self.cast(ConstOp::PtrToInt, ptr, &Ty::Int(bits))
    }

    /// `inttoptr (value to ptr)`.
    pub fn int_to_ptr(&mut self, value: ConstId) -> ConstId {
        self.cast(ConstOp::IntToPtr, value, &Ty::Ptr(0))
    }

    pub fn trunc(&mut self, value: ConstId, bits: u32) -> ConstId {
        self.cast(ConstOp::Trunc, value, &Ty::Int(bits))
    }

    pub fn binary(&mut self, op: ConstOp, lhs: ConstId, rhs: ConstId) -> ConstId {
        self.consts.fold_binary(&self.types, op, lhs, rhs).unwrap()
    }

    pub fn add(&mut self, lhs: ConstId, rhs: ConstId) -> ConstId {
        self.binary(ConstOp::Add, lhs, rhs)
    }

    pub fn sub(&mut self, lhs: ConstId, rhs: ConstId) -> ConstId {
        self.binary(ConstOp::Sub, lhs, rhs)
    }

    /// `getelementptr (source, base, indices...)`, each index as (width,
    /// value).
    pub fn gep(&mut self, source: &Ty, base: ConstId, indices: &[(u32, i64)]) -> ConstId {
        let source = source.intern(&mut self.types);
        let indices: Vec<ConstId> =
            indices.iter().map(|&(bits, value)| self.lit(bits, value)).collect();
        self.consts.fold_gep(&mut self.types, source, base, &indices).unwrap()
    }

    pub fn aggregate(&mut self, ty: &Ty, elems: &[ConstId]) -> ConstId {
        let ty = ty.intern(&mut self.types);
        self.consts.aggregate(&self.types, ty, elems).unwrap()
    }

    // ---- random data ----

    /// A random integer of `bits` bits as canonical words, favouring the
    /// edge values.
    fn random_words(&mut self, bits: u32) -> Vec<u64> {
        let count = bits.div_ceil(64) as usize;
        let mut words: Vec<u64> = match self.rng.below(8) {
            0 => vec![0; count],
            1 => vec![u64::MAX; count],
            2 => {
                let mut words = vec![0; count];
                words[0] = 1;
                words
            }
            3 => {
                let mut words = vec![0; count];
                words[count - 1] = 1 << ((bits - 1) % 64);
                words
            }
            _ => (0..count).map(|_| self.rng.next_u64()).collect(),
        };
        if !bits.is_multiple_of(64) {
            words[count - 1] &= (1 << (bits % 64)) - 1;
        }
        words
    }

    /// A random constant of type `ty` with no symbolic addresses unless
    /// `relocs`: the value's text (without its type) and the constant.
    pub fn random_value(&mut self, ty: &Ty, relocs: bool) -> (String, ConstId) {
        let id = ty.intern(&mut self.types);
        match self.rng.below(100) {
            0..=3 => return ("undef".into(), self.consts.undef(&self.types, id).unwrap()),
            4..=6 => return ("poison".into(), self.consts.poison(&self.types, id).unwrap()),
            7..=12 => {
                return ("zeroinitializer".into(), self.consts.zero(&self.types, id).unwrap());
            }
            _ => {}
        }
        match ty {
            Ty::Int(bits) => {
                let words = self.random_words(*bits);
                let value = self.consts.int_from_words(&self.types, id, &words).unwrap();
                (hex_int(&words), value)
            }
            Ty::Ptr(0) => self.random_pointer(relocs),
            Ty::Ptr(_) => ("null".into(), self.consts.null(&self.types, id).unwrap()),
            Ty::Vector(elem, len) => {
                let uniform = self.rng.chance(25);
                let first = self.random_value(elem, relocs);
                let mut elems = vec![first];
                for _ in 1..*len {
                    let next =
                        if uniform { elems[0].clone() } else { self.random_value(elem, relocs) };
                    elems.push(next);
                }
                let text = list_text("<", ">", elem, &elems);
                let ids: Vec<ConstId> = elems.iter().map(|(_, id)| *id).collect();
                let value = if uniform && self.rng.chance(50) {
                    self.consts.splat(&self.types, id, ids[0]).unwrap()
                } else {
                    self.consts.aggregate(&self.types, id, &ids).unwrap()
                };
                (text, value)
            }
            Ty::Array(_, 0) => {
                ("zeroinitializer".into(), self.consts.zero(&self.types, id).unwrap())
            }
            Ty::Array(elem, len) if **elem == Ty::Int(8) && self.rng.chance(60) => {
                let data: Vec<u8> = (0..*len).map(|_| self.rng.next_u64() as u8).collect();
                let mut text = String::from("c\"");
                for byte in &data {
                    let _ = write!(text, "\\{byte:02X}");
                }
                text.push('"');
                let value = self.consts.bytes(&mut self.types, &data).unwrap();
                assert_eq!(self.consts.ty(value), id);
                (text, value)
            }
            Ty::Array(elem, len) => {
                let elems: Vec<_> = (0..*len).map(|_| self.random_value(elem, relocs)).collect();
                let ids: Vec<ConstId> = elems.iter().map(|(_, id)| *id).collect();
                let value = self.consts.aggregate(&self.types, id, &ids).unwrap();
                (list_text("[", "]", elem, &elems), value)
            }
            Ty::Struct(fields, packed) => {
                if fields.is_empty() {
                    return ("zeroinitializer".into(), self.consts.zero(&self.types, id).unwrap());
                }
                let elems: Vec<_> =
                    fields.iter().map(|field| self.random_value(field, relocs)).collect();
                let ids: Vec<ConstId> = elems.iter().map(|(_, id)| *id).collect();
                let mut text = String::from(if *packed { "<{" } else { "{" });
                for (at, (field, (value, _))) in fields.iter().zip(&elems).enumerate() {
                    let comma = if at == 0 { "" } else { "," };
                    let _ = write!(text, "{comma} {} {value}", field.text());
                }
                text.push_str(if *packed { " }>" } else { " }" });
                (text, self.consts.aggregate(&self.types, id, &ids).unwrap())
            }
            float => {
                let (text, bits) = self.random_float(float);
                (text, self.consts.float(&self.types, id, bits).unwrap())
            }
        }
    }

    fn random_pointer(&mut self, relocs: bool) -> (String, ConstId) {
        let name = *self.rng.pick(&["a", "b", "x", "f"]);
        match self.rng.below(if relocs { 5 } else { 2 }) {
            0 => ("null".into(), self.consts.null(&self.types, TypeId::PTR).unwrap()),
            1 => {
                let value = self.rng.next_u64() >> self.rng.below(64);
                let text = format!("inttoptr (i64 {value} to ptr)");
                (text, self.consts.addr_of_int(value).unwrap())
            }
            2 => (format!("@{name}"), self.addr(name, 0)),
            _ => {
                let offset = self.rng.below(4096) as i64 - 2048;
                let text = format!("getelementptr (i8, ptr @{name}, i64 {offset})");
                (text, self.addr(name, offset))
            }
        }
    }

    /// A random bit pattern of a floating-point type and its LLVM spelling.
    fn random_float(&mut self, ty: &Ty) -> (String, u128) {
        let (low, high) = (self.rng.next_u64(), self.rng.next_u64());
        match ty {
            Ty::Half => (format!("0xH{:04X}", low as u16), u128::from(low as u16)),
            Ty::BFloat => (format!("0xR{:04X}", low as u16), u128::from(low as u16)),
            Ty::Float => {
                // `float` is spelled as the double with the same value; a
                // NaN would not survive the conversion bit for bit.
                let single = f32::from_bits(low as u32);
                let single = if single.is_nan() { 1.5 } else { single };
                let text = format!("0x{:016X}", f64::from(single).to_bits());
                (text, u128::from(single.to_bits()))
            }
            Ty::Double => (format!("0x{low:016X}"), u128::from(low)),
            // Sixteen bits of sign and exponent, then the significand, whose
            // top bit is explicit: set exactly when the exponent is not
            // zero. LLVM reads any other pattern as a NaN.
            Ty::X86Fp80 => {
                let exponent = high as u16 & 0x7fff;
                let low = if exponent == 0 { low & (u64::MAX >> 1) } else { low | (1 << 63) };
                let bits = (u128::from(high as u16) << 64) | u128::from(low);
                (format!("0xK{bits:020X}"), bits)
            }
            // The low word first.
            Ty::Fp128 => {
                (format!("0xL{low:016X}{high:016X}"), (u128::from(high) << 64) | u128::from(low))
            }
            other => panic!("not a floating-point type: {other:?}"),
        }
    }

    // ---- random foldable expressions ----

    /// A random integer literal of `bits` bits: text and constant.
    fn literal(&mut self, bits: u32) -> (String, ConstId) {
        let ty = self.types.int(bits).unwrap();
        // A decimal literal must fit its type.
        if bits >= 8 && self.rng.chance(50) {
            let small = self.rng.below(65) as i64 - 32;
            (small.to_string(), self.consts.int_signed(&self.types, ty, small).unwrap())
        } else {
            let words = self.random_words(bits);
            (hex_int(&words), self.consts.int_from_words(&self.types, ty, &words).unwrap())
        }
    }

    /// An integer expression of literals only, which LLVM folds while
    /// parsing: any width.
    fn literal_expr(&mut self, bits: u32, depth: u32) -> (String, ConstId) {
        if depth == 0 || self.rng.chance(30) {
            return self.literal(bits);
        }
        let ty = format!("i{bits}");
        // `mul` is a constant expression only up to LLVM 19.
        let choices: u64 = if self.llvm < 20 { 5 } else { 4 };
        match self.rng.below(choices) {
            0 => {
                let wider = bits + 1 + self.rng.below(70) as u32;
                let (text, value) = self.literal_expr(wider, depth - 1);
                (format!("trunc (i{wider} {text} to {ty})"), self.trunc(value, bits))
            }
            choice => {
                let (op, name) = match choice {
                    1 => (ConstOp::Add, "add"),
                    2 => (ConstOp::Sub, "sub"),
                    3 => (ConstOp::Xor, "xor"),
                    _ => (ConstOp::Mul, "mul"),
                };
                let (left, lhs) = self.literal_expr(bits, depth - 1);
                let (right, rhs) = self.literal_expr(bits, depth - 1);
                (format!("{name} ({ty} {left}, {ty} {right})"), self.binary(op, lhs, rhs))
            }
        }
    }

    /// A random `getelementptr` over a random type from `base`.
    fn random_gep(&mut self, base_text: &str, base: ConstId) -> (String, ConstId) {
        let source = random_type(&mut self.rng, 2);
        let first_bits = *self.rng.pick(&[8, 16, 32, 64]);
        let first = self.rng.below(9) as i64 - 3;
        let mut text =
            format!("getelementptr ({}, ptr {base_text}, i{first_bits} {first}", source.text());
        let mut indices = vec![(first_bits, first)];
        let mut current = &source;
        while self.rng.chance(75) {
            let (bits, index, next) = match current {
                Ty::Struct(fields, _) if !fields.is_empty() => {
                    let field = self.rng.below(fields.len() as u64);
                    (32, field as i64, &fields[field as usize])
                }
                Ty::Array(elem, len) => (64, self.rng.below(len + 3) as i64 - 1, &**elem),
                // Only vectors whose elements are whole bytes have element
                // addresses.
                Ty::Vector(elem, len)
                    if elem.scalar_bits().is_some_and(|bits| bits.is_multiple_of(8)) =>
                {
                    (64, self.rng.below(u64::from(*len)) as i64, &**elem)
                }
                _ => break,
            };
            let _ = write!(text, ", i{bits} {index}");
            indices.push((bits, index));
            current = next;
        }
        text.push(')');
        (text, self.gep(&source, base, &indices))
    }

    /// A random pointer expression that folds to `symbol + addend`
    /// (`symbolic`) or to an absolute address.
    pub fn pointer_expr(&mut self, symbolic: bool, depth: u32) -> (String, ConstId) {
        if depth == 0 || self.rng.chance(25) {
            return if symbolic {
                let name = *self.rng.pick(&["a", "b", "x", "f"]);
                (format!("@{name}"), self.addr(name, 0))
            } else {
                ("null".into(), self.consts.null(&self.types, TypeId::PTR).unwrap())
            };
        }
        if self.rng.chance(65) {
            let (base_text, base) = self.pointer_expr(symbolic, depth - 1);
            self.random_gep(&base_text, base)
        } else {
            let form = if symbolic { Form::Symbol } else { Form::Number };
            let (text, value) = self.int_expr(64, form, depth - 1);
            (format!("inttoptr (i64 {text} to ptr)"), self.int_to_ptr(value))
        }
    }

    /// A random integer expression of `bits` bits (8, 16, 32 or 64) that
    /// folds to the given form.
    pub fn int_expr(&mut self, bits: u32, form: Form, depth: u32) -> (String, ConstId) {
        let ty = format!("i{bits}");
        let wider: Vec<u32> = [16, 32, 64].into_iter().filter(|&w| w > bits).collect();
        if depth > 0 && !wider.is_empty() && self.rng.chance(20) {
            let from = *self.rng.pick(&wider);
            let (text, value) = self.int_expr(from, form, depth - 1);
            return (format!("trunc (i{from} {text} to {ty})"), self.trunc(value, bits));
        }
        // An operand that folds to a number, and its text with its type.
        let number = |ctx: &mut Ctx| {
            let (text, value) = ctx.int_expr(bits, Form::Number, depth.saturating_sub(1));
            (format!("{ty} {text}"), value)
        };
        match form {
            Form::Number => {
                if depth == 0 || self.rng.chance(35) {
                    return self.literal_expr(bits, depth.min(2));
                }
                match self.rng.below(3) {
                    0 => {
                        let (text, ptr) = self.pointer_expr(false, depth - 1);
                        let (keyword, op) = self.ptrtoint_keyword(bits);
                        (
                            format!("{keyword} (ptr {text} to {ty})"),
                            self.cast(op, ptr, &Ty::Int(bits)),
                        )
                    }
                    choice => {
                        let ((left, lhs), (right, rhs)) = (number(self), number(self));
                        if choice == 1 {
                            (format!("add ({left}, {right})"), self.add(lhs, rhs))
                        } else {
                            (format!("sub ({left}, {right})"), self.sub(lhs, rhs))
                        }
                    }
                }
            }
            Form::Symbol => {
                if depth == 0 || self.rng.chance(40) {
                    let (text, ptr) = self.pointer_expr(true, depth.saturating_sub(1));
                    let (keyword, op) = self.ptrtoint_keyword(bits);
                    let value = self.cast(op, ptr, &Ty::Int(bits));
                    return (format!("{keyword} (ptr {text} to {ty})"), value);
                }
                let (symbol, lhs) = self.int_expr(bits, Form::Symbol, depth - 1);
                let (right, rhs) = number(self);
                match self.rng.below(3) {
                    0 => (format!("add ({ty} {symbol}, {right})"), self.add(lhs, rhs)),
                    1 => (format!("add ({right}, {ty} {symbol})"), self.add(rhs, lhs)),
                    _ => (format!("sub ({ty} {symbol}, {right})"), self.sub(lhs, rhs)),
                }
            }
            Form::Difference => {
                if depth == 0 || self.rng.chance(40) {
                    let (left, lhs) = self.int_expr(bits, Form::Symbol, depth.saturating_sub(1));
                    let (right, rhs) = self.int_expr(bits, Form::Symbol, depth.saturating_sub(1));
                    return (format!("sub ({ty} {left}, {ty} {right})"), self.sub(lhs, rhs));
                }
                let (difference, lhs) = self.int_expr(bits, Form::Difference, depth - 1);
                let (right, rhs) = number(self);
                match self.rng.below(3) {
                    0 => (format!("add ({ty} {difference}, {right})"), self.add(lhs, rhs)),
                    1 => (format!("add ({right}, {ty} {difference})"), self.add(rhs, lhs)),
                    _ => (format!("sub ({ty} {difference}, {right})"), self.sub(lhs, rhs)),
                }
            }
        }
    }

    /// `ptrtoint`, or sometimes `ptrtoaddr` where LLVM has it and the two
    /// agree (the full pointer width).
    fn ptrtoint_keyword(&mut self, bits: u32) -> (&'static str, ConstOp) {
        if self.llvm >= 22 && bits == 64 && self.rng.chance(20) {
            ("ptrtoaddr", ConstOp::PtrToAddr)
        } else {
            ("ptrtoint", ConstOp::PtrToInt)
        }
    }

    /// A random literal-only expression of any width.
    pub fn wide_literal_expr(&mut self) -> (u32, String, ConstId) {
        let bits = if self.rng.chance(50) {
            *self.rng.pick(&[1, 7, 8, 24, 33, 64, 65, 128, 129, 200])
        } else {
            self.rng.below(200) as u32 + 1
        };
        let (text, value) = self.literal_expr(bits, 3);
        (bits, text, value)
    }
}

/// What a random integer expression folds to.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Form {
    /// An integer.
    Number,
    /// `symbol + addend`.
    Symbol,
    /// `symbol - symbol + addend`.
    Difference,
}

/// `u0x...`: an unsigned hexadecimal integer literal.
pub fn hex_int(words: &[u64]) -> String {
    let mut text = String::from("u0x");
    for (at, word) in words.iter().rev().enumerate() {
        if at == 0 {
            let _ = write!(text, "{word:X}");
        } else {
            let _ = write!(text, "{word:016X}");
        }
    }
    text
}

fn list_text(open: &str, close: &str, elem: &Ty, elems: &[(String, ConstId)]) -> String {
    let mut text = String::from(open);
    for (at, (value, _)) in elems.iter().enumerate() {
        let comma = if at == 0 { "" } else { ", " };
        let _ = write!(text, "{comma}{} {value}", elem.text());
    }
    text.push_str(close);
    text
}

/// A hand-written initializer with an address in it.
pub struct Case {
    pub ty: Ty,
    /// The value's LLVM text; `{self}` is the global being defined.
    pub text: &'static str,
    pub build: fn(&mut Ctx) -> ConstId,
    /// Whether an object file can hold it on every target. One-byte
    /// relocations and a lone `dso_local_equivalent` cannot be assembled
    /// everywhere; the assembly-level test still covers them.
    pub in_object: bool,
    /// Whether `llc`'s relocation addend is the one hod computes. It is not
    /// when the addend does not fit the field: hod reduces it to the
    /// field's width, `llc` leaves it to the linker to truncate.
    pub exact_addend: bool,
}

fn case(ty: Ty, text: &'static str, build: fn(&mut Ctx) -> ConstId) -> Case {
    Case { ty, text, build, in_object: true, exact_addend: true }
}

fn vector(elem: Ty, len: u32) -> Ty {
    Ty::Vector(Box::new(elem), len)
}

/// A vector of `len` elements, each the integer `value`.
fn int_vector(c: &mut Ctx, bits: u32, len: u32, value: i64) -> ConstId {
    let elem = c.lit(bits, value);
    c.aggregate(&vector(Ty::Int(bits), len), &vec![elem; len as usize])
}

/// Initializers that show what LLVM puts in the tail padding of a vector:
/// zero, except after a vector of plain data whose bytes are all the same.
pub fn padding_cases() -> Vec<Case> {
    let int = Ty::Int;
    vec![
        case(vector(int(8), 3), "<i8 7, i8 7, i8 7>", |c| int_vector(c, 8, 3, 7)),
        case(vector(int(8), 3), "<i8 7, i8 7, i8 8>", |c| {
            let elems = [c.lit(8, 7), c.lit(8, 7), c.lit(8, 8)];
            c.aggregate(&vector(Ty::Int(8), 3), &elems)
        }),
        case(vector(int(8), 3), "<i8 7, i8 undef, i8 7>", |c| {
            let i8_ty = Ty::Int(8).intern(&mut c.types);
            let elems = [c.lit(8, 7), c.consts.undef(&c.types, i8_ty).unwrap(), c.lit(8, 7)];
            c.aggregate(&vector(Ty::Int(8), 3), &elems)
        }),
        case(vector(int(8), 17), "splat (i8 -1)", |c| {
            let (ty, elem) = (vector(Ty::Int(8), 17).intern(&mut c.types), c.lit(8, -1));
            c.consts.splat(&c.types, ty, elem).unwrap()
        }),
        case(vector(int(16), 3), "<i16 257, i16 257, i16 257>", |c| int_vector(c, 16, 3, 257)),
        case(vector(int(16), 3), "<i16 258, i16 258, i16 258>", |c| int_vector(c, 16, 3, 258)),
        case(
            vector(int(32), 5),
            "<i32 16843009, i32 16843009, i32 16843009, i32 16843009, i32 16843009>",
            |c| int_vector(c, 32, 5, 0x0101_0101),
        ),
        case(vector(int(64), 3), "<i64 -1, i64 -1, i64 -1>", |c| int_vector(c, 64, 3, -1)),
        // Not plain data: `i24` elements are emitted one by one.
        case(vector(int(24), 3), "<i24 65793, i24 65793, i24 65793>", |c| {
            int_vector(c, 24, 3, 0x01_0101)
        }),
        case(vector(Ty::Half, 3), "<half 0xH4040, half 0xH4040, half 0xH4040>", |c| {
            let elem = c.consts.float(&c.types, TypeId::HALF, 0x4040).unwrap();
            c.aggregate(&vector(Ty::Half, 3), &[elem; 3])
        }),
        case(
            vector(Ty::Float, 3),
            "<float 0x4008080800000000, float 0x4008080800000000, float 0x4008080800000000>",
            |c| {
                let elem = c.consts.float(&c.types, TypeId::FLOAT, 0x4040_4040).unwrap();
                c.aggregate(&vector(Ty::Float, 3), &[elem; 3])
            },
        ),
        case(
            vector(Ty::Double, 3),
            "<double 0x4040404040404040, double 0x4040404040404040, double 0x4040404040404040>",
            |c| {
                let elem = c.consts.float(&c.types, TypeId::DOUBLE, 0x4040_4040_4040_4040).unwrap();
                c.aggregate(&vector(Ty::Double, 3), &[elem; 3])
            },
        ),
        case(
            Ty::Array(Box::new(vector(int(8), 3)), 2),
            "[<3 x i8> <i8 9, i8 9, i8 9>, <3 x i8> <i8 9, i8 9, i8 9>]",
            |c| {
                let elem = int_vector(c, 8, 3, 9);
                c.aggregate(&Ty::Array(Box::new(vector(Ty::Int(8), 3)), 2), &[elem, elem])
            },
        ),
        case(
            Ty::Struct(vec![vector(int(8), 3), int(8), vector(int(16), 3)], false),
            "{ <3 x i8> <i8 1, i8 1, i8 1>, i8 0, <3 x i16> <i16 514, i16 514, i16 514> }",
            |c| {
                let fields = [int_vector(c, 8, 3, 1), c.lit(8, 0), int_vector(c, 16, 3, 514)];
                let ty = Ty::Struct(
                    vec![vector(Ty::Int(8), 3), Ty::Int(8), vector(Ty::Int(16), 3)],
                    false,
                );
                c.aggregate(&ty, &fields)
            },
        ),
        // Repeated bytes in types without tail padding of their own.
        case(Ty::Array(Box::new(int(24)), 2), "[i24 65793, i24 65793]", |c| {
            let elem = c.lit(24, 0x01_0101);
            c.aggregate(&Ty::Array(Box::new(Ty::Int(24)), 2), &[elem, elem])
        }),
        case(Ty::Array(Box::new(int(128)), 2), "[i128 -1, i128 -1]", |c| {
            let elem = c.lit(128, -1);
            c.aggregate(&Ty::Array(Box::new(Ty::Int(128)), 2), &[elem, elem])
        }),
    ]
}

fn relative_vtable() -> Ty {
    Ty::Struct(vec![Ty::Int(32), Ty::Int(32), Ty::Int(32)], false)
}

fn record() -> Ty {
    Ty::Struct(vec![Ty::Int(8), Ty::Int(64), Ty::Array(Box::new(Ty::Int(16)), 3)], false)
}

fn mixed() -> Ty {
    let pair = Ty::Array(Box::new(Ty::Ptr(0)), 2);
    Ty::Struct(vec![Ty::Int(8), Ty::Ptr(0), Ty::Int(32), pair], false)
}

/// The address forms of `spec/ir.md` §4.2, as initializers.
#[allow(clippy::too_many_lines)]
pub fn address_cases() -> Vec<Case> {
    let int = Ty::Int;
    let ptr = Ty::Ptr(0);
    let mut cases = vec![
        case(ptr.clone(), "@x", |c| c.addr("x", 0)),
        case(ptr.clone(), "@f", |c| c.addr("f", 0)),
        case(ptr.clone(), "getelementptr (i8, ptr @x, i64 -3)", |c| {
            let base = c.addr("x", 0);
            c.gep(&Ty::Int(8), base, &[(64, -3)])
        }),
        case(ptr.clone(), "getelementptr (i8, ptr @x, i64 1099511627776)", |c| {
            let base = c.addr("x", 0);
            c.gep(&Ty::Int(8), base, &[(64, 1 << 40)])
        }),
        case(ptr.clone(), "getelementptr ([4 x i32], ptr @b, i64 0, i64 2)", |c| {
            let base = c.addr("b", 0);
            c.gep(&Ty::Array(Box::new(Ty::Int(32)), 4), base, &[(64, 0), (64, 2)])
        }),
        case(
            ptr.clone(),
            "getelementptr ({ i8, i64, [3 x i16] }, ptr @x, i64 2, i32 2, i64 1)",
            |c| {
                let base = c.addr("x", 0);
                c.gep(&record(), base, &[(64, 2), (32, 2), (64, 1)])
            },
        ),
        case(
            ptr.clone(),
            "getelementptr (i16, ptr getelementptr ({ i8, i64, [3 x i16] }, ptr @x, i64 0, i32 1), i8 -1)",
            |c| {
                let base = c.addr("x", 0);
                let inner = c.gep(&record(), base, &[(64, 0), (32, 1)]);
                c.gep(&Ty::Int(16), inner, &[(8, -1)])
            },
        ),
        case(ptr.clone(), "getelementptr (i8, ptr null, i64 77)", |c| {
            let null = c.consts.null(&c.types, TypeId::PTR).unwrap();
            c.gep(&Ty::Int(8), null, &[(64, 77)])
        }),
        case(ptr.clone(), "inttoptr (i64 4660 to ptr)", |c| {
            let number = c.lit(64, 4660);
            c.int_to_ptr(number)
        }),
        case(ptr.clone(), "inttoptr (i64 ptrtoint (ptr @x to i64) to ptr)", |c| {
            let x = c.addr("x", 0);
            let number = c.ptr_to_int(x, 64);
            c.int_to_ptr(number)
        }),
        case(ptr.clone(), "inttoptr (i64 add (i64 ptrtoint (ptr @x to i64), i64 9) to ptr)", |c| {
            let x = c.addr("x", 0);
            let (number, nine) = (c.ptr_to_int(x, 64), c.lit(64, 9));
            let sum = c.add(number, nine);
            c.int_to_ptr(sum)
        }),
        case(ptr.clone(), "no_cfi @f", |c| {
            let f = c.sym("f");
            c.consts.no_cfi(f).unwrap()
        }),
        case(int(64), "ptrtoint (ptr @x to i64)", |c| {
            let x = c.addr("x", 0);
            c.ptr_to_int(x, 64)
        }),
        case(int(32), "ptrtoint (ptr @x to i32)", |c| {
            let x = c.addr("x", 0);
            c.ptr_to_int(x, 32)
        }),
        case(int(32), "trunc (i64 ptrtoint (ptr @x to i64) to i32)", |c| {
            let x = c.addr("x", 0);
            let wide = c.ptr_to_int(x, 64);
            c.trunc(wide, 32)
        }),
        case(int(16), "ptrtoint (ptr getelementptr (i8, ptr @x, i64 258) to i16)", |c| {
            let x = c.addr("x", 258);
            c.ptr_to_int(x, 16)
        }),
        case(int(64), "ptrtoint (ptr inttoptr (i64 4660 to ptr) to i64)", |c| c.lit(64, 4660)),
        case(int(64), "add (i64 ptrtoint (ptr @x to i64), i64 24)", |c| {
            let x = c.addr("x", 0);
            let (number, addend) = (c.ptr_to_int(x, 64), c.lit(64, 24));
            c.add(number, addend)
        }),
        case(int(64), "sub (i64 ptrtoint (ptr @x to i64), i64 4)", |c| {
            let x = c.addr("x", 0);
            let (number, addend) = (c.ptr_to_int(x, 64), c.lit(64, 4));
            c.sub(number, addend)
        }),
        case(
            int(64),
            "add (i64 100, i64 ptrtoint (ptr getelementptr (i32, ptr @x, i64 5) to i64))",
            |c| {
                let x = c.addr("x", 0);
                let element = c.gep(&Ty::Int(32), x, &[(64, 5)]);
                let (hundred, number) = (c.lit(64, 100), c.ptr_to_int(element, 64));
                c.add(hundred, number)
            },
        ),
        case(int(32), "sub (i32 ptrtoint (ptr @x to i32), i32 8)", |c| {
            let x = c.addr("x", 0);
            let (number, addend) = (c.ptr_to_int(x, 32), c.lit(32, 8));
            c.sub(number, addend)
        }),
        // Differences. `@a` and `@b` are in one section: a number.
        case(int(64), "sub (i64 ptrtoint (ptr @b to i64), i64 ptrtoint (ptr @a to i64))", |c| {
            let (b, a) = (c.addr("b", 0), c.addr("a", 0));
            let (lhs, rhs) = (c.ptr_to_int(b, 64), c.ptr_to_int(a, 64));
            c.sub(lhs, rhs)
        }),
        case(
            int(64),
            "add (i64 sub (i64 ptrtoint (ptr getelementptr (i32, ptr @b, i64 3) to i64), i64 ptrtoint (ptr @a to i64)), i64 7)",
            |c| {
                let (b, a) = (c.addr("b", 12), c.addr("a", 0));
                let (lhs, rhs) = (c.ptr_to_int(b, 64), c.ptr_to_int(a, 64));
                let (difference, seven) = (c.sub(lhs, rhs), c.lit(64, 7));
                c.add(difference, seven)
            },
        ),
        case(
            int(64),
            "add (i64 sub (i64 ptrtoint (ptr {self} to i64), i64 ptrtoint (ptr {self} to i64)), i64 12)",
            |c| {
                let this = c.this_addr();
                let number = c.ptr_to_int(this, 64);
                let (difference, twelve) = (c.sub(number, number), c.lit(64, 12));
                c.add(difference, twelve)
            },
        ),
        // The subtracted symbol is in the section of the field: PC-relative.
        case(
            int(64),
            "sub (i64 ptrtoint (ptr @f to i64), i64 ptrtoint (ptr {self} to i64))",
            |c| {
                let (f, this) = (c.addr("f", 0), c.this_addr());
                let (lhs, rhs) = (c.ptr_to_int(f, 64), c.ptr_to_int(this, 64));
                c.sub(lhs, rhs)
            },
        ),
        case(
            int(32),
            "trunc (i64 sub (i64 ptrtoint (ptr @f to i64), i64 ptrtoint (ptr {self} to i64)) to i32)",
            |c| {
                let (f, this) = (c.addr("f", 0), c.this_addr());
                let (lhs, rhs) = (c.ptr_to_int(f, 64), c.ptr_to_int(this, 64));
                let difference = c.sub(lhs, rhs);
                c.trunc(difference, 32)
            },
        ),
        case(
            int(16),
            "trunc (i64 sub (i64 ptrtoint (ptr @x to i64), i64 ptrtoint (ptr {self} to i64)) to i16)",
            |c| {
                let (x, this) = (c.addr("x", 0), c.this_addr());
                let (lhs, rhs) = (c.ptr_to_int(x, 64), c.ptr_to_int(this, 64));
                let difference = c.sub(lhs, rhs);
                c.trunc(difference, 16)
            },
        ),
        case(
            int(32),
            "trunc (i64 sub (i64 ptrtoint (ptr @x to i64), i64 ptrtoint (ptr @a to i64)) to i32)",
            |c| {
                let (x, a) = (c.addr("x", 0), c.addr("a", 0));
                let (lhs, rhs) = (c.ptr_to_int(x, 64), c.ptr_to_int(a, 64));
                let difference = c.sub(lhs, rhs);
                c.trunc(difference, 32)
            },
        ),
        case(
            int(32),
            "sub (i32 sub (i32 ptrtoint (ptr @x to i32), i32 ptrtoint (ptr {self} to i32)), i32 10)",
            |c| {
                let (x, this) = (c.addr("x", 0), c.this_addr());
                let (lhs, rhs) = (c.ptr_to_int(x, 32), c.ptr_to_int(this, 32));
                let (difference, ten) = (c.sub(lhs, rhs), c.lit(32, 10));
                c.sub(difference, ten)
            },
        ),
        // The relative-vtable shape, with and without an addend.
        case(
            relative_vtable(),
            "{ i32 0, \
             i32 trunc (i64 sub (i64 ptrtoint (ptr dso_local_equivalent @h to i64), i64 ptrtoint (ptr getelementptr ({ i32, i32, i32 }, ptr {self}, i32 0, i32 1) to i64)) to i32), \
             i32 trunc (i64 sub (i64 ptrtoint (ptr dso_local_equivalent @f to i64), i64 ptrtoint (ptr getelementptr ({ i32, i32, i32 }, ptr {self}, i32 0, i32 1) to i64)) to i32) }",
            |c| {
                let this = c.this_addr();
                let point = c.gep(&relative_vtable(), this, &[(32, 0), (32, 1)]);
                let point = c.ptr_to_int(point, 64);
                let mut entries = vec![c.lit(32, 0)];
                for name in ["h", "f"] {
                    let function = c.dso(name);
                    let function = c.ptr_to_int(function, 64);
                    let difference = c.sub(function, point);
                    entries.push(c.trunc(difference, 32));
                }
                c.aggregate(&relative_vtable(), &entries)
            },
        ),
        case(
            int(32),
            "trunc (i64 add (i64 sub (i64 ptrtoint (ptr dso_local_equivalent @h to i64), i64 ptrtoint (ptr {self} to i64)), i64 8) to i32)",
            |c| {
                let (function, this) = (c.dso("h"), c.this_addr());
                let (lhs, rhs) = (c.ptr_to_int(function, 64), c.ptr_to_int(this, 64));
                let (difference, eight) = (c.sub(lhs, rhs), c.lit(64, 8));
                let sum = c.add(difference, eight);
                c.trunc(sum, 32)
            },
        ),
        // Addresses inside aggregates and vectors.
        case(
            Ty::Array(Box::new(ptr.clone()), 2),
            "[ptr @x, ptr getelementptr (i8, ptr @a, i64 2)]",
            |c| {
                let elems = [c.addr("x", 0), c.addr("a", 2)];
                c.aggregate(&Ty::Array(Box::new(Ty::Ptr(0)), 2), &elems)
            },
        ),
        case(
            Ty::Vector(Box::new(ptr.clone()), 2),
            "<ptr @a, ptr getelementptr (i8, ptr @x, i64 -3)>",
            |c| {
                let elems = [c.addr("a", 0), c.addr("x", -3)];
                c.aggregate(&Ty::Vector(Box::new(Ty::Ptr(0)), 2), &elems)
            },
        ),
        case(
            mixed(),
            "{ i8 9, ptr @f, i32 ptrtoint (ptr @x to i32), [2 x ptr] [ptr null, ptr getelementptr (i8, ptr {self}, i64 1)] }",
            |c| {
                let (x, this) = (c.addr("x", 0), c.this_addr());
                let null = c.consts.null(&c.types, TypeId::PTR).unwrap();
                let inner = c.gep(&Ty::Int(8), this, &[(64, 1)]);
                let pair = c.aggregate(&Ty::Array(Box::new(Ty::Ptr(0)), 2), &[null, inner]);
                let fields = [c.lit(8, 9), c.addr("f", 0), c.ptr_to_int(x, 32), pair];
                c.aggregate(&mixed(), &fields)
            },
        ),
    ];
    // Addends that do not fit the field as `llc` writes them: 70000 in
    // sixteen bits (4464 for hod), and a negative literal, which `llc`
    // zero-extends to 4294967288 (-8 for hod).
    cases.extend(
        [
            case(int(16), "ptrtoint (ptr getelementptr (i8, ptr @x, i64 70000) to i16)", |c| {
                let x = c.addr("x", 70000);
                c.ptr_to_int(x, 16)
            }),
            case(int(32), "add (i32 ptrtoint (ptr @x to i32), i32 -8)", |c| {
                let x = c.addr("x", 0);
                let (number, addend) = (c.ptr_to_int(x, 32), c.lit(32, -8));
                c.add(number, addend)
            }),
        ]
        .map(|case| Case { exact_addend: false, ..case }),
    );
    // Forms an assembler cannot put in an object file on every target.
    cases.extend(
        [
            case(int(8), "ptrtoint (ptr @x to i8)", |c| {
                let x = c.addr("x", 0);
                c.ptr_to_int(x, 8)
            }),
            case(
                int(8),
                "trunc (i64 sub (i64 ptrtoint (ptr @x to i64), i64 ptrtoint (ptr @a to i64)) to i8)",
                |c| {
                    let (x, a) = (c.addr("x", 0), c.addr("a", 0));
                    let (lhs, rhs) = (c.ptr_to_int(x, 64), c.ptr_to_int(a, 64));
                    let difference = c.sub(lhs, rhs);
                    c.trunc(difference, 8)
                },
            ),
            case(ptr, "dso_local_equivalent @h", |c| c.dso("h")),
        ]
        .map(|case| Case { in_object: false, ..case }),
    );
    cases
}
