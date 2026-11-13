//! Support shared by `hod_ir`'s differential tests: the oracle
//! configurations (target x LLVM version), a deterministic generator of
//! LLVM types, and readers for the output of `llvm-readelf` and
//! `llvm-objdump`.
//!
//! Each test file uses part of this module.
#![allow(dead_code)]

pub mod consts;

use std::fmt::Write as _;
use std::path::{Path, PathBuf};

use hod_ir::{
    AARCH64_LINUX_DATA_LAYOUT, AARCH64_LINUX_DATA_LAYOUT_LLVM19, Arch, DataLayout, TypeId,
    TypeTable, X86_64_LINUX_DATA_LAYOUT,
};
use hod_testutil::Scratch;

/// One oracle: a target, an LLVM version, and the data-layout string kiln
/// emits for that pair.
#[derive(Clone, Copy, Debug)]
pub struct Config {
    pub arch: Arch,
    pub llvm: u32,
    pub triple: &'static str,
    pub layout: &'static str,
}

impl Config {
    /// A short name for messages and scratch files.
    pub fn name(&self) -> String {
        let arch = match self.arch {
            Arch::Aarch64 => "aarch64",
            Arch::X86_64 => "x86_64",
        };
        format!("{arch}-llvm{}", self.llvm)
    }

    /// The `target` lines that start a module.
    pub fn header(&self) -> String {
        format!("target datalayout = \"{}\"\ntarget triple = \"{}\"\n", self.layout, self.triple)
    }

    /// A type table laid out as this configuration's LLVM reads its
    /// data layout.
    pub fn type_table(&self) -> TypeTable {
        type_table_for(self.llvm, self.layout)
    }
}

/// A type table laid out as LLVM `major` reads the data-layout string
/// `layout`.
///
/// hod's parser has LLVM 22's rules. LLVM 19 differs in one: it has a
/// default `i1:8:8` entry, which the parser is given explicitly.
pub fn type_table_for(major: u32, layout: &str) -> TypeTable {
    let text = if major < 20 { format!("i1:8-{layout}") } else { layout.to_string() };
    let parsed = DataLayout::parse(&text).unwrap_or_else(|error| panic!("{text}: {error}"));
    TypeTable::new(&parsed)
}

/// Both targets with LLVM 22 and with LLVM 19, each with kiln's layout
/// string for that version.
pub const CONFIGS: [Config; 4] = [
    Config {
        arch: Arch::Aarch64,
        llvm: 22,
        triple: "aarch64-unknown-linux-gnu",
        layout: AARCH64_LINUX_DATA_LAYOUT,
    },
    Config {
        arch: Arch::X86_64,
        llvm: 22,
        triple: "x86_64-unknown-linux-gnu",
        layout: X86_64_LINUX_DATA_LAYOUT,
    },
    Config {
        arch: Arch::Aarch64,
        llvm: 19,
        triple: "aarch64-unknown-linux-gnu",
        layout: AARCH64_LINUX_DATA_LAYOUT_LLVM19,
    },
    Config {
        arch: Arch::X86_64,
        llvm: 19,
        triple: "x86_64-unknown-linux-gnu",
        layout: X86_64_LINUX_DATA_LAYOUT,
    },
];

/// The LLVM tools of one major version.
pub struct Tools {
    pub major: u32,
    pub llc: PathBuf,
    pub opt: PathBuf,
    pub objdump: PathBuf,
    pub readelf: PathBuf,
}

/// Finds LLVM `major`'s tools in `/usr/lib/llvm-<major>/bin`.
///
/// `None`, after a note, when they are missing and tools are optional.
///
/// # Panics
/// Panics when they are missing and `HOD_REQUIRE_TOOLS=1`.
pub fn tools(major: u32) -> Option<Tools> {
    let bin = PathBuf::from(format!("/usr/lib/llvm-{major}/bin"));
    let find = |name: &str| {
        let path = bin.join(name);
        path.is_file().then_some(path)
    };
    if let (Some(llc), Some(opt), Some(objdump), Some(readelf)) =
        (find("llc"), find("opt"), find("llvm-objdump"), find("llvm-readelf"))
    {
        return Some(Tools { major, llc, opt, objdump, readelf });
    }
    assert!(
        !hod_testutil::tools_required(),
        "LLVM {major} tools not found in {} and HOD_REQUIRE_TOOLS=1",
        bin.display()
    );
    eprintln!("note: skipping: LLVM {major} tools not found in {}", bin.display());
    None
}

impl Tools {
    /// Compiles `ir` at `-O0` to `<name>.o` (or `<name>.s` when `asm`) in
    /// `scratch`. `Err` holds `llc`'s diagnostics.
    pub fn compile(
        &self,
        scratch: &Scratch,
        name: &str,
        ir: &str,
        asm: bool,
    ) -> Result<PathBuf, String> {
        let input = scratch.write(&format!("{name}.ll"), ir);
        let output = scratch.join(format!("{name}.{}", if asm { "s" } else { "o" }));
        let filetype = if asm { "-filetype=asm" } else { "-filetype=obj" };
        let outcome = hod_testutil::run(
            &self.llc,
            [
                "-O0".as_ref(),
                filetype.as_ref(),
                "-o".as_ref(),
                output.as_os_str(),
                input.as_os_str(),
            ],
        );
        if outcome.success() { Ok(output) } else { Err(outcome.stderr_text()) }
    }

    /// Runs `opt -S` on `ir` with the given extra arguments and returns the
    /// printed module. `Err` holds `opt`'s diagnostics.
    pub fn opt(
        &self,
        scratch: &Scratch,
        name: &str,
        ir: &str,
        args: &[&str],
    ) -> Result<String, String> {
        let input = scratch.write(&format!("{name}.ll"), ir);
        let mut all: Vec<&std::ffi::OsStr> = vec!["-S".as_ref(), "-o".as_ref(), "-".as_ref()];
        all.extend(args.iter().map(std::ffi::OsStr::new));
        all.push(input.as_os_str());
        let outcome = hod_testutil::run(&self.opt, all);
        if outcome.success() { Ok(outcome.stdout_text()) } else { Err(outcome.stderr_text()) }
    }

    /// The contents of section `name` of `object`, or nothing when the
    /// object has no such section.
    pub fn section_bytes(&self, object: &Path, name: &str) -> Vec<u8> {
        let outcome = hod_testutil::run(
            &self.objdump,
            ["-s".as_ref(), "-j".as_ref(), name.as_ref(), object.as_os_str()],
        );
        assert!(outcome.success(), "llvm-objdump failed: {}", outcome.stderr_text());
        parse_hex_dump(&outcome.stdout_text())
    }

    /// The sections, symbols and `.rela.data` relocations of `object`.
    pub fn read_object(&self, object: &Path) -> Object {
        let outcome = hod_testutil::run(
            &self.readelf,
            ["-S".as_ref(), "-r".as_ref(), "-s".as_ref(), "--wide".as_ref(), object.as_os_str()],
        );
        assert!(outcome.success(), "llvm-readelf failed: {}", outcome.stderr_text());
        parse_readelf(&outcome.stdout_text())
    }
}

/// The bytes of an `llvm-objdump -s` hex dump. Each line is an address,
/// then up to sixteen bytes in four groups, then the same bytes as text.
pub fn parse_hex_dump(text: &str) -> Vec<u8> {
    let mut bytes = Vec::new();
    let mut lines = text.lines().skip_while(|line| !line.starts_with("Contents of section"));
    lines.next();
    for line in lines {
        let Some(rest) = line.strip_prefix(' ') else { break };
        let Some(space) = rest.find(' ') else { break };
        // Four groups of a space and eight hex digits; short lines are
        // padded with spaces.
        let hex: String = rest[space..].chars().take(36).filter(|c| !c.is_whitespace()).collect();
        assert!(hex.len().is_multiple_of(2), "odd hex dump line: {line}");
        for at in (0..hex.len()).step_by(2) {
            bytes.push(u8::from_str_radix(&hex[at..at + 2], 16).expect("hex digits"));
        }
    }
    bytes
}

/// Where a symbol is defined.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Place {
    /// In the section with this index.
    Section(u32),
    /// Nowhere: `UND`.
    Undefined,
    /// A common symbol: `COM`.
    Common,
    /// Anything else (`ABS`).
    Other,
}

#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Symbol {
    pub name: String,
    pub value: u64,
    pub size: u64,
    pub place: Place,
}

#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Reloc {
    pub offset: u64,
    /// The relocation type's name, such as `R_X86_64_64`.
    pub kind: String,
    /// The symbol's name; empty for a relocation with no symbol.
    pub symbol: String,
    pub addend: i64,
}

/// What `llvm-readelf -S -r -s --wide` says about an object.
#[derive(Default, Debug)]
pub struct Object {
    /// Section names by index.
    pub sections: Vec<String>,
    pub symbols: Vec<Symbol>,
    /// The entries of `.rela.data`, in file order.
    pub data_relocs: Vec<Reloc>,
}

impl Object {
    pub fn symbol(&self, name: &str) -> &Symbol {
        self.symbols
            .iter()
            .find(|symbol| symbol.name == name)
            .unwrap_or_else(|| panic!("no symbol {name} in the object"))
    }

    pub fn section_index(&self, name: &str) -> Option<u32> {
        self.sections.iter().position(|section| section == name).map(|at| at as u32)
    }
}

fn parse_readelf(text: &str) -> Object {
    #[derive(PartialEq)]
    enum Part {
        None,
        Sections,
        DataRelocs,
        Symbols,
    }
    let mut object = Object::default();
    let mut part = Part::None;
    for line in text.lines() {
        if line.starts_with("Section Headers:") {
            part = Part::Sections;
        } else if line.starts_with("Relocation section") {
            part = if line.contains("'.rela.data'") { Part::DataRelocs } else { Part::None };
        } else if line.starts_with("Symbol table") {
            part = Part::Symbols;
        } else if line.starts_with("Key to Flags") || line.trim().is_empty() {
            part = Part::None;
        } else {
            match part {
                Part::Sections => parse_section_line(line, &mut object),
                Part::DataRelocs => object.data_relocs.extend(parse_reloc_line(line)),
                Part::Symbols => object.symbols.extend(parse_symbol_line(line)),
                Part::None => {}
            }
        }
    }
    object
}

/// `  [ 3] .data  PROGBITS ...`
fn parse_section_line(line: &str, object: &mut Object) {
    let Some(rest) = line.trim_start().strip_prefix('[') else { return };
    let Some((index, rest)) = rest.split_once(']') else { return };
    let Ok(index) = index.trim().parse::<usize>() else { return };
    assert_eq!(index, object.sections.len(), "sections out of order: {line}");
    // The null section has no name: its first column is its type.
    let name = if index == 0 { "" } else { rest.split_whitespace().next().unwrap_or("") };
    object.sections.push(name.to_string());
}

/// `0000000000000048  0000000c00000001 R_X86_64_64  0000000000000000 x - 3`
/// (the addend in hex), or with nothing but the addend after the type for a
/// relocation with no symbol.
fn parse_reloc_line(line: &str) -> Option<Reloc> {
    let fields: Vec<&str> = line.split_whitespace().collect();
    let offset = u64::from_str_radix(fields.first()?, 16).ok()?;
    let kind = (*fields.get(2)?).to_string();
    let hex = |text: &str| u64::from_str_radix(text, 16).expect("hex addend") as i64;
    let (symbol, addend) = match fields.len() {
        // offset info type addend
        4 => (String::new(), hex(fields[3])),
        // offset info type value name sign addend
        7 => {
            let magnitude = hex(fields[6]);
            let addend = if fields[5] == "-" { magnitude.wrapping_neg() } else { magnitude };
            (fields[4].to_string(), addend)
        }
        _ => panic!("unexpected relocation line: {line}"),
    };
    Some(Reloc { offset, kind, symbol, addend })
}

/// `     6: 0000000000000010    16 OBJECT  GLOBAL DEFAULT     3 g0`
fn parse_symbol_line(line: &str) -> Option<Symbol> {
    let fields: Vec<&str> = line.split_whitespace().collect();
    fields.first()?.strip_suffix(':')?.parse::<u32>().ok()?;
    let value = u64::from_str_radix(fields.get(1)?, 16).ok()?;
    let size_text = fields.get(2)?;
    let size = match size_text.strip_prefix("0x") {
        Some(hex) => u64::from_str_radix(hex, 16).ok()?,
        None => size_text.parse().ok()?,
    };
    let place = match *fields.get(6)? {
        "UND" => Place::Undefined,
        "COM" => Place::Common,
        index => index.parse().map_or(Place::Other, Place::Section),
    };
    let name = fields.get(7).copied().unwrap_or("").to_string();
    Some(Symbol { name, value, size, place })
}

/// A deterministic pseudo-random number generator (xorshift64*).
pub struct Rng(u64);

impl Rng {
    pub fn new(seed: u64) -> Rng {
        Rng(seed.max(1))
    }

    pub fn next_u64(&mut self) -> u64 {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        self.0.wrapping_mul(0x2545_f491_4f6c_dd1d)
    }

    /// A number in `0..bound`.
    pub fn below(&mut self, bound: u64) -> u64 {
        (self.next_u64() >> 11) % bound
    }

    /// True `percent` times in a hundred.
    pub fn chance(&mut self, percent: u64) -> bool {
        self.below(100) < percent
    }

    pub fn pick<'a, T>(&mut self, items: &'a [T]) -> &'a T {
        &items[self.below(items.len() as u64) as usize]
    }
}

/// An LLVM type, as a tree: printable as LLVM text and internable in a
/// [`TypeTable`].
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum Ty {
    Int(u32),
    Half,
    BFloat,
    Float,
    Double,
    Fp128,
    X86Fp80,
    Ptr(u32),
    Vector(Box<Ty>, u32),
    Array(Box<Ty>, u64),
    Struct(Vec<Ty>, bool),
}

impl Ty {
    pub fn text(&self) -> String {
        match self {
            Ty::Int(bits) => format!("i{bits}"),
            Ty::Half => "half".into(),
            Ty::BFloat => "bfloat".into(),
            Ty::Float => "float".into(),
            Ty::Double => "double".into(),
            Ty::Fp128 => "fp128".into(),
            Ty::X86Fp80 => "x86_fp80".into(),
            Ty::Ptr(0) => "ptr".into(),
            Ty::Ptr(space) => format!("ptr addrspace({space})"),
            Ty::Vector(elem, len) => format!("<{len} x {}>", elem.text()),
            Ty::Array(elem, len) => format!("[{len} x {}]", elem.text()),
            Ty::Struct(fields, packed) => {
                let mut text = String::from(if *packed { "<{" } else { "{" });
                for (at, field) in fields.iter().enumerate() {
                    let _ = write!(text, "{} {}", if at == 0 { "" } else { "," }, field.text());
                }
                text.push_str(if *packed { " }>" } else { " }" });
                text
            }
        }
    }

    pub fn intern(&self, types: &mut TypeTable) -> TypeId {
        match self {
            Ty::Int(bits) => types.int(*bits),
            Ty::Half => Ok(TypeId::HALF),
            Ty::BFloat => Ok(TypeId::BFLOAT),
            Ty::Float => Ok(TypeId::FLOAT),
            Ty::Double => Ok(TypeId::DOUBLE),
            Ty::Fp128 => Ok(TypeId::FP128),
            Ty::X86Fp80 => Ok(TypeId::X86_FP80),
            Ty::Ptr(space) => types.ptr(*space),
            Ty::Vector(elem, len) => {
                let elem = elem.intern(types);
                types.vector(elem, *len)
            }
            Ty::Array(elem, len) => {
                let elem = elem.intern(types);
                types.array(elem, *len)
            }
            Ty::Struct(fields, packed) => {
                let fields: Vec<TypeId> = fields.iter().map(|field| field.intern(types)).collect();
                types.struct_type(&fields, *packed)
            }
        }
        .unwrap_or_else(|error| panic!("{}: {error}", self.text()))
    }

    pub fn is_float(&self) -> bool {
        matches!(self, Ty::Half | Ty::BFloat | Ty::Float | Ty::Double | Ty::Fp128 | Ty::X86Fp80)
    }

    /// Width in bits of an integer, float or pointer of address space 0.
    pub fn scalar_bits(&self) -> Option<u32> {
        Some(match self {
            Ty::Int(bits) => *bits,
            Ty::Half | Ty::BFloat => 16,
            Ty::Float => 32,
            Ty::Double | Ty::Ptr(0) => 64,
            Ty::X86Fp80 => 80,
            Ty::Fp128 => 128,
            _ => return None,
        })
    }
}

const INT_WIDTHS: [u32; 23] =
    [1, 2, 7, 8, 9, 15, 16, 17, 24, 31, 32, 33, 48, 63, 64, 65, 96, 127, 128, 129, 200, 256, 1000];
const FLOATS: [Ty; 6] = [Ty::Half, Ty::BFloat, Ty::Float, Ty::Double, Ty::Fp128, Ty::X86Fp80];
const ADDR_SPACES: [u32; 5] = [0, 1, 270, 271, 272];

fn boxed(ty: Ty) -> Box<Ty> {
    Box::new(ty)
}

/// Types chosen by hand for the layout rules they exercise.
pub fn edge_types() -> Vec<Ty> {
    let int = Ty::Int;
    let vector = |elem: Ty, len| Ty::Vector(boxed(elem), len);
    let array = |elem: Ty, len| Ty::Array(boxed(elem), len);
    let plain = |fields: &[Ty]| Ty::Struct(fields.to_vec(), false);
    let packed = |fields: &[Ty]| Ty::Struct(fields.to_vec(), true);
    let mut types: Vec<Ty> = INT_WIDTHS.iter().map(|&bits| int(bits)).collect();
    types.extend(FLOATS);
    types.extend(ADDR_SPACES.iter().map(|&space| Ty::Ptr(space)));
    types.extend([
        vector(int(1), 1),
        vector(int(1), 8),
        vector(int(1), 9),
        vector(int(3), 5),
        vector(int(24), 3),
        vector(int(24), 4),
        vector(int(8), 16),
        vector(int(8), 17),
        vector(int(65), 2),
        vector(int(128), 2),
        vector(Ty::Half, 3),
        vector(Ty::Float, 3),
        vector(Ty::Double, 4),
        vector(Ty::Double, 8),
        vector(Ty::X86Fp80, 2),
        vector(Ty::Fp128, 3),
        vector(Ty::Ptr(0), 3),
        vector(Ty::Ptr(270), 2),
        vector(Ty::Ptr(270), 3),
        array(int(64), 0),
        array(int(24), 3),
        array(int(8), 16),
        array(int(8), 17),
        array(Ty::X86Fp80, 2),
        array(plain(&[int(8), int(32)]), 3),
        array(vector(int(8), 3), 3),
        plain(&[]),
        packed(&[]),
        plain(&[int(8)]),
        packed(&[int(8), int(16)]),
        plain(&[int(8), int(64), int(8)]),
        packed(&[int(8), int(64), int(8)]),
        plain(&[int(8), int(128)]),
        plain(&[int(8), Ty::Fp128]),
        plain(&[int(8), Ty::X86Fp80, int(8)]),
        packed(&[int(8), Ty::X86Fp80, int(8)]),
        // WP-138: `{long, long double}`, `va_list`'s `__va_list_tag` and
        // its array, and pointers of x86-64's address spaces 270..272.
        plain(&[int(64), Ty::X86Fp80]),
        plain(&[int(32), int(32), Ty::Ptr(0), Ty::Ptr(0)]),
        array(plain(&[int(32), int(32), Ty::Ptr(0), Ty::Ptr(0)]), 1),
        plain(&[int(8), Ty::Ptr(270), Ty::Ptr(271), int(8), Ty::Ptr(272)]),
        plain(&[int(8), vector(int(32), 4)]),
        packed(&[int(8), vector(int(64), 2)]),
        plain(&[vector(int(8), 3), int(8)]),
        plain(&[array(int(64), 0), int(8)]),
        plain(&[int(8), array(int(64), 0)]),
        plain(&[int(32), plain(&[]), int(32)]),
        plain(&[int(65), int(8)]),
        packed(&[int(65), int(8)]),
        plain(&[plain(&[int(8), int(64)]), int(8)]),
        plain(&[packed(&[int(8), int(64)]), int(8)]),
        packed(&[plain(&[int(8), int(64)]), int(8)]),
        plain(&[int(1), Ty::Ptr(270), int(1), Ty::Ptr(0)]),
        plain(&[Ty::Half, Ty::Double, Ty::Float, Ty::Fp128, Ty::BFloat]),
    ]);
    types
}

fn random_scalar(rng: &mut Rng) -> Ty {
    match rng.below(100) {
        0..=29 => Ty::Int(*rng.pick(&INT_WIDTHS)),
        30..=54 => Ty::Int(rng.below(130) as u32 + 1),
        55..=79 => rng.pick(&FLOATS).clone(),
        80..=91 => Ty::Ptr(0),
        _ => Ty::Ptr(*rng.pick(&ADDR_SPACES)),
    }
}

/// A random type nested at most `depth` levels.
pub fn random_type(rng: &mut Rng, depth: u32) -> Ty {
    if depth == 0 || rng.chance(30) {
        return random_scalar(rng);
    }
    match rng.below(10) {
        0..=2 => {
            let len = if rng.chance(15) { 16 + rng.below(2) } else { 1 + rng.below(9) };
            Ty::Vector(boxed(random_scalar(rng)), len as u32)
        }
        3..=5 => Ty::Array(boxed(random_type(rng, depth - 1)), rng.below(6)),
        _ => {
            let count = rng.below(7);
            let fields = (0..count).map(|_| random_type(rng, depth - 1)).collect();
            Ty::Struct(fields, rng.chance(30))
        }
    }
}

/// The hand-picked types followed by random ones, `count` in all.
pub fn type_corpus(seed: u64, count: usize) -> Vec<Ty> {
    let mut rng = Rng::new(seed);
    let mut types = edge_types();
    while types.len() < count {
        let ty = random_type(&mut rng, 3);
        if !types.contains(&ty) {
            types.push(ty);
        }
    }
    types
}
