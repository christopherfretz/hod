//! Constant folding differential tests (`spec/ir.md` §4.2): what
//! `ConstPool::fold_gep`, `fold_cast` and `fold_binary` make of a constant
//! expression against what LLVM makes of it, on both targets, with LLVM 22
//! and LLVM 19.
//!
//! Each configuration compiles one module of global variables whose
//! initializers are random constant expressions: integer arithmetic at any
//! width, and at pointer-sized and narrower widths every shape that folds
//! to a number, to `symbol + addend` or to `symbol - symbol + addend`,
//! through `getelementptr` over random types, `ptrtoint`, `ptrtoaddr`,
//! `inttoptr`, `trunc`, `add` and `sub`. The hand-written address forms of
//! `flatten_diff.rs` are compiled too, with those an object file cannot
//! hold.
//!
//! The oracle is the assembly `llc` prints (one run per configuration): the
//! directives of each variable are read back as bytes and as linear
//! expressions over symbols, and compared with the image and fixups of the
//! folded constant. Numbers are compared modulo the width of their field,
//! which is what a field can hold. `flatten_diff.rs` checks the
//! relocations an assembler makes of the same forms.
//!
//! The operators LLVM's text format no longer has as constant expressions
//! (`and`, `or`, `shl`, `zext`, `sext` in both versions, `mul` since
//! LLVM 20) have unit tests in `src/fold.rs` only.

mod common;

use std::collections::{BTreeMap, HashMap};
use std::fmt::Write as _;

use common::consts::{Ctx, Form, PRELUDE, address_cases};
use common::{CONFIGS, Config, Ty};
use hod_ir::{ConstId, ConstOp, FixupTarget, IrErrorKind};
use hod_testutil::Scratch;

/// Random expressions per configuration.
const EXPR_COUNT: usize = 1500;

/// A sum of symbols with coefficients, and a number.
#[derive(Clone, Default, PartialEq, Eq, Debug)]
struct Linear {
    symbols: BTreeMap<String, i64>,
    number: i128,
}

impl Linear {
    fn add(&mut self, other: &Linear, sign: i64) {
        for (symbol, coefficient) in &other.symbols {
            let total = self.symbols.entry(symbol.clone()).or_insert(0);
            *total += sign * coefficient;
            if *total == 0 {
                self.symbols.remove(symbol);
            }
        }
        self.number = self.number.wrapping_add(other.number.wrapping_mul(i128::from(sign)));
    }
}

/// A reader of the expressions `llc` prints: sums and differences of
/// numbers, symbols and parenthesized expressions.
struct Reader<'a> {
    text: &'a [u8],
    at: usize,
}

impl Reader<'_> {
    fn peek(&mut self) -> Option<u8> {
        while self.text.get(self.at).is_some_and(u8::is_ascii_whitespace) {
            self.at += 1;
        }
        self.text.get(self.at).copied()
    }

    fn sum(&mut self) -> Linear {
        let mut total = self.term();
        loop {
            let sign = match self.peek() {
                Some(b'+') => 1,
                Some(b'-') => -1,
                _ => return total,
            };
            self.at += 1;
            let term = self.term();
            total.add(&term, sign);
        }
    }

    fn term(&mut self) -> Linear {
        let mut value = Linear::default();
        match self.peek().expect("an operand") {
            b'-' => {
                self.at += 1;
                let term = self.term();
                value.add(&term, -1);
            }
            b'(' => {
                self.at += 1;
                value = self.sum();
                assert_eq!(self.peek(), Some(b')'), "an unclosed parenthesis");
                self.at += 1;
            }
            _ => {
                let start = self.at;
                let is_name = |c: &u8| c.is_ascii_alphanumeric() || b"_.$@".contains(c);
                while self.text.get(self.at).is_some_and(is_name) {
                    self.at += 1;
                }
                let word = std::str::from_utf8(&self.text[start..self.at]).expect("ASCII");
                assert!(!word.is_empty(), "an operand, not {:?}", char::from(self.text[start]));
                if word.as_bytes()[0].is_ascii_digit() {
                    let number = match word.strip_prefix("0x") {
                        Some(digits) => u128::from_str_radix(digits, 16),
                        None => word.parse(),
                    };
                    value.number = number.unwrap_or_else(|_| panic!("number {word}")) as i128;
                } else {
                    value.symbols.insert(word.to_string(), 1);
                }
            }
        }
        value
    }
}

fn read_expression(text: &str) -> Linear {
    let mut reader = Reader { text: text.as_bytes(), at: 0 };
    let value = reader.sum();
    assert_eq!(reader.peek(), None, "trailing text in {text:?}");
    value
}

/// A field whose value is known at link time: symbols and a number, modulo
/// the field's width.
#[derive(Clone, PartialEq, Eq, Debug)]
struct Field {
    offset: usize,
    size: usize,
    symbols: BTreeMap<String, i64>,
    number: u64,
}

/// The contents of a variable: bytes, zero where a field is, and the fields.
#[derive(Clone, Default, PartialEq, Eq, Debug)]
struct Image {
    bytes: Vec<u8>,
    fields: Vec<Field>,
}

fn low_bits(number: u64, size: usize) -> u64 {
    if size == 8 { number } else { number & ((1 << (8 * size)) - 1) }
}

impl Image {
    fn push(&mut self, size: usize, value: &Linear) {
        let number = low_bits(value.number as u64, size);
        let offset = self.bytes.len();
        if value.symbols.is_empty() {
            self.bytes.extend_from_slice(&number.to_le_bytes()[..size]);
        } else {
            self.bytes.resize(offset + size, 0);
            self.fields.push(Field { offset, size, symbols: value.symbols.clone(), number });
        }
    }
}

/// The variables named in `wanted`, as `llc`'s assembly defines them: the
/// data directives between `name:` and `.size name, N`, and those
/// directives as text.
fn read_assembly(text: &str, wanted: &[&str]) -> HashMap<String, (Image, String)> {
    let mut images = HashMap::new();
    let mut current: Option<(String, Image, String)> = None;
    for line in text.lines() {
        // Comments: `#` on x86-64, `//` on AArch64. No directive read here
        // has a string operand.
        let line = line.split('#').next().unwrap_or("");
        let line = line.split("//").next().unwrap_or("").trim();
        if let Some(label) = line.strip_suffix(':') {
            assert!(current.is_none(), "label {label} inside a variable");
            if wanted.contains(&label) {
                current = Some((label.to_string(), Image::default(), String::new()));
            }
            continue;
        }
        let Some((name, image, directives)) = &mut current else { continue };
        if line.is_empty() {
            continue;
        }
        let (directive, operands) = line.split_once(char::is_whitespace).unwrap_or((line, ""));
        let _ = write!(directives, "\n    {line}");
        let size = match directive {
            ".byte" => 1,
            ".short" | ".hword" => 2,
            ".long" | ".word" => 4,
            ".quad" | ".xword" => 8,
            ".zero" => {
                let count: usize = operands.trim().parse().expect("a byte count");
                image.bytes.resize(image.bytes.len() + count, 0);
                continue;
            }
            ".size" => {
                let (symbol, size) = operands.split_once(',').expect("a symbol and a size");
                assert_eq!(symbol.trim(), name, ".size of another symbol");
                let size: usize = size.trim().parse().expect("a size");
                assert_eq!(size, image.bytes.len(), "{name}: {directives}");
                let (name, image, directives) = current.take().expect("a variable");
                images.insert(name, (image, directives));
                continue;
            }
            other => panic!("{name}: unexpected directive {other} in {line:?}"),
        };
        for operand in operands.split(',') {
            image.push(size, &read_expression(operand));
        }
    }
    images
}

/// The image of constant `value` as hod flattens it.
fn hod_image(ctx: &Ctx, value: ConstId) -> Result<Image, String> {
    let (mut bytes, mut fixups) = (Vec::new(), Vec::new());
    ctx.consts
        .flatten(&ctx.types, value, &mut bytes, &mut fixups)
        .map_err(|error| format!("flatten failed: {error}"))?;
    let fields = fixups
        .iter()
        .map(|fixup| {
            let mut value = Linear::default();
            let mut term = |name: String, sign| {
                let symbol = Linear { symbols: BTreeMap::from([(name, 1)]), number: 0 };
                value.add(&symbol, sign);
            };
            match fixup.target {
                FixupTarget::Sym(sym) => term(ctx.name(sym).to_string(), 1),
                FixupTarget::Diff { a, b } => {
                    term(ctx.name(a).to_string(), 1);
                    term(ctx.name(b).to_string(), -1);
                }
                FixupTarget::PltDiff { a, b } => {
                    term(format!("{}@PLT", ctx.name(a)), 1);
                    term(ctx.name(b).to_string(), -1);
                }
                FixupTarget::Plt(sym) => term(format!("{}@PLT", ctx.name(sym)), 1),
                FixupTarget::Block { .. } => panic!("no case has a block address"),
            }
            let size = usize::from(fixup.size);
            Field {
                offset: fixup.offset as usize,
                size,
                symbols: value.symbols,
                number: low_bits(fixup.addend as u64, size),
            }
        })
        .collect();
    Ok(Image { bytes, fields })
}

/// A variable of the module.
struct Global {
    name: String,
    definition: String,
    value: ConstId,
}

/// The module of one configuration, its variables with their initializers
/// folded in `ctx`, and how many there are of each kind.
fn module(config: &Config, ctx: &mut Ctx) -> (String, Vec<Global>, [usize; 7]) {
    let mut ir = config.header();
    ir.push_str(PRELUDE);
    let mut globals = Vec::new();
    let mut counts = [0usize; 7];
    for index in 0..EXPR_COUNT {
        let name = format!("e{index}");
        ctx.this = ctx.sym(&name);
        let kind = index % 6;
        let width = *ctx.rng.pick(&[8, 16, 32, 64, 64]);
        let (ty, text, value) = match kind {
            0 => {
                let (bits, text, value) = ctx.wide_literal_expr();
                (Ty::Int(bits), text, value)
            }
            1..=3 => {
                let form = [Form::Number, Form::Symbol, Form::Difference][kind - 1];
                let (text, value) = ctx.int_expr(width, form, 4);
                (Ty::Int(width), text, value)
            }
            _ => {
                let (text, value) = ctx.pointer_expr(kind == 4, 4);
                (Ty::Ptr(0), text, value)
            }
        };
        counts[kind] += 1;
        let definition = format!("@{name} = global {} {text}", ty.text());
        let _ = writeln!(ir, "{definition}");
        assert_eq!(ctx.consts.ty(value), ty.intern(&mut ctx.types), "{definition}");
        globals.push(Global { name, definition, value });
    }
    for (index, case) in address_cases().iter().enumerate() {
        let name = format!("k{index}");
        ctx.this = ctx.sym(&name);
        let text = case.text.replace("{self}", &format!("@{name}"));
        let definition = format!("@{name} = global {} {text}", case.ty.text());
        let _ = writeln!(ir, "{definition}");
        let value = (case.build)(ctx);
        counts[6] += 1;
        globals.push(Global { name, definition, value });
    }
    (ir, globals, counts)
}

fn check(config: &Config) {
    let Some(tools) = common::tools(config.llvm) else { return };
    let mut ctx = Ctx::new(config, 0x00f0_1d5e);
    let (ir, globals, counts) = module(config, &mut ctx);

    let scratch = Scratch::new(&format!("fold-{}", config.name()));
    let assembly_path = tools.compile(&scratch, "fold", &ir, true).unwrap_or_else(|error| {
        panic!("{}: llc rejected the expression module:\n{error}", config.name())
    });
    let assembly = std::fs::read_to_string(&assembly_path).expect("llc's output");
    let names: Vec<&str> = globals.iter().map(|global| global.name.as_str()).collect();
    let images = read_assembly(&assembly, &names);

    let mut failures = Vec::new();
    let (mut numbers, mut fields) = (0, 0);
    for Global { name, definition, value } in &globals {
        let (llvm, directives) =
            images.get(name).unwrap_or_else(|| panic!("{name} is not in llc's output"));
        match hod_image(&ctx, *value) {
            Ok(hod) if hod == *llvm => {
                if hod.fields.is_empty() {
                    numbers += 1;
                } else {
                    fields += hod.fields.len();
                }
            }
            Ok(hod) => failures.push(format!(
                "{definition}\n  hod  {:?}\n    {hod:?}\n  LLVM {llvm:?}{directives}",
                ctx.consts.get(*value)
            )),
            Err(error) => failures.push(format!(
                "{definition}\n  hod  {:?}\n    {error}\n  LLVM{directives}",
                ctx.consts.get(*value)
            )),
        }
    }
    eprintln!(
        "fold_diff {}: {} expressions ({} of literals at any width, {} numbers, {} symbol + \
         addend, {} symbol differences, {} symbolic pointers, {} absolute pointers, {} address \
         forms by hand); {numbers} fold to numbers, the rest to {fields} fields with symbols",
        config.name(),
        globals.len(),
        counts[0],
        counts[1],
        counts[2],
        counts[3],
        counts[4],
        counts[5],
        counts[6],
    );
    assert!(
        failures.is_empty(),
        "{}: {} folding mismatches, the first of them:\n{}",
        config.name(),
        failures.len(),
        failures.iter().take(15).cloned().collect::<Vec<_>>().join("\n")
    );
}

/// §4.2 folds on aarch64 against LLVM 22.
#[test]
fn folds_match_llvm22_on_aarch64() {
    check(&CONFIGS[0]);
}

/// §4.2 folds on x86-64 against LLVM 22.
#[test]
fn folds_match_llvm22_on_x86_64() {
    check(&CONFIGS[1]);
}

/// §4.2 folds on aarch64 against LLVM 19.
#[test]
fn folds_match_llvm19_on_aarch64() {
    check(&CONFIGS[2]);
}

/// §4.2 folds on x86-64 against LLVM 19.
#[test]
fn folds_match_llvm19_on_x86_64() {
    check(&CONFIGS[3]);
}

/// Builds a constant through hod's folds.
type Build = fn(&mut Ctx) -> ConstId;

/// An initializer `llc` cannot emit, and the same expression through hod's
/// folds.
struct Rejected {
    ty: &'static str,
    text: &'static str,
    /// LLVM versions whose text format has the expression.
    llvm: std::ops::RangeInclusive<u32>,
    build: Build,
}

/// `ptrtoint (ptr @a to i64)`.
fn a_as_int(c: &mut Ctx) -> ConstId {
    let a = c.addr("a", 0);
    c.ptr_to_int(a, 64)
}

const REJECTED: [Rejected; 5] = [
    Rejected {
        ty: "i64",
        text: "add (i64 ptrtoint (ptr @a to i64), i64 ptrtoint (ptr @a to i64))",
        llvm: 0..=u32::MAX,
        build: |c| {
            let a = a_as_int(c);
            c.add(a, a)
        },
    },
    Rejected {
        ty: "i64",
        text: "add (i64 ptrtoint (ptr @a to i64), i64 ptrtoint (ptr @x to i64))",
        llvm: 0..=u32::MAX,
        build: |c| {
            let (a, x) = (a_as_int(c), c.addr("x", 0));
            let x = c.ptr_to_int(x, 64);
            c.add(a, x)
        },
    },
    Rejected {
        ty: "i64",
        text: "xor (i64 ptrtoint (ptr @a to i64), i64 2)",
        llvm: 0..=u32::MAX,
        build: |c| {
            let (a, two) = (a_as_int(c), c.lit(64, 2));
            c.binary(ConstOp::Xor, a, two)
        },
    },
    Rejected {
        ty: "i64",
        text: "mul (i64 ptrtoint (ptr @a to i64), i64 2)",
        llvm: 0..=19,
        build: |c| {
            let (a, two) = (a_as_int(c), c.lit(64, 2));
            c.binary(ConstOp::Mul, a, two)
        },
    },
    Rejected {
        ty: "i128",
        text: "ptrtoint (ptr @a to i128)",
        llvm: 0..=u32::MAX,
        build: |c| {
            let a = c.addr("a", 0);
            c.ptr_to_int(a, 128)
        },
    },
];

/// What `llc` rejects in an initializer, hod's folds leave as an
/// expression with no image (`UnsupportedInitializer`), never as a wrong
/// address.
#[test]
fn what_llc_rejects_has_no_image() {
    for config in &CONFIGS {
        let Some(tools) = common::tools(config.llvm) else { continue };
        let scratch = Scratch::new(&format!("fold-rejected-{}", config.name()));
        let mut count = 0;
        for (index, case) in REJECTED.iter().enumerate() {
            if !case.llvm.contains(&config.llvm) {
                continue;
            }
            let definition = format!("@r = global {} {}", case.ty, case.text);
            let ir = format!("{}{PRELUDE}{definition}\n", config.header());
            let outcome = tools.compile(&scratch, &format!("rejected{index}"), &ir, false);
            assert!(outcome.is_err(), "{}: llc accepted {definition}", config.name());

            let mut ctx = Ctx::new(config, 1);
            let value = (case.build)(&mut ctx);
            let (mut bytes, mut fixups) = (Vec::new(), Vec::new());
            let error = ctx
                .consts
                .flatten(&ctx.types, value, &mut bytes, &mut fixups)
                .expect_err(&definition);
            assert_eq!(error.kind, IrErrorKind::UnsupportedInitializer, "{definition}");
            count += 1;
        }
        eprintln!("fold_diff {}: {count} initializers llc rejects have no image", config.name());
    }
}

/// The initializers `llc` emits that hod has no image for. Each is a
/// limit of the normal forms, recorded in `spec/landed/ir/WP-002.md`; the
/// test fails when either side changes.
#[test]
fn what_llc_accepts_and_hod_does_not() {
    let cases: [(&str, &str, Build); 2] = [
        // A number minus an address: `llc` emits a PC-relative relocation
        // with no symbol. Not `symbol + addend`, so hod keeps the `sub`.
        ("i64", "sub (i64 4, i64 ptrtoint (ptr @a to i64))", |c| {
            let (four, a) = (c.lit(64, 4), a_as_int(c));
            c.sub(four, a)
        }),
        // `llc` emits a 32-bit relocation in the four bytes an `i24` is
        // allocated. hod has fixups for 8, 16, 32 and 64 bits only.
        ("i24", "ptrtoint (ptr @a to i24)", |c| {
            let a = c.addr("a", 0);
            c.ptr_to_int(a, 24)
        }),
    ];
    for config in &CONFIGS {
        let Some(tools) = common::tools(config.llvm) else { continue };
        let scratch = Scratch::new(&format!("fold-limits-{}", config.name()));
        for (index, (ty, text, build)) in cases.iter().enumerate() {
            let definition = format!("@r = global {ty} {text}");
            let ir = format!("{}{PRELUDE}{definition}\n", config.header());
            let outcome = tools.compile(&scratch, &format!("limit{index}"), &ir, false);
            assert!(outcome.is_ok(), "{}: llc rejected {definition}", config.name());

            let mut ctx = Ctx::new(config, 1);
            let value = build(&mut ctx);
            let (mut bytes, mut fixups) = (Vec::new(), Vec::new());
            let error = ctx
                .consts
                .flatten(&ctx.types, value, &mut bytes, &mut fixups)
                .expect_err(&definition);
            assert_eq!(error.kind, IrErrorKind::UnsupportedInitializer, "{definition}");
        }
    }
}
