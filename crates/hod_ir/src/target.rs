//! The target description (`spec/ir.md` §2.1): architecture, OS/ABI and the
//! parsed data layout.
//!
//! [`DataLayout`] follows the LLVM Language Reference, "Data Layout", with
//! the grammar LLVM 22 accepts (which is stricter than LLVM 19's, and adds
//! the pointer flags `e` and `u`). The alignment rules are those of LLVM's
//! `DataLayout` class; `crates/hod_ir/tests/layout_diff.rs` checks them
//! against `llc`.

use std::fmt;

use crate::error::{IrErrorKind, IrResult, err};

/// kiln's data-layout string for `aarch64-unknown-linux-gnu` at clang 20 and
/// later. The layout hod uses for the target.
pub const AARCH64_LINUX_DATA_LAYOUT: &str =
    "e-m:e-p270:32:32-p271:32:32-p272:64:64-i8:8:32-i16:16:32-i64:64-i128:128-n32:64-S128-Fn32";

/// The same target's string at clang 19, which has no `p270`/`p271`/`p272`
/// entries. It differs from [`AARCH64_LINUX_DATA_LAYOUT`] only for pointers
/// outside address space 0.
pub const AARCH64_LINUX_DATA_LAYOUT_LLVM19: &str =
    "e-m:e-i8:8:32-i16:16:32-i64:64-i128:128-n32:64-S128-Fn32";

/// kiln's data-layout string for `x86_64-unknown-linux-gnu` (clang 19 to 22).
pub const X86_64_LINUX_DATA_LAYOUT: &str =
    "e-m:e-p270:32:32-p271:32:32-p272:64:64-i64:64-i128:128-f80:128-n8:16:32:64-S128";

/// The largest address space number (24 bits).
pub const MAX_ADDR_SPACE: u32 = (1 << 24) - 1;

/// An ABI and a preferred alignment, in bytes. Both are powers of two and
/// `pref >= abi`.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct AlignPair {
    /// ABI alignment in bytes.
    pub abi: u64,
    /// Preferred alignment in bytes.
    pub pref: u64,
}

/// The alignment of integers, floats or vectors of one bit width.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct PrimSpec {
    /// Width in bits.
    pub bits: u32,
    /// Alignments in bytes.
    pub align: AlignPair,
}

/// The layout of pointers in one address space.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct PointerSpec {
    /// Address space.
    pub addr_space: u32,
    /// Pointer width in bits.
    pub bits: u32,
    /// Alignments in bytes.
    pub align: AlignPair,
    /// Width in bits of the index type used for address arithmetic.
    pub index_bits: u32,
}

/// Symbol mangling (`m:`). Kept for completeness; no effect on layout.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum Mangling {
    /// No `m:` specification.
    None,
    /// `m:e`.
    Elf,
    /// `m:l`.
    Goff,
    /// `m:o`.
    MachO,
    /// `m:m`.
    Mips,
    /// `m:w`.
    WinCoff,
    /// `m:x`.
    WinCoffX86,
    /// `m:a`.
    XCoff,
}

/// What a function-pointer alignment (`F`) is relative to.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum FunctionPtrAlignKind {
    /// `Fi`: independent of the function's alignment.
    Independent,
    /// `Fn`: a multiple of the function's alignment.
    MultipleOfFunctionAlign,
}

/// A parsed LLVM data-layout string.
///
/// Alignments are in bytes. The integer, float, vector and pointer tables
/// start from LLVM's defaults and are sorted by width (pointers by address
/// space); a specification replaces the entry of the same width.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct DataLayout {
    /// `E`: big-endian. (`e`, the default, is little-endian.)
    pub big_endian: bool,
    /// `m:`.
    pub mangling: Mangling,
    /// `S`: natural stack alignment in bytes, when specified.
    pub stack_align: Option<u64>,
    /// `P`: address space of functions.
    pub program_addr_space: u32,
    /// `A`: address space of `alloca`.
    pub alloca_addr_space: u32,
    /// `G`: default address space of global variables.
    pub globals_addr_space: u32,
    /// `F`: function-pointer alignment kind and bytes, when specified.
    pub function_ptr_align: Option<(FunctionPtrAlignKind, u64)>,
    /// `n`: native integer widths in bits, in the order given.
    pub native_int_widths: Vec<u32>,
    /// `ni:`: non-integral address spaces, in the order given.
    pub non_integral_addr_spaces: Vec<u32>,
    /// `a`: aggregate alignment. An ABI alignment of 1 is "none".
    pub aggregate_align: AlignPair,
    ints: Vec<PrimSpec>,
    floats: Vec<PrimSpec>,
    vectors: Vec<PrimSpec>,
    pointers: Vec<PointerSpec>,
}

const fn prim(bits: u32, abi: u64, pref: u64) -> PrimSpec {
    PrimSpec { bits, align: AlignPair { abi, pref } }
}

impl Default for DataLayout {
    /// LLVM's defaults: the layout of the empty string.
    fn default() -> Self {
        DataLayout {
            big_endian: false,
            mangling: Mangling::None,
            stack_align: None,
            program_addr_space: 0,
            alloca_addr_space: 0,
            globals_addr_space: 0,
            function_ptr_align: None,
            native_int_widths: Vec::new(),
            non_integral_addr_spaces: Vec::new(),
            aggregate_align: AlignPair { abi: 1, pref: 8 },
            ints: vec![
                prim(1, 1, 1),
                prim(8, 1, 1),
                prim(16, 2, 2),
                prim(32, 4, 4),
                prim(64, 4, 8),
            ],
            floats: vec![prim(16, 2, 2), prim(32, 4, 4), prim(64, 8, 8), prim(128, 16, 16)],
            vectors: vec![prim(64, 8, 8), prim(128, 16, 16)],
            pointers: vec![PointerSpec {
                addr_space: 0,
                bits: 64,
                align: AlignPair { abi: 8, pref: 8 },
                index_bits: 64,
            }],
        }
    }
}

/// The mangling mode named in `m:<mangling>`.
fn mangling(spec: &str, name: &str) -> IrResult<Mangling> {
    Ok(match name {
        "e" => Mangling::Elf,
        "l" => Mangling::Goff,
        "o" => Mangling::MachO,
        "m" => Mangling::Mips,
        "w" => Mangling::WinCoff,
        "x" => Mangling::WinCoffX86,
        "a" => Mangling::XCoff,
        _ => return bad(spec, "unknown mangling mode"),
    })
}

fn bad<T>(spec: &str, why: &str) -> IrResult<T> {
    err(IrErrorKind::InvalidDataLayout, format!("data layout specification `{spec}`: {why}"))
}

/// A decimal number of at most `max`. LLVM accepts digits only.
fn number(spec: &str, text: &str, what: &str, max: u32) -> IrResult<u32> {
    if text.is_empty() {
        return bad(spec, &format!("{what} component cannot be empty"));
    }
    if !text.bytes().all(|b| b.is_ascii_digit()) {
        return bad(spec, &format!("{what} is not a number"));
    }
    match text.parse::<u32>() {
        Ok(n) if n <= max => Ok(n),
        _ => bad(spec, &format!("{what} is out of range")),
    }
}

const MAX_SIZE_BITS: u32 = (1 << 24) - 1;

/// A non-zero 24-bit size in bits.
fn size_bits(spec: &str, text: &str, what: &str) -> IrResult<u32> {
    match number(spec, text, what, MAX_SIZE_BITS)? {
        0 => bad(spec, &format!("{what} must be non-zero")),
        n => Ok(n),
    }
}

/// An alignment given in bits: a 16-bit power of two times the byte width.
/// Returns bytes; zero is returned as zero when `allow_zero`.
fn align_bits(spec: &str, text: &str, what: &str, allow_zero: bool) -> IrResult<u64> {
    let bits = number(spec, text, what, u32::from(u16::MAX))?;
    if bits == 0 {
        return if allow_zero { Ok(0) } else { bad(spec, &format!("{what} must be non-zero")) };
    }
    if bits % 8 != 0 || !(bits / 8).is_power_of_two() {
        return bad(spec, &format!("{what} must be a power of two times the byte width"));
    }
    Ok(u64::from(bits / 8))
}

/// `<abi>[:<pref>]` from the components after the size.
fn align_pair(spec: &str, parts: &[&str], abi_may_be_zero: bool) -> IrResult<AlignPair> {
    let abi = align_bits(spec, parts[0], "ABI alignment", abi_may_be_zero)?;
    let pref = match parts.get(1) {
        Some(text) => align_bits(spec, text, "preferred alignment", false)?,
        None => abi,
    };
    if pref < abi {
        return bad(spec, "preferred alignment cannot be less than the ABI alignment");
    }
    // A zero ABI alignment (aggregates only) means "none": one byte.
    Ok(AlignPair { abi: abi.max(1), pref: pref.max(1) })
}

fn set_prim(table: &mut Vec<PrimSpec>, spec: PrimSpec) {
    match table.binary_search_by_key(&spec.bits, |s| s.bits) {
        Ok(at) => table[at] = spec,
        Err(at) => table.insert(at, spec),
    }
}

impl DataLayout {
    /// Parses a data-layout string. The empty string is LLVM's defaults.
    ///
    /// Fails with `InvalidDataLayout` on anything LLVM 22 rejects.
    pub fn parse(text: &str) -> IrResult<DataLayout> {
        let mut layout = DataLayout::default();
        if text.is_empty() {
            return Ok(layout);
        }
        for spec in text.split('-') {
            layout.parse_spec(spec)?;
        }
        Ok(layout)
    }

    fn parse_spec(&mut self, spec: &str) -> IrResult<()> {
        let Some(first) = spec.chars().next() else {
            return bad(spec, "empty specification is not allowed");
        };
        if let Some(rest) = spec.strip_prefix("ni") {
            return self.parse_non_integral(spec, rest);
        }
        let rest = &spec[first.len_utf8()..];
        let parts: Vec<&str> = rest.split(':').collect();
        match first {
            'e' | 'E' => {
                if !rest.is_empty() {
                    return bad(spec, "must be just 'e' or 'E'");
                }
                self.big_endian = first == 'E';
            }
            'i' | 'f' | 'v' => {
                if !(2..=3).contains(&parts.len()) {
                    return bad(spec, "must be of the form <kind><size>:<abi>[:<pref>]");
                }
                let bits = size_bits(spec, parts[0], "size")?;
                let align = align_pair(spec, &parts[1..], false)?;
                if first == 'i' && bits == 8 && align.abi != 1 {
                    return bad(spec, "i8 must be 8-bit aligned");
                }
                let table = match first {
                    'i' => &mut self.ints,
                    'f' => &mut self.floats,
                    _ => &mut self.vectors,
                };
                set_prim(table, PrimSpec { bits, align });
            }
            'a' => {
                if !(2..=3).contains(&parts.len()) {
                    return bad(spec, "must be of the form a:<abi>[:<pref>]");
                }
                if !parts[0].is_empty() && number(spec, parts[0], "size", MAX_SIZE_BITS)? != 0 {
                    return bad(spec, "size must be zero");
                }
                self.aggregate_align = align_pair(spec, &parts[1..], true)?;
            }
            'p' => self.parse_pointer(spec, &parts)?,
            'n' => {
                for text in parts {
                    self.native_int_widths.push(size_bits(spec, text, "size")?);
                }
            }
            'S' => {
                if parts.len() != 1 {
                    return bad(spec, "must be of the form S<size>");
                }
                let bytes = align_bits(spec, parts[0], "stack natural alignment", false)?;
                self.stack_align = Some(bytes);
            }
            'F' => {
                let mut chars = rest.chars();
                let kind = match chars.next() {
                    Some('i') => FunctionPtrAlignKind::Independent,
                    Some('n') => FunctionPtrAlignKind::MultipleOfFunctionAlign,
                    Some(_) => return bad(spec, "unknown function pointer alignment type"),
                    None => return bad(spec, "must be of the form F<type><abi>"),
                };
                let bytes = align_bits(spec, chars.as_str(), "ABI alignment", false)?;
                self.function_ptr_align = Some((kind, bytes));
            }
            'P' | 'A' | 'G' => {
                if parts.len() != 1 {
                    return bad(spec, "must be of the form <kind><address space>");
                }
                let space = number(spec, parts[0], "address space", MAX_ADDR_SPACE)?;
                match first {
                    'P' => self.program_addr_space = space,
                    'A' => self.alloca_addr_space = space,
                    _ => self.globals_addr_space = space,
                }
            }
            'm' => {
                if parts.len() != 2 || !parts[0].is_empty() {
                    return bad(spec, "must be of the form m:<mangling>");
                }
                self.mangling = mangling(spec, parts[1])?;
            }
            // `s<size>:<abi>` is obsolete; LLVM still accepts and ignores it.
            's' => {}
            _ => return bad(spec, "unknown specifier"),
        }
        Ok(())
    }

    /// `ni:<as>[:<as>]...`, given what follows `ni`. (`n` followed by `i` is
    /// not a native width.)
    fn parse_non_integral(&mut self, spec: &str, rest: &str) -> IrResult<()> {
        let Some(list) = rest.strip_prefix(':') else {
            return bad(spec, "must be of the form ni:<address space>[:<address space>]...");
        };
        for text in list.split(':') {
            let space = number(spec, text, "address space", MAX_ADDR_SPACE)?;
            if space == 0 {
                return bad(spec, "address space 0 cannot be non-integral");
            }
            self.non_integral_addr_spaces.push(space);
        }
        Ok(())
    }

    /// `p[<flags>][<as>]:<size>:<abi>[:<pref>[:<idx>]]`. The flags `e`
    /// (external state) and `u` (unstable representation) of LLVM 21 and
    /// later are accepted and not kept: they do not change layout.
    fn parse_pointer(&mut self, spec: &str, parts: &[&str]) -> IrResult<()> {
        if !(3..=5).contains(&parts.len()) {
            return bad(spec, "must be of the form p[<n>]:<size>:<abi>[:<pref>[:<idx>]]");
        }
        let space_text = parts[0].trim_start_matches(['e', 'u']);
        let addr_space = if space_text.is_empty() {
            0
        } else {
            number(spec, space_text, "address space", MAX_ADDR_SPACE)?
        };
        let bits = size_bits(spec, parts[1], "pointer size")?;
        let align = align_pair(spec, &parts[2..parts.len().min(4)], false)?;
        let index_bits = match parts.get(4) {
            Some(text) => size_bits(spec, text, "index size")?,
            None => bits,
        };
        if index_bits > bits {
            return bad(spec, "index size cannot be larger than the pointer size");
        }
        let pointer = PointerSpec { addr_space, bits, align, index_bits };
        match self.pointers.binary_search_by_key(&addr_space, |p| p.addr_space) {
            Ok(at) => self.pointers[at] = pointer,
            Err(at) => self.pointers.insert(at, pointer),
        }
        Ok(())
    }

    /// The integer alignment table, sorted by width.
    pub fn int_specs(&self) -> &[PrimSpec] {
        &self.ints
    }

    /// The float alignment table, sorted by width.
    pub fn float_specs(&self) -> &[PrimSpec] {
        &self.floats
    }

    /// The vector alignment table, sorted by width.
    pub fn vector_specs(&self) -> &[PrimSpec] {
        &self.vectors
    }

    /// The pointer table, sorted by address space. Always has address
    /// space 0.
    pub fn pointer_specs(&self) -> &[PointerSpec] {
        &self.pointers
    }

    /// The alignment of `iN`.
    ///
    /// LLVM's rule: the entry of that width; otherwise the entry of the next
    /// larger specified width; otherwise (wider than every entry) the entry
    /// of the largest specified width.
    pub fn int_align(&self, bits: u32) -> AlignPair {
        let at = self.ints.partition_point(|s| s.bits < bits);
        self.ints[at.min(self.ints.len() - 1)].align
    }

    /// The alignment of a floating-point type `bits` wide.
    ///
    /// LLVM's rule: the entry of exactly that width; otherwise the first
    /// power of two not less than the width in bytes (which is how
    /// `x86_fp80` gets 16 on a target with no `f80` entry).
    pub fn float_align(&self, bits: u32) -> AlignPair {
        if let Ok(at) = self.floats.binary_search_by_key(&bits, |s| s.bits) {
            return self.floats[at].align;
        }
        let natural = u64::from(bits / 8).next_power_of_two();
        AlignPair { abi: natural, pref: natural }
    }

    /// The alignment of a vector `bits` wide whose store size is
    /// `store_size` bytes (for a scalable vector, the minimum of each).
    ///
    /// LLVM's rule: the entry of exactly that width; otherwise the first
    /// power of two not less than the store size. `None` when that power of
    /// two is not representable.
    pub fn vector_align(&self, bits: u64, store_size: u64) -> Option<AlignPair> {
        if let Ok(narrow) = u32::try_from(bits)
            && let Ok(at) = self.vectors.binary_search_by_key(&narrow, |s| s.bits)
        {
            return Some(self.vectors[at].align);
        }
        let natural = store_size.checked_next_power_of_two()?;
        Some(AlignPair { abi: natural, pref: natural })
    }

    /// The layout of pointers in `addr_space`: its entry, or address
    /// space 0's when it has none.
    pub fn pointer(&self, addr_space: u32) -> PointerSpec {
        match self.pointers.binary_search_by_key(&addr_space, |p| p.addr_space) {
            Ok(at) => self.pointers[at],
            Err(_) => PointerSpec { addr_space, ..self.pointers[0] },
        }
    }

    /// The first difference from `other` that changes what hod generates,
    /// or `None` when the two layouts are interchangeable for hod.
    ///
    /// Compared: endianness; the address-space-0 pointer; the effective
    /// alignment (ABI and preferred) of every integer width, of the float
    /// widths 16, 32, 64, 80 and 128, and of every vector width either
    /// layout names; the aggregate alignment; the stack alignment; the
    /// program, alloca and globals address spaces. Preferred alignments are
    /// compared because they set the alignment of a variable that has none.
    ///
    /// Not compared, because hod's output does not depend on them: pointers
    /// in other address spaces (only address space 0 is supported, §3),
    /// mangling, native integer widths, non-integral address spaces and the
    /// function-pointer alignment.
    pub fn layout_difference(&self, other: &DataLayout) -> Option<String> {
        if self.big_endian != other.big_endian {
            return Some("endianness".into());
        }
        if self.pointer(0) != other.pointer(0) {
            return Some("pointer size or alignment".into());
        }
        // Integer alignment is a step function of the width with steps at
        // the widths either table names.
        let widest = self.ints.iter().chain(&other.ints).map(|s| s.bits).max().unwrap_or(0);
        let widths = self.ints.iter().chain(&other.ints).map(|s| s.bits).chain([widest + 1]);
        for bits in widths {
            if self.int_align(bits) != other.int_align(bits) {
                return Some(format!("alignment of i{bits}"));
            }
        }
        let float_widths = self.floats.iter().chain(&other.floats).map(|s| s.bits);
        for bits in float_widths.chain([16, 32, 64, 80, 128]) {
            if self.float_align(bits) != other.float_align(bits) {
                return Some(format!("alignment of {bits}-bit floating point"));
            }
        }
        for bits in self.vectors.iter().chain(&other.vectors).map(|s| s.bits) {
            let wide = u64::from(bits);
            let store = wide.div_ceil(8);
            if self.vector_align(wide, store) != other.vector_align(wide, store) {
                return Some(format!("alignment of {bits}-bit vectors"));
            }
        }
        if self.aggregate_align != other.aggregate_align {
            return Some("aggregate alignment".into());
        }
        if self.stack_align != other.stack_align {
            return Some("stack alignment".into());
        }
        if (self.program_addr_space, self.alloca_addr_space, self.globals_addr_space)
            != (other.program_addr_space, other.alloca_addr_space, other.globals_addr_space)
        {
            return Some("program, alloca or globals address space".into());
        }
        None
    }
}

impl fmt::Display for DataLayout {
    /// A data-layout string that parses back to an equal `DataLayout`.
    /// Every table entry is written, defaults included.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(if self.big_endian { "E" } else { "e" })?;
        let mangling = match self.mangling {
            Mangling::None => "",
            Mangling::Elf => "e",
            Mangling::Goff => "l",
            Mangling::MachO => "o",
            Mangling::Mips => "m",
            Mangling::WinCoff => "w",
            Mangling::WinCoffX86 => "x",
            Mangling::XCoff => "a",
        };
        if !mangling.is_empty() {
            write!(f, "-m:{mangling}")?;
        }
        for p in &self.pointers {
            let AlignPair { abi, pref } = p.align;
            write!(f, "-p{}:{}:{}:{}:{}", p.addr_space, p.bits, abi * 8, pref * 8, p.index_bits)?;
        }
        for (letter, table) in [('i', &self.ints), ('f', &self.floats), ('v', &self.vectors)] {
            for s in table {
                write!(f, "-{letter}{}:{}:{}", s.bits, s.align.abi * 8, s.align.pref * 8)?;
            }
        }
        let aggregate_abi =
            if self.aggregate_align.abi == 1 { 0 } else { self.aggregate_align.abi };
        write!(f, "-a:{}:{}", aggregate_abi * 8, self.aggregate_align.pref * 8)?;
        for (i, bits) in self.native_int_widths.iter().enumerate() {
            write!(f, "{}{bits}", if i == 0 { "-n" } else { ":" })?;
        }
        for (i, space) in self.non_integral_addr_spaces.iter().enumerate() {
            write!(f, "{}{space}", if i == 0 { "-ni:" } else { ":" })?;
        }
        if let Some(bytes) = self.stack_align {
            write!(f, "-S{}", bytes * 8)?;
        }
        for (letter, space) in [
            ('P', self.program_addr_space),
            ('A', self.alloca_addr_space),
            ('G', self.globals_addr_space),
        ] {
            if space != 0 {
                write!(f, "-{letter}{space}")?;
            }
        }
        if let Some((kind, bytes)) = self.function_ptr_align {
            let kind = match kind {
                FunctionPtrAlignKind::Independent => 'i',
                FunctionPtrAlignKind::MultipleOfFunctionAlign => 'n',
            };
            write!(f, "-F{kind}{}", bytes * 8)?;
        }
        Ok(())
    }
}

/// A supported architecture.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum Arch {
    /// AArch64 (little-endian, LP64).
    Aarch64,
    /// x86-64 (LP64).
    X86_64,
}

/// A supported OS and ABI.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum Os {
    /// Linux with the GNU C library ABI (`*-linux-gnu`).
    LinuxGnu,
}

/// What a module is compiled for: an architecture, an OS/ABI, and the data
/// layout that follows from them (§2.1).
///
/// Both supported targets are little-endian with 64-bit pointers in address
/// space 0; code in this crate that writes bytes relies on that.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct TargetSpec {
    arch: Arch,
    os: Os,
    layout: DataLayout,
}

impl TargetSpec {
    /// The description of `arch` on `os`.
    pub fn new(arch: Arch, os: Os) -> TargetSpec {
        let Os::LinuxGnu = os;
        let text = match arch {
            Arch::Aarch64 => AARCH64_LINUX_DATA_LAYOUT,
            Arch::X86_64 => X86_64_LINUX_DATA_LAYOUT,
        };
        let layout = DataLayout::parse(text).expect("the built-in data layouts parse");
        TargetSpec { arch, os, layout }
    }

    /// `aarch64-unknown-linux-gnu`.
    pub fn aarch64_linux_gnu() -> TargetSpec {
        TargetSpec::new(Arch::Aarch64, Os::LinuxGnu)
    }

    /// `x86_64-unknown-linux-gnu`.
    pub fn x86_64_linux_gnu() -> TargetSpec {
        TargetSpec::new(Arch::X86_64, Os::LinuxGnu)
    }

    /// The description for an LLVM target triple: `aarch64` (or `arm64`) or
    /// `x86_64` (or `amd64`), any vendor, `linux`, and the `gnu` environment
    /// (or none). Anything else is `UnsupportedTarget`.
    pub fn from_triple(triple: &str) -> IrResult<TargetSpec> {
        let unsupported =
            || err(IrErrorKind::UnsupportedTarget, format!("unsupported target triple `{triple}`"));
        let parts: Vec<&str> = triple.split('-').collect();
        let arch = match parts[0] {
            "aarch64" | "arm64" => Arch::Aarch64,
            "x86_64" | "amd64" => Arch::X86_64,
            _ => return unsupported(),
        };
        // `arch-linux[-gnu]` or `arch-vendor-linux[-gnu]`.
        let rest = match &parts[1..] {
            [os, rest @ ..] if *os == "linux" => rest,
            [_vendor, os, rest @ ..] if *os == "linux" => rest,
            _ => return unsupported(),
        };
        match rest {
            [] | ["gnu"] => Ok(TargetSpec::new(arch, Os::LinuxGnu)),
            _ => unsupported(),
        }
    }

    /// The architecture.
    pub fn arch(&self) -> Arch {
        self.arch
    }

    /// The OS and ABI.
    pub fn os(&self) -> Os {
        self.os
    }

    /// The canonical LLVM triple.
    pub fn triple(&self) -> &'static str {
        match (self.arch, self.os) {
            (Arch::Aarch64, Os::LinuxGnu) => "aarch64-unknown-linux-gnu",
            (Arch::X86_64, Os::LinuxGnu) => "x86_64-unknown-linux-gnu",
        }
    }

    /// The data-layout string, as clang 22 writes it for this target.
    pub fn data_layout_str(&self) -> &'static str {
        match (self.arch, self.os) {
            (Arch::Aarch64, Os::LinuxGnu) => AARCH64_LINUX_DATA_LAYOUT,
            (Arch::X86_64, Os::LinuxGnu) => X86_64_LINUX_DATA_LAYOUT,
        }
    }

    /// The parsed data layout.
    pub fn data_layout(&self) -> &DataLayout {
        &self.layout
    }

    /// Checks a module's `target datalayout` string against this target.
    ///
    /// `InvalidDataLayout` when it does not parse; `DataLayoutMismatch`
    /// when it differs in a way that changes layout
    /// ([`DataLayout::layout_difference`]). A module with no data-layout
    /// line is not checked by this function: like `llc`, hod then uses the
    /// target's layout.
    pub fn check_data_layout(&self, module_layout: &str) -> IrResult<()> {
        let parsed = DataLayout::parse(module_layout)?;
        match self.layout.layout_difference(&parsed) {
            None => Ok(()),
            Some(what) => err(
                IrErrorKind::DataLayoutMismatch,
                format!("data layout `{module_layout}` differs from {}'s in {what}", self.triple()),
            ),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(text: &str) -> DataLayout {
        DataLayout::parse(text).unwrap_or_else(|e| panic!("{text}: {e}"))
    }

    fn pair(abi: u64, pref: u64) -> AlignPair {
        AlignPair { abi, pref }
    }

    /// §2.1: kiln's aarch64 string (clang 20+ form), field by field.
    #[test]
    fn aarch64_layout_fields() {
        let dl = parse(AARCH64_LINUX_DATA_LAYOUT);
        assert!(!dl.big_endian);
        assert_eq!(dl.mangling, Mangling::Elf);
        assert_eq!(dl.stack_align, Some(16));
        assert_eq!(dl.native_int_widths, [32, 64]);
        assert_eq!(dl.function_ptr_align, Some((FunctionPtrAlignKind::MultipleOfFunctionAlign, 4)));
        assert_eq!(dl.aggregate_align, pair(1, 8));
        assert_eq!(
            dl.int_specs(),
            [
                prim(1, 1, 1),
                prim(8, 1, 4),
                prim(16, 2, 4),
                prim(32, 4, 4),
                prim(64, 8, 8),
                prim(128, 16, 16)
            ]
        );
        assert_eq!(dl.pointer(0).bits, 64);
        assert_eq!(dl.pointer(0).align, pair(8, 8));
        assert_eq!(dl.pointer(0).index_bits, 64);
        assert_eq!(dl.pointer(270).bits, 32);
        assert_eq!(dl.pointer(271).align, pair(4, 4));
        assert_eq!(dl.pointer(272).bits, 64);
        // An address space with no entry is laid out like address space 0.
        assert_eq!(dl.pointer(5), PointerSpec { addr_space: 5, ..dl.pointer(0) });
        assert_eq!(dl.pointer_specs().len(), 4);
    }

    /// §2.1: kiln's x86-64 string, field by field.
    #[test]
    fn x86_64_layout_fields() {
        let dl = parse(X86_64_LINUX_DATA_LAYOUT);
        assert_eq!(dl.native_int_widths, [8, 16, 32, 64]);
        assert_eq!(dl.stack_align, Some(16));
        assert_eq!(dl.function_ptr_align, None);
        assert_eq!(dl.int_align(8), pair(1, 1));
        assert_eq!(dl.int_align(64), pair(8, 8));
        assert_eq!(dl.int_align(128), pair(16, 16));
        assert_eq!(dl.float_align(80), pair(16, 16));
        assert_eq!(dl.float_specs().len(), 5);
        assert_eq!(dl.vector_specs(), [prim(64, 8, 8), prim(128, 16, 16)]);
    }

    /// §2.1: the clang 19 aarch64 string parses and differs from the
    /// clang 20+ string only outside address space 0.
    #[test]
    fn aarch64_llvm19_layout_is_compatible() {
        let old = parse(AARCH64_LINUX_DATA_LAYOUT_LLVM19);
        let new = parse(AARCH64_LINUX_DATA_LAYOUT);
        assert_ne!(old, new);
        assert_eq!(old.pointer(270).bits, 64);
        assert_eq!(new.layout_difference(&old), None);
        let target = TargetSpec::aarch64_linux_gnu();
        target.check_data_layout(AARCH64_LINUX_DATA_LAYOUT_LLVM19).unwrap();
        target.check_data_layout(AARCH64_LINUX_DATA_LAYOUT).unwrap();
    }

    /// §2.1: LLVM's integer alignment rule for widths with no entry.
    #[test]
    fn integer_alignment_uses_next_larger_then_largest() {
        let dl = parse(AARCH64_LINUX_DATA_LAYOUT);
        assert_eq!(dl.int_align(1), pair(1, 1));
        assert_eq!(dl.int_align(2), pair(1, 4));
        assert_eq!(dl.int_align(9), pair(2, 4));
        assert_eq!(dl.int_align(24), pair(4, 4));
        assert_eq!(dl.int_align(33), pair(8, 8));
        assert_eq!(dl.int_align(65), pair(16, 16));
        assert_eq!(dl.int_align(129), pair(16, 16));
        assert_eq!(dl.int_align(1 << 23), pair(16, 16));
        // The defaults stop at i64, whose ABI alignment is 4.
        let defaults = parse("");
        assert_eq!(defaults.int_align(64), pair(4, 8));
        assert_eq!(defaults.int_align(128), pair(4, 8));
    }

    /// §2.1: float and vector alignment fall back to the natural alignment.
    #[test]
    fn float_and_vector_alignment_fallbacks() {
        let dl = parse(AARCH64_LINUX_DATA_LAYOUT);
        assert_eq!(dl.float_align(16), pair(2, 2));
        assert_eq!(dl.float_align(80), pair(16, 16));
        assert_eq!(dl.float_align(128), pair(16, 16));
        assert_eq!(dl.vector_align(64, 8), Some(pair(8, 8)));
        assert_eq!(dl.vector_align(128, 16), Some(pair(16, 16)));
        assert_eq!(dl.vector_align(72, 9), Some(pair(16, 16)));
        assert_eq!(dl.vector_align(256, 32), Some(pair(32, 32)));
        assert_eq!(dl.vector_align(3, 1), Some(pair(1, 1)));
        assert_eq!(dl.vector_align(u64::MAX, u64::MAX / 2 + 2), None);
        let custom = parse("v256:128-v64:64:128");
        assert_eq!(custom.vector_align(256, 32), Some(pair(16, 16)));
        assert_eq!(custom.vector_align(64, 8), Some(pair(8, 16)));
    }

    /// §2.1: every specification kind, and the forms LLVM 22 prints.
    #[test]
    fn every_specification_kind_parses() {
        let dl = parse(
            "E-m:o-p:32:32:64:16-pe1:64:64-pu2:64:64:64:32-peu3:16:16-i24:32:64-f80:128-v256:256\
             -a:0:64-n8:16-ni:1:2-S64-P1-A2-G3-Fi8-s32:32",
        );
        assert!(dl.big_endian);
        assert_eq!(dl.mangling, Mangling::MachO);
        assert_eq!(
            dl.pointer(0),
            PointerSpec { addr_space: 0, bits: 32, align: pair(4, 8), index_bits: 16 }
        );
        assert_eq!(dl.pointer(1).bits, 64);
        assert_eq!(dl.pointer(2).index_bits, 32);
        assert_eq!(dl.pointer(3).bits, 16);
        assert_eq!(dl.int_align(24), pair(4, 8));
        assert_eq!(dl.int_align(17), pair(4, 8));
        assert_eq!(dl.native_int_widths, [8, 16]);
        assert_eq!(dl.non_integral_addr_spaces, [1, 2]);
        assert_eq!(dl.stack_align, Some(8));
        assert_eq!((dl.program_addr_space, dl.alloca_addr_space, dl.globals_addr_space), (1, 2, 3));
        assert_eq!(dl.function_ptr_align, Some((FunctionPtrAlignKind::Independent, 1)));
        assert_eq!(parse("a:128:256").aggregate_align, pair(16, 32));
        assert_eq!(parse("a0:0").aggregate_align, pair(1, 1));
        assert_eq!(parse("a:8").aggregate_align, pair(1, 1));
        // A later specification replaces an earlier one.
        assert_eq!(parse("i32:32-i32:64").int_align(32), pair(8, 8));
        assert_eq!(parse("p1:32:32-p1:64:64").pointer(1).bits, 64);
        assert!(!parse("E-e").big_endian);
        for mangling in ["e", "l", "o", "m", "w", "x", "a"] {
            assert_ne!(parse(&format!("m:{mangling}")).mangling, Mangling::None);
        }
    }

    /// §2.1 `InvalidDataLayout`: the strings LLVM 22 rejects.
    #[test]
    fn malformed_layouts_are_errors() {
        for text in [
            "e-",
            "-e",
            "e--m:e",
            "ex",
            "z",
            "x:1",
            ":8",
            "p:64",
            "p:64:64:64:64:64",
            "p:64:64:64:128",
            "p:64:64:64:0",
            "p0:0:64",
            "p:64:0",
            "p:64:24",
            "p16777216:64:64",
            "p-1:64:64",
            "i8:16",
            "i64",
            "i64:0",
            "i64:64:32",
            "i64:64:64:64",
            "i0:8",
            "i16777216:8",
            "i:8",
            "i32:",
            "i32:8:",
            "i32:65536",
            "i32:32:65536",
            "i32:32:0",
            "i+8:8",
            "i 8:8",
            "i0x8:8",
            "i32:12",
            "a",
            "a8:0",
            "a:16:8",
            "a:0:0",
            "n",
            "n0",
            "n8:",
            "S",
            "S0",
            "S24",
            "S65536",
            "F",
            "Fx8",
            "Fn0",
            "Fn",
            "P16777216",
            "P",
            "A1:2",
            "m",
            "m:q",
            "m:ee",
            "me",
            "ni",
            "ni:",
            "ni:0",
            "é",
        ] {
            let error = DataLayout::parse(text).expect_err(text);
            assert_eq!(error.kind, IrErrorKind::InvalidDataLayout, "{text}");
        }
    }

    /// §2.1: `Display` writes a string that parses back to the same layout.
    #[test]
    fn display_round_trips() {
        for text in [
            "",
            AARCH64_LINUX_DATA_LAYOUT,
            AARCH64_LINUX_DATA_LAYOUT_LLVM19,
            X86_64_LINUX_DATA_LAYOUT,
            "E-m:o-p:32:32:64:16-p1:64:64-i24:32:64-f80:128-v256:256-a:16:64-n8:16-ni:1:2-S64-P1-A2-G3-Fi8",
        ] {
            let dl = parse(text);
            let printed = dl.to_string();
            assert_eq!(parse(&printed), dl, "{text} -> {printed}");
        }
        assert_eq!(
            parse("").to_string(),
            "e-p0:64:64:64:64-i1:8:8-i8:8:8-i16:16:16-i32:32:32-i64:32:64-f16:16:16-f32:32:32\
             -f64:64:64-f128:128:128-v64:64:64-v128:128:128-a:0:64"
        );
    }

    /// §2.1 `DataLayoutMismatch`: only differences that change layout.
    #[test]
    fn mismatch_is_reported_only_for_layout_changes() {
        let target = TargetSpec::x86_64_linux_gnu();
        target.check_data_layout(X86_64_LINUX_DATA_LAYOUT).unwrap();
        // Mangling, native widths, other address spaces, function-pointer
        // alignment and redundant entries do not change layout.
        target
            .check_data_layout(
                "e-m:w-p1:32:32-i64:64-i128:128-i24:32-i256:128-f80:128-v128:128-n64-S128-Fn8-ni:7",
            )
            .unwrap();
        let mismatch = |text: &str| {
            let error = target.check_data_layout(text).expect_err(text);
            assert_eq!(error.kind, IrErrorKind::DataLayoutMismatch, "{text}");
            error.message
        };
        let base = "e-m:e-i64:64-i128:128-f80:128-S128";
        target.check_data_layout(base).unwrap();
        assert!(mismatch(&format!("E{}", &base[1..])).contains("endianness"));
        assert!(mismatch(&format!("{base}-p:32:32")).contains("pointer"));
        assert!(mismatch(&format!("{base}-p:64:64:64:32")).contains("pointer"));
        assert!(mismatch("e-i128:128-f80:128-S128").contains("i64"));
        assert!(mismatch("e-i64:64-f80:128-S128").contains("i128"));
        assert!(mismatch(&format!("{base}-i16:16:32")).contains("i16"));
        assert!(mismatch(&format!("{base}-i256:256")).contains("i256"));
        assert!(mismatch(&format!("{base}-f80:32")).contains("80-bit"));
        assert!(mismatch(&format!("{base}-v128:64")).contains("128-bit vectors"));
        assert!(mismatch(&format!("{base}-v256:128")).contains("256-bit vectors"));
        assert!(mismatch(&format!("{base}-a:64")).contains("aggregate"));
        assert!(mismatch("e-i64:64-i128:128-f80:128").contains("stack"));
        assert!(mismatch(&format!("{base}-A1")).contains("address space"));
        assert!(mismatch("").contains("i64"));
        // The two targets' layouts are not interchangeable.
        assert!(mismatch(AARCH64_LINUX_DATA_LAYOUT).contains("i8"));
        assert_eq!(
            target.check_data_layout("e-q").unwrap_err().kind,
            IrErrorKind::InvalidDataLayout
        );
    }

    /// §2.1: triples. `UnsupportedTarget` for anything but the two targets.
    #[test]
    fn triples_map_to_targets() {
        for triple in ["aarch64-unknown-linux-gnu", "aarch64-linux-gnu", "arm64-pc-linux"] {
            let target = TargetSpec::from_triple(triple).unwrap();
            assert_eq!(target, TargetSpec::aarch64_linux_gnu());
            assert_eq!(target.arch(), Arch::Aarch64);
            assert_eq!(target.os(), Os::LinuxGnu);
            assert_eq!(target.triple(), "aarch64-unknown-linux-gnu");
            assert_eq!(target.data_layout_str(), AARCH64_LINUX_DATA_LAYOUT);
        }
        for triple in ["x86_64-unknown-linux-gnu", "x86_64-pc-linux-gnu", "amd64-linux"] {
            let target = TargetSpec::from_triple(triple).unwrap();
            assert_eq!(target.arch(), Arch::X86_64);
            assert_eq!(target.triple(), "x86_64-unknown-linux-gnu");
            assert_eq!(target.data_layout(), &parse(X86_64_LINUX_DATA_LAYOUT));
        }
        for triple in [
            "",
            "aarch64",
            "riscv64-unknown-linux-gnu",
            "x86_64-unknown-linux-musl",
            "aarch64-apple-darwin",
            "x86_64-pc-windows-msvc",
            "aarch64-unknown-linux-gnu-extra",
            "aarch64_be-unknown-linux-gnu",
        ] {
            let error = TargetSpec::from_triple(triple).expect_err(triple);
            assert_eq!(error.kind, IrErrorKind::UnsupportedTarget, "{triple}");
        }
    }
}
