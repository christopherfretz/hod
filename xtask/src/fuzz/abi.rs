//! The `abi` profile's generator: seeded C programs about the calling
//! convention and the layout of aggregates (`spec/harness.md` "Random
//! programs", "Profiles"), the class csmith's programs reach least (the
//! census of `--census`: no `long double`, no `__int128`, no variadic
//! function of their own, few aggregates passed or returned by value).
//!
//! A program has structs and unions of mixed members (every scalar type
//! of C on both targets, `long double`, `__int128`, pointers, arrays,
//! bit-fields, nested aggregates, packed ones and homogeneous ones of
//! floating-point members), globals of those types with static
//! initializers, and pairs of functions: `callee_<k>` takes many
//! parameters (scalars, aggregates by value, pointers to aggregates, a
//! variadic tail read with `va_arg`) and prints every scalar it received,
//! writes through its pointers and returns a scalar or an aggregate;
//! `caller_<k>` builds the arguments, calls, and prints what came back
//! and what was written. `main` calls each caller and prints the globals.
//! Built whole, the program checks hod against itself; split by `--mix`,
//! a caller and its callee are in different objects in half of the
//! cases, which is where a disagreement about the convention shows.
//!
//! Every program is defined: union members are read as they were written
//! (member 0), bit-field values fit their width, floating-point constants
//! are exact (hexadecimal), and the output names no address.

use std::fmt::Write as _;

/// A seeded generator (splitmix64): the same seed, the same program.
#[derive(Clone, Debug)]
pub struct Rng(u64);

impl Rng {
    pub fn new(seed: u64) -> Self {
        Self(seed ^ 0x6a09_e667_f3bc_c909)
    }

    pub fn next_u64(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9e37_79b9_7f4a_7c15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
        z ^ (z >> 31)
    }

    /// A number in `0..n` (`n` > 0).
    pub fn below(&mut self, n: usize) -> usize {
        usize::try_from(self.next_u64() % n.max(1) as u64).unwrap_or(0)
    }

    /// True with `percent` per cent.
    pub fn chance(&mut self, percent: usize) -> bool {
        self.below(100) < percent
    }

    pub fn pick<'a, T>(&mut self, items: &'a [T]) -> &'a T {
        &items[self.below(items.len())]
    }
}

/// The scalar types of the programs.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Scalar {
    Bool,
    SChar,
    UChar,
    Short,
    UShort,
    Int,
    UInt,
    Long,
    ULong,
    LongLong,
    ULongLong,
    Int128,
    UInt128,
    Float,
    Double,
    LongDouble,
    /// `int *`, pointing into `g_targets`.
    Ptr,
}

const SCALARS: [Scalar; 17] = [
    Scalar::Bool,
    Scalar::SChar,
    Scalar::UChar,
    Scalar::Short,
    Scalar::UShort,
    Scalar::Int,
    Scalar::UInt,
    Scalar::Long,
    Scalar::ULong,
    Scalar::LongLong,
    Scalar::ULongLong,
    Scalar::Int128,
    Scalar::UInt128,
    Scalar::Float,
    Scalar::Double,
    Scalar::LongDouble,
    Scalar::Ptr,
];

/// The variadic arguments' types: those the default argument promotions
/// leave alone.
const PROMOTED: [Scalar; 9] = [
    Scalar::Int,
    Scalar::UInt,
    Scalar::Long,
    Scalar::ULongLong,
    Scalar::Int128,
    Scalar::Double,
    Scalar::LongDouble,
    Scalar::Ptr,
    Scalar::LongLong,
];

impl Scalar {
    fn c(self) -> &'static str {
        match self {
            Self::Bool => "_Bool",
            Self::SChar => "signed char",
            Self::UChar => "unsigned char",
            Self::Short => "short",
            Self::UShort => "unsigned short",
            Self::Int => "int",
            Self::UInt => "unsigned",
            Self::Long => "long",
            Self::ULong => "unsigned long",
            Self::LongLong => "long long",
            Self::ULongLong => "unsigned long long",
            Self::Int128 => "__int128",
            Self::UInt128 => "unsigned __int128",
            Self::Float => "float",
            Self::Double => "double",
            Self::LongDouble => "long double",
            Self::Ptr => "int *",
        }
    }

    /// The bits of an integer type, and whether it is signed.
    fn integer(self) -> Option<(u32, bool)> {
        Some(match self {
            Self::Bool => (1, false),
            Self::SChar => (8, true),
            Self::UChar => (8, false),
            Self::Short => (16, true),
            Self::UShort => (16, false),
            Self::Int => (32, true),
            Self::UInt => (32, false),
            Self::Long | Self::LongLong => (64, true),
            Self::ULong | Self::ULongLong => (64, false),
            Self::Int128 => (128, true),
            Self::UInt128 => (128, false),
            Self::Float | Self::Double | Self::LongDouble | Self::Ptr => return None,
        })
    }

    /// A constant of the type, exact.
    fn literal(self, rng: &mut Rng) -> String {
        match self {
            Self::Float | Self::Double | Self::LongDouble => {
                // m * 2^-e: exact in every floating-point type here.
                let m = rng.next_u64() % (1 << 20);
                let e = rng.below(16);
                let sign = if rng.chance(30) { "-" } else { "" };
                let suffix = match self {
                    Self::Float => "f",
                    Self::LongDouble => "L",
                    _ => "",
                };
                format!("{sign}0x{m:x}p-{e}{suffix}")
            }
            Self::Ptr => format!("&g_targets[{}]", rng.below(4)),
            Self::Int128 | Self::UInt128 => {
                let (hi, lo) = (rng.next_u64(), rng.next_u64());
                format!("({})(((unsigned __int128)0x{hi:x}ULL << 64) | 0x{lo:x}ULL)", self.c())
            }
            _ => {
                let (bits, signed) = self.integer().unwrap_or((32, true));
                integer_literal(rng, bits, signed)
            }
        }
    }

    /// A statement that prints `expr` of this type.
    fn print(self, expr: &str) -> String {
        match self {
            Self::Float | Self::Double => format!("printf(\" %a\", (double)({expr}));"),
            Self::LongDouble => format!("printf(\" %La\", (long double)({expr}));"),
            Self::Ptr => format!("printf(\" p%d\", (int)(({expr}) - g_targets));"),
            Self::Int128 | Self::UInt128 => format!(
                "printf(\" %llx:%llx\", (unsigned long long)((unsigned __int128)({expr}) >> 64), \
                 (unsigned long long)({expr}));"
            ),
            _ if self.integer().is_some_and(|(_, signed)| signed) => {
                format!("printf(\" %lld\", (long long)({expr}));")
            }
            _ => format!("printf(\" %llu\", (unsigned long long)({expr}));"),
        }
    }
}

/// A decimal constant of an integer of `bits` (at most 64), with a suffix
/// that keeps its type wide enough.
fn integer_literal(rng: &mut Rng, bits: u32, signed: bool) -> String {
    let raw = rng.next_u64();
    // Small values are common in real code, and in ABI corner cases
    // (extension of narrow values) the sign matters: both kinds.
    let small = rng.chance(30);
    if bits == 1 {
        // A signed bit-field of one bit holds 0 and -1.
        return match (signed, raw & 1) {
            (_, 0) => "0".to_owned(),
            (true, _) => "-1".to_owned(),
            (false, _) => "1".to_owned(),
        };
    }
    if signed {
        // Within [-(2^(bits-1) - 1), 2^(bits-1) - 1]: never the minimum,
        // whose literal is not of the type.
        let max = (1u64 << (bits - 1)) - 1;
        let magnitude = if small { raw % 16 } else { raw } % (max + 1);
        let negative = rng.chance(50);
        let suffix = if bits > 32 { "LL" } else { "" };
        format!("{}{magnitude}{suffix}", if negative { "-" } else { "" })
    } else {
        let mask = if bits == 64 { u64::MAX } else { (1u64 << bits) - 1 };
        let value = if small { raw % 16 } else { raw } & mask;
        let suffix = if bits > 32 { "ULL" } else { "U" };
        format!("{value}{suffix}")
    }
}

/// A member of an aggregate.
#[derive(Clone, Debug)]
enum Member {
    Scalar(Scalar),
    Array(Scalar, usize),
    /// A bit-field: its declared type, signed or not, and width.
    Bits(Scalar, u32),
    /// An aggregate defined before this one.
    Nested(usize),
}

/// A struct or union.
#[derive(Clone, Debug)]
struct Aggregate {
    union: bool,
    members: Vec<Member>,
}

/// The types and functions of a program.
struct Program {
    rng: Rng,
    aggregates: Vec<Aggregate>,
    out: String,
}

/// A parameter's or argument's type.
#[derive(Clone, Copy, Debug)]
enum Ty {
    Scalar(Scalar),
    Agg(usize),
    /// A pointer to an aggregate the callee writes through.
    PtrAgg(usize),
}

impl Program {
    fn type_name(&self, index: usize) -> String {
        let kind = if self.aggregates[index].union { "union" } else { "struct" };
        format!("{kind} A{index}")
    }

    fn ty(&self, ty: Ty) -> String {
        match ty {
            Ty::Scalar(s) => s.c().to_owned(),
            Ty::Agg(i) => self.type_name(i),
            Ty::PtrAgg(i) => format!("{} *", self.type_name(i)),
        }
    }

    fn scalar(&mut self) -> Scalar {
        *self.rng.pick(&SCALARS)
    }

    /// A new aggregate of one of the shapes that matter to the calling
    /// conventions; its index.
    #[expect(clippy::too_many_lines, reason = "one arm per shape")]
    fn new_aggregate(&mut self) -> usize {
        let rng = &mut self.rng;
        let shape = rng.below(9);
        let mut members = Vec::new();
        let mut union = false;
        match shape {
            // Homogeneous floating-point aggregates (AArch64 passes them in
            // vector registers; x86-64 in SSE registers when small).
            0 => {
                let s = *rng.pick(&[Scalar::Float, Scalar::Double, Scalar::LongDouble]);
                for _ in 0..=rng.below(4) {
                    members.push(Member::Scalar(s));
                }
                if rng.chance(25) {
                    members = vec![Member::Array(s, 2 + rng.below(3))];
                }
            }
            // Small integer aggregates: one register or two.
            1 => {
                for _ in 0..=rng.below(3) {
                    let s = *rng.pick(&[
                        Scalar::SChar,
                        Scalar::UChar,
                        Scalar::Short,
                        Scalar::UShort,
                        Scalar::Int,
                        Scalar::UInt,
                    ]);
                    members.push(Member::Scalar(s));
                }
            }
            // Integer and floating-point members mixed (x86-64's INTEGER
            // and SSE eightbytes).
            2 => {
                let ints = [Scalar::Int, Scalar::Long, Scalar::Short, Scalar::Ptr, Scalar::UChar];
                let floats = [Scalar::Float, Scalar::Double];
                for i in 0..2 + rng.below(3) {
                    let s = if (i + shape).is_multiple_of(2) {
                        *rng.pick(&ints)
                    } else {
                        *rng.pick(&floats)
                    };
                    members.push(Member::Scalar(s));
                }
            }
            // Bit-fields.
            3 => {
                for _ in 0..2 + rng.below(5) {
                    let base = *rng.pick(&[
                        Scalar::Int,
                        Scalar::UInt,
                        Scalar::LongLong,
                        Scalar::ULongLong,
                        Scalar::UChar,
                        Scalar::Short,
                    ]);
                    let (bits, _) = base.integer().unwrap_or((32, true));
                    let width = 1 + u32::try_from(rng.below(bits as usize)).unwrap_or(0);
                    members.push(Member::Bits(base, width));
                }
                if rng.chance(50) {
                    members.insert(rng.below(members.len()), Member::Scalar(Scalar::Int));
                }
            }
            // Arrays.
            4 => {
                for _ in 0..=rng.below(2) {
                    let s = *rng.pick(&[
                        Scalar::UChar,
                        Scalar::Short,
                        Scalar::Int,
                        Scalar::Float,
                        Scalar::Double,
                        Scalar::LongLong,
                    ]);
                    members.push(Member::Array(s, 1 + rng.below(5)));
                }
            }
            // A union.
            5 => {
                union = true;
                for _ in 0..2 + rng.below(2) {
                    let s = *rng.pick(&SCALARS);
                    members.push(Member::Scalar(s));
                }
            }
            // Wide scalars alone or in pairs.
            6 => {
                let s = *rng.pick(&[Scalar::LongDouble, Scalar::Int128, Scalar::UInt128]);
                members.push(Member::Scalar(s));
                if rng.chance(50) {
                    members.insert(0, Member::Scalar(*rng.pick(&[Scalar::Int, Scalar::Double])));
                }
            }
            // Anything, many members (in memory on both targets).
            _ => {
                for _ in 0..3 + rng.below(6) {
                    members.push(Member::Scalar(*rng.pick(&SCALARS)));
                }
            }
        }
        // Nested aggregates.
        if !self.aggregates.is_empty() && self.rng.chance(30) {
            let inner = self.rng.below(self.aggregates.len());
            let at = self.rng.below(members.len() + 1);
            members.insert(at, Member::Nested(inner));
        }
        let packed = !union && self.rng.chance(15);
        self.aggregates.push(Aggregate { union, members });
        let index = self.aggregates.len() - 1;
        let name = self.type_name(index);
        let mut text = format!("{name} {{");
        for (m, member) in self.aggregates[index].members.iter().enumerate() {
            let decl = match member {
                Member::Scalar(s) => format!(" {} m{m};", s.c()),
                Member::Array(s, n) => format!(" {} m{m}[{n}];", s.c()),
                Member::Bits(s, w) => format!(" {} m{m} : {w};", s.c()),
                Member::Nested(i) => format!(" {} m{m};", self.type_name(*i)),
            };
            text.push_str(&decl);
        }
        text.push_str(if packed { " } __attribute__((packed));\n" } else { " };\n" });
        self.out.push_str(&text);
        index
    }

    /// A brace initializer of aggregate `index` (a union's member 0).
    fn initializer(&mut self, index: usize) -> String {
        let aggregate = self.aggregates[index].clone();
        let members =
            if aggregate.union { &aggregate.members[..1] } else { &aggregate.members[..] };
        let mut parts = Vec::new();
        for member in members {
            parts.push(match member {
                Member::Scalar(s) => s.literal(&mut self.rng),
                Member::Array(s, n) => {
                    let items: Vec<String> = (0..*n).map(|_| s.literal(&mut self.rng)).collect();
                    format!("{{ {} }}", items.join(", "))
                }
                Member::Bits(s, w) => {
                    let signed = s.integer().is_some_and(|(_, signed)| signed);
                    integer_literal(&mut self.rng, *w, signed)
                }
                Member::Nested(i) => self.initializer(*i),
            });
        }
        format!("{{ {} }}", parts.join(", "))
    }

    /// A constant expression of `ty` for an argument (`&local` for a
    /// pointer: the caller declares it).
    fn value(&mut self, ty: Ty) -> String {
        match ty {
            Ty::Scalar(s) => s.literal(&mut self.rng),
            Ty::Agg(i) | Ty::PtrAgg(i) => self.initializer(i),
        }
    }

    /// Statements printing every scalar of `expr`, an aggregate `index`.
    fn print_aggregate(&self, expr: &str, index: usize, out: &mut String) {
        let aggregate = &self.aggregates[index];
        let members =
            if aggregate.union { &aggregate.members[..1] } else { &aggregate.members[..] };
        for (m, member) in members.iter().enumerate() {
            let path = format!("{expr}.m{m}");
            match member {
                Member::Scalar(s) | Member::Bits(s, _) => {
                    let _ = writeln!(out, "  {}", s.print(&path));
                }
                Member::Array(s, n) => {
                    for i in 0..*n {
                        let _ = writeln!(out, "  {}", s.print(&format!("{path}[{i}]")));
                    }
                }
                Member::Nested(i) => self.print_aggregate(&path, *i, out),
            }
        }
    }

    fn print_value(&self, expr: &str, ty: Ty, out: &mut String) {
        match ty {
            Ty::Scalar(s) => {
                let _ = writeln!(out, "  {}", s.print(expr));
            }
            Ty::Agg(i) => self.print_aggregate(expr, i, out),
            Ty::PtrAgg(i) => self.print_aggregate(&format!("(*{expr})"), i, out),
        }
    }

    /// A statement writing a new value to the first scalar member of the
    /// aggregate `*expr` (a pointer parameter), if it has one.
    fn write_through(&mut self, expr: &str, index: usize) -> Option<String> {
        let aggregate = self.aggregates[index].clone();
        match aggregate.members.first()? {
            Member::Scalar(s) => Some(format!("  {expr}->m0 = {};", s.literal(&mut self.rng))),
            Member::Array(s, _) => Some(format!("  {expr}->m0[0] = {};", s.literal(&mut self.rng))),
            Member::Bits(s, w) => {
                let signed = s.integer().is_some_and(|(_, signed)| signed);
                Some(format!("  {expr}->m0 = {};", integer_literal(&mut self.rng, *w, signed)))
            }
            Member::Nested(_) => None,
        }
    }

    /// A parameter type.
    fn param(&mut self) -> Ty {
        let aggregates = self.aggregates.len();
        match self.rng.below(10) {
            5..=8 if aggregates > 0 => Ty::Agg(self.rng.below(aggregates)),
            9 if aggregates > 0 => Ty::PtrAgg(self.rng.below(aggregates)),
            _ => Ty::Scalar(self.scalar()),
        }
    }

    /// `callee_<k>` and `caller_<k>`.
    #[expect(clippy::too_many_lines, reason = "a callee and its caller, written in order")]
    fn pair(&mut self, k: usize, globals: &[(usize, String)]) {
        // Many parameters sometimes: past the argument registers of both
        // targets (6 + 8 on x86-64, 8 + 8 on AArch64).
        let count = if self.rng.chance(30) { 8 + self.rng.below(10) } else { self.rng.below(6) };
        let mut params: Vec<Ty> = (0..count).map(|_| self.param()).collect();
        let variadic: Vec<Ty> = if self.rng.chance(30) {
            (0..=self.rng.below(6))
                .map(|_| {
                    if !self.aggregates.is_empty() && self.rng.chance(30) {
                        Ty::Agg(self.rng.below(self.aggregates.len()))
                    } else {
                        Ty::Scalar(*self.rng.pick(&PROMOTED))
                    }
                })
                .collect()
        } else {
            Vec::new()
        };
        // `va_start` names the last fixed parameter, which must be of a
        // type the promotions leave alone: an `int`.
        if !variadic.is_empty() {
            params.push(Ty::Scalar(Scalar::Int));
        }
        let ret = match self.rng.below(10) {
            0..=1 => None,
            2..=4 => Some(Ty::Scalar(self.scalar())),
            _ if !self.aggregates.is_empty() => {
                Some(Ty::Agg(self.rng.below(self.aggregates.len())))
            }
            _ => Some(Ty::Scalar(self.scalar())),
        };
        let ret_c = ret.map_or_else(|| "void".to_owned(), |t| self.ty(t));
        // The callee.
        let mut list: Vec<String> =
            params.iter().enumerate().map(|(i, &t)| format!("{} p{i}", self.ty(t))).collect();
        if !variadic.is_empty() {
            list.push("...".to_owned());
        }
        let fixed = params.len();
        let signature = if list.is_empty() { "void".to_owned() } else { list.join(", ") };
        let mut body = String::new();
        let _ = writeln!(body, "  printf(\"callee_{k}:\");");
        for (i, &t) in params.iter().enumerate() {
            self.print_value(&format!("p{i}"), t, &mut body);
        }
        if !variadic.is_empty() {
            let _ = writeln!(body, "  va_list ap;\n  va_start(ap, p{});", fixed - 1);
            for (i, &t) in variadic.iter().enumerate() {
                let _ = writeln!(body, "  {} v{i} = va_arg(ap, {});", self.ty(t), self.ty(t));
                self.print_value(&format!("v{i}"), t, &mut body);
            }
            let _ = writeln!(body, "  va_end(ap);");
        }
        let _ = writeln!(body, "  printf(\"\\n\");");
        for (i, &t) in params.iter().enumerate() {
            if let Ty::PtrAgg(a) = t
                && let Some(write) = self.write_through(&format!("p{i}"), a)
            {
                let _ = writeln!(body, "{write}");
            }
        }
        if let Some(t) = ret {
            // Return a parameter of the same type sometimes, else a constant.
            let same = params.iter().position(|&p| self.ty(p) == self.ty(t));
            let value = match same {
                Some(i) if self.rng.chance(50) => format!("p{i}"),
                _ => match t {
                    Ty::Agg(i) => {
                        let init = self.initializer(i);
                        let _ = writeln!(body, "  {} r = {init};", self.ty(t));
                        "r".to_owned()
                    }
                    _ => self.value(t),
                },
            };
            let _ = writeln!(body, "  return {value};");
        }
        let _ = writeln!(self.out, "{ret_c} callee_{k}({signature}) {{\n{body}}}\n");
        // The caller.
        let mut body = String::new();
        let mut args = Vec::new();
        let mut written = Vec::new();
        let all: Vec<Ty> = params.iter().chain(&variadic).copied().collect();
        for (i, &t) in all.iter().enumerate() {
            match t {
                Ty::PtrAgg(a) => {
                    let init = self.initializer(a);
                    let _ = writeln!(body, "  {} a{i} = {init};", self.type_name(a));
                    args.push(format!("&a{i}"));
                    written.push((format!("a{i}"), a));
                }
                Ty::Agg(a) => {
                    // A global of the type sometimes: its layout is the
                    // other object's in half of the cases.
                    match globals.iter().find(|(g, _)| *g == a) {
                        Some((_, name)) if self.rng.chance(40) => args.push(name.clone()),
                        _ => {
                            let init = self.initializer(a);
                            let _ = writeln!(body, "  {} a{i} = {init};", self.type_name(a));
                            args.push(format!("a{i}"));
                        }
                    }
                }
                // Of the parameter's type exactly (a variadic argument's
                // type is the one `va_arg` reads).
                Ty::Scalar(s) => args.push(format!("({})({})", s.c(), s.literal(&mut self.rng))),
            }
        }
        let call = format!("callee_{k}({})", args.join(", "));
        match ret {
            Some(t) => {
                let _ = writeln!(body, "  {} r = {call};", self.ty(t));
                let _ = writeln!(body, "  printf(\"caller_{k}:\");");
                self.print_value("r", t, &mut body);
            }
            None => {
                let _ = writeln!(body, "  {call};\n  printf(\"caller_{k}:\");");
            }
        }
        for (name, a) in written {
            self.print_aggregate(&name, a, &mut body);
        }
        let _ = writeln!(body, "  printf(\"\\n\");");
        let _ = writeln!(self.out, "void caller_{k}(void) {{\n{body}}}\n");
    }
}

/// The program of `seed`.
pub fn program(seed: u64) -> String {
    let mut p = Program { rng: Rng::new(seed), aggregates: Vec::new(), out: String::new() };
    let _ = writeln!(
        p.out,
        "/* An ABI program of `cargo xtask fuzz --profile abi`, seed {seed}. */\n\
         #include <stdarg.h>\n#include <stdio.h>\n\nint g_targets[4];\n"
    );
    for _ in 0..3 + p.rng.below(6) {
        p.new_aggregate();
    }
    p.out.push('\n');
    // Globals with static initializers.
    let mut globals = Vec::new();
    for g in 0..p.rng.below(4) {
        let a = p.rng.below(p.aggregates.len());
        let init = p.initializer(a);
        let _ = writeln!(p.out, "{} g{g} = {init};", p.type_name(a));
        globals.push((a, format!("g{g}")));
    }
    p.out.push('\n');
    let pairs = 4 + p.rng.below(5);
    for k in 0..pairs {
        p.pair(k, &globals);
    }
    let mut main = String::from("int main(void) {\n");
    for k in 0..pairs {
        let _ = writeln!(main, "  caller_{k}();");
    }
    for (a, name) in &globals {
        let _ = writeln!(main, "  printf(\"{name}:\");");
        p.print_aggregate(name, *a, &mut main);
        let _ = writeln!(main, "  printf(\"\\n\");");
    }
    main.push_str("  return 0;\n}\n");
    p.out.push_str(&main);
    p.out
}

#[cfg(test)]
mod tests {
    use super::*;

    // spec/harness.md "Random programs", "Profiles": the generator is a
    // function of the seed, and its programs have what the profile is for.
    #[test]
    fn programs_by_seed() {
        assert_eq!(program(5), program(5));
        assert_ne!(program(5), program(6));
        let all: String = (0..40).map(program).collect();
        for needle in [
            "long double",
            "__int128",
            "va_arg(ap,",
            "__attribute__((packed))",
            "union A",
            " : ",
            "callee_",
            "caller_",
        ] {
            assert!(all.contains(needle), "{needle}");
        }
    }

    // Literals are of their type: within range, exact floating point.
    #[test]
    fn literals() {
        let mut rng = Rng::new(1);
        for _ in 0..200 {
            let text = integer_literal(&mut rng, 8, true);
            let value: i64 = text.trim_end_matches('L').parse().unwrap();
            assert!((-127..=127).contains(&value), "{text}");
            let text = integer_literal(&mut rng, 3, false);
            let value: u64 = text.trim_end_matches('U').parse().unwrap();
            assert!(value < 8, "{text}");
        }
        let float = Scalar::Float.literal(&mut rng);
        assert!(float.contains("0x") && float.ends_with('f'), "{float}");
    }
}
