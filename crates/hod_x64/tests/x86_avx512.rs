//! AVX-512 (WP-208, `spec/codegen.md` §5.6.35, design
//! `docs/design/0007-avx512.md`).
//!
//! **The masked memory intrinsics** (`llvm.masked.load`, `.store`,
//! `.expandload`, `.compressstore`), which hod builds lane by lane
//! whatever the features: generated functions over every vector shape
//! the AVX-512 headers use, with a variable mask and with constant ones,
//! compiled by hod (plain, under every stress option, and from LLVM 22's
//! spelling of the signatures) and by `llc -O0 -fast-isel=0` of LLVM 22
//! and 19 (without features, where it scalarizes, and with AVX2, where it
//! uses `vmaskmov` for the 4- and 8-byte lanes), linked with one C driver
//! and run on the emulator: every leg prints the same bytes. The memory
//! a call reads or writes ends at a page the driver has made
//! inaccessible, so a lane whose mask bit is clear and that touched
//! memory would fault.

#![allow(
    clippy::too_many_lines,
    clippy::cast_possible_truncation,
    reason = "a test generator: `.ll` and C text built by hand"
)]
#![allow(
    clippy::many_single_char_names,
    clippy::similar_names,
    clippy::unused_self,
    clippy::needless_range_loop,
    clippy::items_after_statements,
    clippy::case_sensitive_file_extension_comparisons,
    reason = "an interpreter of listings (registers and bytes by short names), and rows               indexed in several tables; `.sd` ends an intrinsic's name, not a file's"
)]

#[path = "../../hod_obj/tests/common/linking.rs"]
#[allow(dead_code, reason = "a shared helper: not every function is used here")]
mod linking;

#[path = "common/float_gen.rs"]
mod float_gen;

use std::fmt::Write as _;
use std::path::Path;
use std::process::Command;
use std::time::Duration;

use float_gen::{Rng, ll_header, llc, parse, pic};
use hod_cg::{CgError, Stress};
use hod_testutil::Scratch;
use linking::Toolchain;

/// The vector shapes of the masked loads and stores of the AVX-512
/// headers (F, VL, BW, FP16), and a few others: element type, lanes,
/// element bytes.
const SHAPES: &[(&str, u32, u32)] = &[
    ("float", 16, 4),
    ("double", 8, 8),
    ("i32", 16, 4),
    ("i64", 8, 8),
    ("i16", 32, 2),
    ("i8", 64, 1),
    ("half", 32, 2),
    ("float", 8, 4),
    ("double", 4, 8),
    ("i32", 8, 4),
    ("i64", 4, 8),
    ("i16", 16, 2),
    ("i8", 32, 1),
    ("float", 4, 4),
    ("double", 2, 8),
    ("i32", 4, 4),
    ("i64", 2, 8),
    ("i16", 8, 2),
    ("i8", 16, 1),
    ("i32", 3, 4),
    ("float", 2, 4),
];

/// The four intrinsics: name, whether it loads, whether it packs the set
/// lanes (expand and compress).
const OPS: &[(&str, bool, bool)] = &[
    ("load", true, false),
    ("store", false, false),
    ("expandload", true, true),
    ("compressstore", false, true),
];

/// One generated function `void tK(ptr %mem, ptr %vec, i64 %bits)`: a
/// load reads `%mem` under the mask into the vector at `%vec` (whose
/// bytes are the pass-through), a store writes the vector at `%vec` to
/// `%mem`.
struct Func {
    op: usize,
    shape: usize,
    /// A constant mask, or `None` for the low lanes of `%bits`.
    fixed: Option<u64>,
}

/// The LLVM mangling of a vector of `lanes` `elem`.
fn mangle(elem: &str, lanes: u32) -> String {
    let e = match elem {
        "float" => "f32",
        "double" => "f64",
        "half" => "f16",
        other => other,
    };
    format!("v{lanes}{e}")
}

/// The constant `<N x i1>` of `bits`.
fn mask_const(lanes: u32, bits: u64) -> String {
    let items: Vec<String> =
        (0..lanes).map(|i| format!("i1 {}", u8::from(bits >> i & 1 == 1))).collect();
    format!("<{}>", items.join(", "))
}

/// The module text of `funcs`: LLVM 19's signatures (`new` false, which
/// LLVM 22 also reads, upgrading them) or LLVM 22's (`new` true), every
/// function with `attrs` (`""` for none).
fn module(funcs: &[Func], new: bool, attrs: &str) -> String {
    let mut text = ll_header();
    let mut declared = std::collections::BTreeSet::new();
    for (k, f) in funcs.iter().enumerate() {
        let (op, load, packed) = OPS[f.op];
        let (elem, lanes, _) = SHAPES[f.shape];
        let vty = format!("<{lanes} x {elem}>");
        let mty = format!("<{lanes} x i1>");
        let suffix =
            if packed { mangle(elem, lanes) } else { format!("{}.p0", mangle(elem, lanes)) };
        let name = format!("llvm.masked.{op}.{suffix}");
        let align = if new { "" } else { "i32, " };
        if declared.insert(name.clone()) {
            let decl = match (load, packed) {
                (true, false) => format!("{vty} @{name}(ptr, {align}{mty}, {vty})"),
                (false, false) => format!("void @{name}({vty}, ptr, {align}{mty})"),
                (true, true) => format!("{vty} @{name}(ptr, {mty}, {vty})"),
                (false, true) => format!("void @{name}({vty}, ptr, {mty})"),
            };
            writeln!(text, "declare {decl}").unwrap();
        }
        let mark = if attrs.is_empty() { "" } else { " #0" };
        writeln!(text, "define void @t{k}(ptr %mem, ptr %vec, i64 %bits){mark} {{").unwrap();
        let m = if let Some(bits) = f.fixed {
            mask_const(lanes, bits)
        } else {
            if lanes == 64 {
                writeln!(text, "  %m = bitcast i64 %bits to {mty}").unwrap();
            } else {
                writeln!(text, "  %b = trunc i64 %bits to i{lanes}").unwrap();
                writeln!(text, "  %m = bitcast i{lanes} %b to {mty}").unwrap();
            }
            "%m".to_string()
        };
        writeln!(text, "  %v = load {vty}, ptr %vec, align 1").unwrap();
        let (ptr, align) = if new { ("ptr align 1 %mem", "") } else { ("ptr %mem", "i32 1, ") };
        let call = match (load, packed) {
            (true, false) => format!("%r = call {vty} @{name}({ptr}, {align}{mty} {m}, {vty} %v)"),
            (false, false) => format!("call void @{name}({vty} %v, {ptr}, {align}{mty} {m})"),
            (true, true) => format!("%r = call {vty} @{name}(ptr %mem, {mty} {m}, {vty} %v)"),
            (false, true) => format!("call void @{name}({vty} %v, ptr %mem, {mty} {m})"),
        };
        writeln!(text, "  {call}").unwrap();
        if load {
            writeln!(text, "  store {vty} %r, ptr %vec, align 1").unwrap();
        }
        text.push_str("  ret void\n}\n");
    }
    if !attrs.is_empty() {
        writeln!(text, "attributes #0 = {{ \"target-features\"=\"{attrs}\" }}").unwrap();
    }
    text
}

/// Every function: each intrinsic on each shape with a variable mask, and
/// with constant masks (none, all, alternating, the top lane) on some.
fn functions() -> Vec<Func> {
    let mut funcs = Vec::new();
    for op in 0..OPS.len() {
        for shape in 0..SHAPES.len() {
            funcs.push(Func { op, shape, fixed: None });
        }
        for shape in [0, 5, 13, 19] {
            let lanes = SHAPES[shape].1;
            let all = if lanes == 64 { u64::MAX } else { (1 << lanes) - 1 };
            for fixed in [0, all, all & 0x5555_5555_5555_5555, 1 << (lanes - 1)] {
                funcs.push(Func { op, shape, fixed: Some(fixed) });
            }
        }
    }
    funcs
}

/// The C driver: for each function and case, the mask, the memory
/// bytes and the vector bytes from the tables; the memory a call may
/// touch placed right below an inaccessible page. Prints the vector
/// after a load, and the memory (with the 8 bytes below it) after a
/// store.
fn driver(funcs: &[Func], cases: &[Vec<(u64, Vec<u8>)>]) -> String {
    let mut c = String::from(
        "#include <stdio.h>\n#include <string.h>\n#include <stdint.h>\n#include <sys/mman.h>\n",
    );
    for k in 0..funcs.len() {
        writeln!(c, "void t{k}(void *, void *, uint64_t);").unwrap();
    }
    c.push_str("typedef void (*fn)(void *, void *, uint64_t);\nstatic const fn fns[] = {");
    for k in 0..funcs.len() {
        write!(c, "t{k},").unwrap();
    }
    c.push_str("};\n/* load, packed, element bytes, lanes */\nstatic const int kind[][4] = {");
    for f in funcs {
        let (_, load, packed) = OPS[f.op];
        let (_, lanes, bytes) = SHAPES[f.shape];
        write!(c, "{{{},{},{bytes},{lanes}}},", u8::from(load), u8::from(packed)).unwrap();
    }
    let per = cases[0].len();
    c.push_str("};\nstatic const uint64_t bits[] = {");
    for (_, set) in funcs.iter().zip(cases) {
        for (bits, _) in set {
            write!(c, "{bits}u,").unwrap();
        }
    }
    c.push_str("};\nstatic const unsigned char data[][1024] = {");
    for set in cases {
        for (_, bytes) in set {
            c.push('{');
            for b in bytes {
                write!(c, "{b},").unwrap();
            }
            c.push_str("},\n");
        }
    }
    writeln!(c, "}};\nstatic const int per = {per};").unwrap();
    c.push_str(
        "int main(void) {\n  \
         unsigned char *map = mmap(0, 8192, PROT_READ | PROT_WRITE, MAP_PRIVATE | MAP_ANONYMOUS, -1, 0);\n  \
         if (map == MAP_FAILED) return 2;\n\
         #ifndef NO_GUARD\n  \
         if (mprotect(map + 4096, 4096, PROT_NONE)) return 2;\n\
         #endif\n  \
         unsigned char *guard = map + 4096;\n  \
         setvbuf(stdout, 0, _IOLBF, 0);\n  \
         for (unsigned f = 0; f < sizeof fns / sizeof *fns; f++)\n    \
         for (int i = 0; i < per; i++) {\n      \
         const int load = kind[f][0], packed = kind[f][1], size = kind[f][2], lanes = kind[f][3];\n      \
         uint64_t m = bits[f * per + i];\n      \
         if (lanes < 64) m &= (1ull << lanes) - 1;\n      \
         int span = 0;\n      \
         for (int l = 0; l < lanes; l++)\n        \
         if (m >> l & 1) span = packed ? span + size : (l + 1) * size;\n      \
         const unsigned char *d = data[f * per + i];\n      \
         unsigned char *mem = guard - span;\n      \
         memcpy(map + 4096 - 600, d, 600);\n      \
         unsigned char vec[512];\n      \
         memcpy(vec, d + 512, 512);\n      \
         fns[f](mem, vec, bits[f * per + i]);\n      \
         printf(\"%u %d \", f, i);\n      \
         if (load) for (int k = 0; k < lanes * size; k++) printf(\"%02x\", vec[k]);\n      \
         else for (int k = -8; k < span; k++) printf(\"%02x\", mem[k]);\n      \
         printf(\"\\n\");\n    }\n  \
         return 0;\n}\n",
    );
    c
}

/// §5.6.35: the masked memory intrinsics hod builds lane by lane compute
/// what `llc`'s code computes, and touch no memory of a lane whose bit is
/// clear.
#[test]
fn masked_memory_intrinsics_agree_with_llc() {
    let funcs = functions();
    let mut rng = Rng(0x0208);
    let cases: Vec<Vec<(u64, Vec<u8>)>> = funcs
        .iter()
        .map(|f| {
            (0..8)
                .map(|i| {
                    // A constant mask's function ignores the bits; the
                    // driver places the memory by them.
                    let bits = match (f.fixed, i) {
                        (Some(fixed), _) => fixed,
                        (None, 0) => 0,
                        (None, 1) => u64::MAX,
                        _ => rng.next(),
                    };
                    let bytes = (0..1024).map(|_| rng.next() as u8).collect();
                    (bits, bytes)
                })
                .collect()
        })
        .collect();
    let lines: usize = cases.iter().map(Vec::len).sum();

    let old = module(&funcs, false, "");
    let new = module(&funcs, true, "");
    let avx2 = module(&funcs, false, "+avx,+avx2");
    let scratch = Scratch::new("x64-avx512-masked");
    let mut legs = Vec::new();
    for (mode, stress, text) in [
        ("plain", Stress::default(), &old),
        ("stress", Stress::ALL, &old),
        ("llvm22-form", Stress::default(), &new),
        ("avx2", Stress::default(), &avx2),
    ] {
        let (module, bodies) = parse(text);
        let object = hod_x64::compile_to_object(&module, &bodies, &pic(stress))
            .unwrap_or_else(|e| panic!("hod {mode}: {e}"));
        legs.push((format!("hod {mode}"), scratch.write(&format!("hod-{mode}.o"), object)));
    }
    for major in [22, 19] {
        for (mode, text) in [("plain", &old), ("avx2", &avx2)] {
            if let Some(object) = llc(&scratch, major, &format!("llc{major}-{mode}"), text) {
                legs.push((format!("llc {major} {mode}"), object));
            }
        }
    }
    if let Some(object) = llc(&scratch, 22, "llc22-llvm22-form", &new) {
        legs.push(("llc 22 llvm22-form".to_string(), object));
    }
    eprintln!("masked: {} functions, {lines} cases, {} legs", funcs.len(), legs.len());

    let Some(toolchain) = Toolchain::x86_64(&scratch) else { return };
    let toolchain = toolchain.with_emulated_cpu("max");
    let source = driver(&funcs, &cases);
    let guarded = toolchain.compile_c(&scratch, "driver", &source, &[]);
    // The AVX2 `llc` legs fault under QEMU on a `vmaskmov` whose
    // masked-off lanes reach the inaccessible page (measured; the
    // hardware suppresses that fault): they run without it.
    let unguarded = toolchain.compile_c(&scratch, "driver-unguarded", &source, &["-DNO_GUARD"]);
    let mut runs: Vec<(String, String)> = Vec::new();
    let mut crashed = Vec::new();
    for (leg, object) in &legs {
        let linkers: Vec<_> = if leg == "hod plain" {
            toolchain.linkers.iter().filter(|l| matches!(l.name, "bfd" | "lld")).collect()
        } else {
            toolchain.linkers.iter().take(1).collect()
        };
        let driver =
            if leg.starts_with("llc") && leg.ends_with("avx2") { &unguarded } else { &guarded };
        for linker in linkers {
            let leg = format!("{leg} ({})", linker.name);
            let program = scratch.join(leg.replace([' ', '(', ')'], "-"));
            let ran = toolchain.link_and_run(linker, &[], &[driver, object], &program);
            let out = ran.stdout_text();
            if !ran.success() {
                crashed.push(format!(
                    "{leg}: {:?} after `{}`\n{}",
                    ran.status,
                    out.lines().last().unwrap_or("").get(..12).unwrap_or(""),
                    ran.stderr_text()
                ));
            }
            runs.push((leg, out));
        }
    }
    assert!(crashed.is_empty(), "{}", crashed.join("\n"));
    let (reference_leg, reference) =
        &runs[runs.iter().position(|r| r.0.starts_with("llc 22")).unwrap_or(0)];
    assert_eq!(reference.lines().count(), lines, "{reference_leg}: one line per case");
    let mut wrong = Vec::new();
    for (leg, out) in &runs {
        assert_eq!(out.lines().count(), lines, "{leg}: one line per case");
        for (want, got) in reference.lines().zip(out.lines()) {
            if want != got {
                wrong.push(format!("{leg}: want {want}\n  got {got}"));
            }
        }
    }
    eprintln!(
        "masked: legs {:?} agree on {lines} cases",
        runs.iter().map(|r| &r.0).collect::<Vec<_>>()
    );
    assert!(
        wrong.is_empty(),
        "{} disagreements:\n{}",
        wrong.len(),
        wrong[..wrong.len().min(20)].join("\n")
    );
}

/// Compiles `text` (functions after the module header) with hod.
fn compile(text: &str) -> Result<(), CgError> {
    let (module, bodies) = parse(&format!("{}{text}", ll_header()));
    match hod_x64::compile_to_object(&module, &bodies, &pic(Stress::default())) {
        Ok(_) => Ok(()),
        Err(hod_x64::ModuleError::Function { error, .. }) => Err(error),
        Err(other) => panic!("{other}"),
    }
}

/// §5.6.35: a mask of more than 64 lanes is refused as `x64:mask-wide`;
/// the rows are x86-64 ones, not built on AArch64.
#[test]
fn masked_refusals_and_rows() {
    let wide = "declare <128 x i8> @llvm.masked.load.v128i8.p0(ptr, i32, <128 x i1>, <128 x i8>)\n\
                define void @f(ptr %p, ptr %q) {\n  %m = load <128 x i1>, ptr %q, align 1\n  \
                %v = load <128 x i8>, ptr %q, align 1\n  \
                %r = call <128 x i8> @llvm.masked.load.v128i8.p0(ptr %p, i32 1, <128 x i1> %m, <128 x i8> %v)\n  \
                store <128 x i8> %r, ptr %q, align 1\n  ret void\n}\n";
    assert_eq!(compile(wide), Err(CgError::unsupported("x64:mask-wide")));
    let x64 = hod_ir::Support::shipped(hod_ir::Arch::X86_64);
    let a64 = hod_ir::Support::shipped(hod_ir::Arch::Aarch64);
    for name in [
        "intrinsic:llvm.masked.load",
        "intrinsic:llvm.masked.store",
        "intrinsic:llvm.masked.expandload",
        "intrinsic:llvm.masked.compressstore",
    ] {
        assert_eq!(x64.supports(name), Some(true), "{name}");
        assert_eq!(a64.supports(name), Some(false), "{name}");
    }
}

// ---------------------------------------------------------------------
// The AVX-512 intrinsics (design 0007 §4.2, §4.3)
// ---------------------------------------------------------------------

/// A symbolic interpreter of `llvm-objdump -M intel` listings of one
/// test function (design 0007 §4.2): what the function stores to `*%out`,
/// as bytes that are constants, bytes of the inputs, or bytes of the
/// results of instructions, which are nodes (a mnemonic, its immediate,
/// its decorations, its mask and its inputs in operand order). Two
/// listings compute the same thing when their results are equal: a
/// [`Graph`] shared by the listings interns equal nodes once, so equal
/// trees are equal ids.
mod dataflow {
    use std::collections::HashMap;
    use std::fmt::Write as _;

    /// Where a byte comes from.
    #[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
    pub enum Src {
        /// The constant `at`.
        Const,
        /// Byte `at` of an input: 1..=3 the memory `%a`, `%b`, `%c` point
        /// at, 4 the register `%m`.
        Leaf(u8),
        /// Byte `at` of a node's result.
        Node(u32),
        /// Byte `at` of a register's content at the entry (garbage).
        Entry(u8),
        /// Byte `at` of the address `base + off`: 0 `%out`, 1..=3 `%a`..
        /// `%c`, 5 and up the frame (a new base after a realignment).
        Ptr(u8, i64),
        /// Byte `at` of memory nothing wrote.
        Undef(u8),
    }

    /// One symbolic byte: its source, and the bits of it that are kept
    /// (the others are 0), for an `and` with a constant and for the
    /// lanes of a mask.
    #[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
    pub struct Sym {
        pub src: Src,
        pub at: i64,
        pub keep: u8,
    }

    impl Sym {
        pub const fn of(src: Src, at: i64) -> Self {
            Sym { src, at, keep: 0xff }
        }

        pub const fn constant(v: u8) -> Self {
            Sym { src: Src::Const, at: v as i64, keep: 0xff }
        }

        /// The byte with only the bits of `mask` kept.
        pub fn and(self, mask: u8) -> Self {
            match self.src {
                Src::Const => Sym::constant(self.at as u8 & mask),
                _ if self.keep & mask == 0 => Sym::constant(0),
                _ => Sym { keep: self.keep & mask, ..self },
            }
        }
    }

    /// A node: an instruction that is not a move.
    #[derive(Clone, PartialEq, Eq, Hash, Debug)]
    pub struct Node {
        pub mnemonic: String,
        pub imm: Option<i64>,
        /// `{z}`, `{rn-sae}`, `{1to16}`, ...
        pub deco: Vec<String>,
        /// The mask's bits, when masked.
        pub mask: Option<Vec<Sym>>,
        pub inputs: Vec<Vec<Sym>>,
    }

    /// The nodes of every listing compared.
    #[derive(Default)]
    pub struct Graph {
        ids: HashMap<Node, u32>,
        nodes: Vec<Node>,
    }

    impl Graph {
        fn intern(&mut self, node: Node) -> u32 {
            if let Some(&id) = self.ids.get(&node) {
                return id;
            }
            let id = u32::try_from(self.nodes.len()).unwrap();
            self.nodes.push(node.clone());
            self.ids.insert(node, id);
            id
        }

        /// `bytes` as text: runs of consecutive bytes of one source.
        pub fn show(&self, bytes: &[Sym]) -> String {
            let mut out = String::new();
            let mut i = 0;
            while i < bytes.len() {
                let s = bytes[i];
                let mut j = i + 1;
                while j < bytes.len()
                    && bytes[j].src == s.src
                    && bytes[j].keep == s.keep
                    && (bytes[j].at == s.at + (j - i) as i64
                        || s.src == Src::Const && bytes[j].at == s.at)
                {
                    j += 1;
                }
                if !out.is_empty() {
                    out.push(' ');
                }
                let keep = if s.keep == 0xff { String::new() } else { format!("&{:#x}", s.keep) };
                let name = match s.src {
                    Src::Const => format!("{:#x}", s.at),
                    Src::Leaf(4) => "m".to_string(),
                    Src::Leaf(n) => ["out", "a", "b", "c"][usize::from(n)].to_string(),
                    Src::Node(n) => format!("#{n}"),
                    Src::Entry(r) => format!("entry{r}"),
                    Src::Ptr(b, off) => format!("&{b}{off:+}"),
                    Src::Undef(b) => format!("undef{b}"),
                };
                if s.src == Src::Const {
                    write!(out, "{name}{keep}x{}", j - i).unwrap();
                } else {
                    write!(out, "{name}[{}..{}]{keep}", s.at, s.at + (j - i) as i64).unwrap();
                }
                i = j;
            }
            out
        }

        /// The tree of `bytes` as text, every node once.
        pub fn tree(&self, bytes: &[Sym]) -> String {
            let mut seen = Vec::new();
            let mut out = self.show(bytes);
            let mut todo: Vec<u32> = node_ids(bytes);
            while let Some(id) = todo.pop() {
                if seen.contains(&id) {
                    continue;
                }
                seen.push(id);
                let n = &self.nodes[id as usize];
                write!(out, "\n  #{id} = {}", n.mnemonic).unwrap();
                for d in &n.deco {
                    write!(out, " {d}").unwrap();
                }
                if let Some(m) = &n.mask {
                    write!(out, " mask({})", self.show(m)).unwrap();
                    todo.extend(node_ids(m));
                }
                for input in &n.inputs {
                    write!(out, ", ({})", self.show(input)).unwrap();
                    todo.extend(node_ids(input));
                }
                if let Some(imm) = n.imm {
                    write!(out, ", {imm:#x}").unwrap();
                }
            }
            out
        }
    }

    fn node_ids(bytes: &[Sym]) -> Vec<u32> {
        let mut ids: Vec<u32> = bytes
            .iter()
            .filter_map(|s| if let Src::Node(n) = s.src { Some(n) } else { None })
            .collect();
        ids.dedup();
        ids
    }

    /// An operand.
    #[derive(Clone, Debug)]
    enum Opnd {
        /// A general register: number, byte offset (1 for `ah`), bytes.
        Gpr(u8, u8, u8),
        /// A vector register and its bytes.
        Vec(u8, u8),
        /// An opmask register.
        K(u8),
        /// Memory: base register, index register and scale, displacement,
        /// bytes (0 for `lea`'s), broadcast; WP-224: a vector of indices
        /// (VSIB: the vector register, its bytes, the scale).
        Mem {
            base: Option<u8>,
            index: Option<(u8, i64)>,
            disp: i64,
            bytes: u8,
            bcst: bool,
            vindex: Option<(u8, u8, i64)>,
        },
        /// `rip`-relative memory (a constant of the object).
        Rip(i64, u8),
        Imm(i64),
        /// `{rn-sae}`, `{sae}`.
        Deco(String),
    }

    const GPR64: [&str; 16] = [
        "rax", "rcx", "rdx", "rbx", "rsp", "rbp", "rsi", "rdi", "r8", "r9", "r10", "r11", "r12",
        "r13", "r14", "r15",
    ];
    const GPR32: [&str; 16] = [
        "eax", "ecx", "edx", "ebx", "esp", "ebp", "esi", "edi", "r8d", "r9d", "r10d", "r11d",
        "r12d", "r13d", "r14d", "r15d",
    ];
    const GPR16: [&str; 16] = [
        "ax", "cx", "dx", "bx", "sp", "bp", "si", "di", "r8w", "r9w", "r10w", "r11w", "r12w",
        "r13w", "r14w", "r15w",
    ];
    const GPR8: [&str; 16] = [
        "al", "cl", "dl", "bl", "spl", "bpl", "sil", "dil", "r8b", "r9b", "r10b", "r11b", "r12b",
        "r13b", "r14b", "r15b",
    ];

    fn reg(name: &str) -> Option<Opnd> {
        let at = |list: &[&str]| list.iter().position(|&r| r == name).map(|i| i as u8);
        if let Some(i) = at(&GPR64) {
            return Some(Opnd::Gpr(i, 0, 8));
        }
        if let Some(i) = at(&GPR32) {
            return Some(Opnd::Gpr(i, 0, 4));
        }
        if let Some(i) = at(&GPR16) {
            return Some(Opnd::Gpr(i, 0, 2));
        }
        if let Some(i) = at(&GPR8) {
            return Some(Opnd::Gpr(i, 0, 1));
        }
        if let Some(i) = ["ah", "ch", "dh", "bh"].iter().position(|&r| r == name) {
            return Some(Opnd::Gpr(i as u8, 1, 1));
        }
        for (prefix, bytes) in [("xmm", 16), ("ymm", 32), ("zmm", 64)] {
            if let Some(n) = name.strip_prefix(prefix).and_then(|n| n.parse::<u8>().ok()) {
                return Some(Opnd::Vec(n, bytes));
            }
        }
        name.strip_prefix('k').and_then(|n| n.parse::<u8>().ok()).filter(|&n| n < 8).map(Opnd::K)
    }

    fn number(text: &str) -> Option<i64> {
        let (neg, t) = match text.strip_prefix('-') {
            Some(t) => (true, t),
            None => (false, text),
        };
        let v = match t.strip_prefix("0x") {
            Some(hex) => u64::from_str_radix(hex, 16).ok()? as i64,
            None => t.parse::<i64>().ok()?,
        };
        Some(if neg { -v } else { v })
    }

    fn mem_bytes(prefix: &str) -> u8 {
        match prefix {
            "byte" => 1,
            "word" => 2,
            "dword" => 4,
            "qword" => 8,
            "tbyte" => 10,
            "xmmword" => 16,
            "ymmword" => 32,
            "zmmword" => 64,
            _ => 0,
        }
    }

    /// One operand's text: the operand, its mask register, `{z}`.
    fn operand(text: &str) -> (Opnd, Option<u8>, bool) {
        let text = text.trim();
        if text.starts_with('{') && !text.contains('[') {
            return (Opnd::Deco(text.to_string()), None, false);
        }
        let (mut mask, mut zero) = (None, false);
        let mut core = text.to_string();
        // Decorations after the operand: ` {k1}`, ` {z}`, `{1to16}`.
        let mut bcst = false;
        while let Some(open) = core.rfind('{') {
            let deco = core[open..].trim().to_string();
            core.truncate(open);
            match deco.as_str() {
                "{z}" => zero = true,
                d if d.starts_with("{k") => mask = d[2..d.len() - 1].parse::<u8>().ok(),
                d if d.starts_with("{1to") => bcst = true,
                d => panic!("decoration {d} in `{text}`"),
            }
        }
        let core = core.trim();
        if let Some(r) = reg(core) {
            return (r, mask, zero);
        }
        if let Some(v) = number(core) {
            return (Opnd::Imm(v), mask, zero);
        }
        let open = core.find('[').unwrap_or_else(|| panic!("operand `{text}`"));
        let bytes = mem_bytes(core.split_whitespace().next().unwrap_or(""));
        let inner = &core[open + 1..core.len() - 1];
        let (mut base, mut index, mut disp, mut rip) = (None, None, 0i64, false);
        let mut vindex = None;
        let mut sign = 1;
        for tok in inner.split_whitespace() {
            match tok {
                "+" => sign = 1,
                "-" => sign = -1,
                "rip" => rip = true,
                t if t.contains('*') => {
                    let (s, r) = t.split_once('*').unwrap();
                    let s = number(s).unwrap();
                    match reg(r) {
                        Some(Opnd::Gpr(r, 0, 8)) => index = Some((r, s)),
                        Some(Opnd::Vec(r, n)) => vindex = Some((r, n, s)),
                        _ => panic!("index in `{text}`"),
                    }
                }
                t => match reg(t) {
                    Some(Opnd::Gpr(r, 0, 8)) if base.is_none() => base = Some(r),
                    Some(Opnd::Gpr(r, 0, 8)) => index = Some((r, 1)),
                    Some(Opnd::Vec(r, n)) => vindex = Some((r, n, 1)),
                    _ => disp += sign * number(t).unwrap_or_else(|| panic!("`{t}` in `{text}`")),
                },
            }
        }
        if rip {
            return (Opnd::Rip(disp, bytes), mask, zero);
        }
        (Opnd::Mem { base, index, disp, bytes, bcst, vindex }, mask, zero)
    }

    /// One instruction: mnemonic, operands, mask register, `{z}`.
    struct Line {
        mnemonic: String,
        ops: Vec<Opnd>,
        mask: Option<u8>,
        zero: bool,
    }

    fn line(text: &str) -> Line {
        let text = text.split('#').next().unwrap_or("").trim();
        let mut words = text.splitn(2, char::is_whitespace);
        let mut mnemonic = words.next().unwrap_or("").to_string();
        let mut rest = words.next().unwrap_or("").trim().to_string();
        while matches!(mnemonic.as_str(), "lock" | "rep" | "notrack" | "cs") {
            let mut words = rest.splitn(2, char::is_whitespace);
            mnemonic = words.next().unwrap_or("").to_string();
            rest = words.next().unwrap_or("").trim().to_string();
        }
        let (mut mask, mut zero) = (None, false);
        let mut ops = Vec::new();
        if !rest.is_empty() {
            for part in rest.split(", ") {
                let (op, m, z) = operand(part);
                mask = mask.or(m);
                zero |= z;
                ops.push(op);
            }
        }
        // hod writes GFNI's 128-bit forms in their SSE encoding, `llc`
        // with AVX in their VEX one: `op x, y` is `vop x, x, y`.
        if mnemonic.starts_with("gf2p8") && !ops.is_empty() {
            mnemonic.insert(0, 'v');
            ops.insert(1, ops[0].clone());
        }
        Line { mnemonic, ops, mask, zero }
    }

    /// The listings of an `llvm-objdump -d -M intel --no-show-raw-insn
    /// --no-leading-addr` output, by function name.
    pub fn functions(text: &str) -> HashMap<String, Vec<String>> {
        let mut out: HashMap<String, Vec<String>> = HashMap::new();
        let mut current = None;
        for l in text.lines() {
            if let Some(name) = l.strip_prefix('<').and_then(|l| l.strip_suffix(">:")) {
                current = Some(name.to_string());
                out.entry(name.to_string()).or_default();
            } else if l.starts_with(char::is_whitespace)
                && !l.trim().is_empty()
                && let Some(name) = &current
            {
                out.get_mut(name).unwrap().push(l.trim().to_string());
            }
        }
        out
    }

    /// The machine state.
    struct State {
        gpr: [[Sym; 8]; 16],
        vec: Vec<[Sym; 64]>,
        k: [[Sym; 8]; 8],
        mem: HashMap<(u8, i64), Sym>,
        frames: u8,
        /// WP-224: the lanes a scatter stored, in order (nodes `scatter`).
        events: Vec<u32>,
    }

    /// Mnemonics that read their destination besides their sources.
    fn tied(m: &str) -> bool {
        [
            "vpermt2",
            "vpermi2",
            "vfmadd",
            "vfmsub",
            "vfnmadd",
            "vfnmsub",
            "vpternlog",
            "vpdp",
            "vpmadd52",
            "vpshldv",
            "vpshrdv",
        ]
        .iter()
        .any(|p| m.starts_with(p))
    }

    /// Whether `m` moves bytes unchanged (when unmasked).
    fn is_move(m: &str) -> bool {
        matches!(
            m,
            "vmovdqu"
                | "vmovdqa"
                | "vmovdqu8"
                | "vmovdqu16"
                | "vmovdqu32"
                | "vmovdqu64"
                | "vmovdqa32"
                | "vmovdqa64"
                | "vmovups"
                | "vmovaps"
                | "vmovupd"
                | "vmovapd"
                | "movups"
                | "movaps"
                | "movupd"
                | "movapd"
                | "movdqu"
                | "movdqa"
        )
    }

    /// Whether `m` reads a source half the destination's width.
    fn half_source(m: &str) -> bool {
        matches!(m, "vcvttps2qq" | "vcvttps2uqq" | "vcvtps2qq" | "vcvtps2uqq" | "vcvtps2pd")
    }

    /// The bytes of an element of `m`'s lanes, by its suffix.
    fn lane_bytes(m: &str) -> u32 {
        if m.ends_with("pd") {
            8
        } else if m.ends_with("ps") || m.ends_with('d') {
            4
        } else if m.ends_with('q') {
            8
        } else if m.ends_with("ph") || m.ends_with('w') {
            2
        } else {
            1
        }
    }

    /// Runs `listing` and returns the bytes stored to `%out[0..bytes]`.
    /// `mask_lanes`: how many bits of a mask register the instructions
    /// of the test read (both compilers' code reads the same lanes, but
    /// may leave different garbage above them).
    pub fn run(graph: &mut Graph, listing: &[String], bytes: usize, mask_lanes: u32) -> Vec<Sym> {
        run_with_ret(graph, listing, bytes, mask_lanes, 0).0
    }

    /// [`run`], and (WP-208) the 64 bytes of each of the first `ret_regs`
    /// vector registers at the `ret`.
    pub fn run_with_ret(
        graph: &mut Graph,
        listing: &[String],
        bytes: usize,
        mask_lanes: u32,
        ret_regs: usize,
    ) -> (Vec<Sym>, Vec<Vec<Sym>>) {
        let s = execute(graph, listing, mask_lanes);
        let out = (0..bytes as i64).map(|at| s.load_byte(0, at)).collect();
        (out, s.vec[..ret_regs].iter().map(|v| v.to_vec()).collect())
    }

    /// WP-224: [`run`], and the lanes the scatters stored, in order: each
    /// a node `scatter` over the base pointer, the index, the element
    /// stored, and its mask bit unless the bit is a constant 1 (a lane
    /// whose bit is a constant 0 stores nothing). The memory a gather or
    /// a scatter reaches through its vector of indices is not modelled
    /// as bytes: a gathered lane is a node `gather` over the base, the
    /// index and (masked) the pass-through, so two listings agree when
    /// they reach the same addresses under the same mask bits.
    pub fn run_with_events(
        graph: &mut Graph,
        listing: &[String],
        bytes: usize,
        mask_lanes: u32,
    ) -> (Vec<Sym>, Vec<u32>) {
        let s = execute(graph, listing, mask_lanes);
        let out = (0..bytes as i64).map(|at| s.load_byte(0, at)).collect();
        (out, s.events)
    }

    fn execute(graph: &mut Graph, listing: &[String], mask_lanes: u32) -> State {
        let mut s = State {
            gpr: std::array::from_fn(|r| {
                std::array::from_fn(|b| Sym::of(Src::Entry(r as u8), b as i64))
            }),
            vec: (0..32)
                .map(|r| std::array::from_fn(|b| Sym::of(Src::Entry(16 + r as u8), b as i64)))
                .collect(),
            k: std::array::from_fn(|r| {
                std::array::from_fn(|b| Sym::of(Src::Entry(48 + r as u8), b as i64))
            }),
            mem: HashMap::new(),
            frames: 5,
            events: Vec::new(),
        };
        let ptr = |base: u8| -> [Sym; 8] {
            std::array::from_fn(|b| Sym::of(Src::Ptr(base, 0), b as i64))
        };
        s.gpr[7] = ptr(0);
        s.gpr[6] = ptr(1);
        s.gpr[2] = ptr(2);
        s.gpr[1] = ptr(3);
        s.gpr[8] = std::array::from_fn(|b| Sym::of(Src::Leaf(4), b as i64));
        s.gpr[4] = ptr(5);
        for text in listing {
            let l = line(text);
            if !s.step(graph, &l, mask_lanes, text) {
                break;
            }
        }
        s
    }

    impl State {
        fn load_byte(&self, base: u8, at: i64) -> Sym {
            match self.mem.get(&(base, at)) {
                Some(&b) => b,
                None if (1..=3).contains(&base) => Sym::of(Src::Leaf(base), at),
                None => Sym::of(Src::Undef(base), at),
            }
        }

        /// The address `base + disp` of a memory operand.
        fn address(&self, op: &Opnd) -> (u8, i64) {
            let Opnd::Mem { base: Some(b), index, disp, .. } = op else {
                panic!("address of {op:?}")
            };
            let p = self.gpr[usize::from(*b)];
            let Src::Ptr(base, off) = p[0].src else { panic!("an address from {:?}", p[0]) };
            assert!(
                p.iter().enumerate().all(|(i, s)| s.src == p[0].src && s.at == i as i64),
                "a partial pointer"
            );
            let mut at = off + disp;
            if let Some((r, scale)) = index {
                at += scale * self.constant(&self.gpr[usize::from(*r)]);
            }
            (base, at)
        }

        fn constant(&self, bytes: &[Sym]) -> i64 {
            let mut v = 0i64;
            for (i, b) in bytes.iter().enumerate() {
                assert_eq!(b.src, Src::Const, "a constant index");
                v |= b.at << (8 * i);
            }
            v
        }

        fn read(&self, op: &Opnd) -> Vec<Sym> {
            match *op {
                Opnd::Gpr(r, off, n) => {
                    self.gpr[usize::from(r)][usize::from(off)..usize::from(off + n)].to_vec()
                }
                Opnd::Vec(r, n) => self.vec[usize::from(r)][..usize::from(n)].to_vec(),
                Opnd::K(r) => self.k[usize::from(r)].to_vec(),
                Opnd::Mem { bytes, .. } => {
                    let (base, at) = self.address(op);
                    (0..i64::from(bytes)).map(|i| self.load_byte(base, at + i)).collect()
                }
                Opnd::Rip(disp, bytes) => {
                    (0..i64::from(bytes)).map(|i| Sym::of(Src::Leaf(99), disp + i)).collect()
                }
                Opnd::Imm(v) => (0..8).map(|i| Sym::constant((v >> (8 * i)) as u8)).collect(),
                Opnd::Deco(_) => panic!("read of a decoration"),
            }
        }

        fn width(op: &Opnd) -> usize {
            match *op {
                Opnd::Gpr(_, _, n) | Opnd::Vec(_, n) | Opnd::Rip(_, n) => usize::from(n),
                Opnd::Mem { bytes, .. } => usize::from(bytes),
                Opnd::K(_) | Opnd::Imm(_) => 8,
                Opnd::Deco(_) => 0,
            }
        }

        /// Writes `bytes` to `op`; `vex`: a vector write clears the
        /// register above it (VEX and EVEX), else keeps it (legacy SSE).
        fn write(&mut self, op: &Opnd, bytes: &[Sym], vex: bool) {
            match *op {
                Opnd::Gpr(r, off, n) => {
                    let g = &mut self.gpr[usize::from(r)];
                    for i in 0..usize::from(n) {
                        g[usize::from(off) + i] = bytes[i];
                    }
                    if n == 4 {
                        for b in &mut g[4..] {
                            *b = Sym::constant(0);
                        }
                    }
                }
                Opnd::Vec(r, n) => {
                    let v = &mut self.vec[usize::from(r)];
                    v[..usize::from(n)].copy_from_slice(&bytes[..usize::from(n)]);
                    if vex {
                        for b in &mut v[usize::from(n)..] {
                            *b = Sym::constant(0);
                        }
                    }
                }
                Opnd::K(r) => {
                    let k = &mut self.k[usize::from(r)];
                    for (i, b) in k.iter_mut().enumerate() {
                        *b = bytes.get(i).copied().unwrap_or(Sym::constant(0));
                    }
                }
                Opnd::Mem { .. } => {
                    let (base, at) = self.address(op);
                    for (i, &b) in bytes.iter().enumerate() {
                        self.mem.insert((base, at + i as i64), b);
                    }
                }
                _ => panic!("write to {op:?}"),
            }
        }

        fn rsp(&self) -> (u8, i64) {
            match self.gpr[4][0].src {
                Src::Ptr(b, off) => (b, off),
                other => panic!("rsp is {other:?}"),
            }
        }

        fn set_ptr(&mut self, r: u8, base: u8, off: i64) {
            self.gpr[usize::from(r)] =
                std::array::from_fn(|b| Sym::of(Src::Ptr(base, off), b as i64));
        }

        /// The pointer `r` holds, if it holds one.
        fn ptr_of(&self, r: u8) -> Option<(u8, i64)> {
            match self.gpr[usize::from(r)][0].src {
                Src::Ptr(b, off) => Some((b, off)),
                _ => None,
            }
        }

        /// WP-224: a gather (`vpgatherdd zmm0 {k1}, [rcx + 4*zmm1]`) or a
        /// scatter (`vpscatterdd [rcx + 4*zmm1] {k1}, zmm2`) under opmask
        /// `k`. Its lanes: the fewer of the data register's elements and
        /// the index register's indices (the element size is the last
        /// letter, `q`, `pd` 8 bytes, else 4; the index size the letter
        /// after `gather`/`scatter`). Lane `j` reads mask bit `j`; a gather
        /// writes its lanes and clears the register above them, and both
        /// clear the opmask. The base pointer is normalized with the
        /// displacement folded in (`lea rax, [rsi + 8]` then `[rax + ..]`
        /// is `[rsi + 8 + ..]`); the instruction's own spelling (integer
        /// or float) is not kept, its bytes moved are the same.
        fn vsib(&mut self, graph: &mut Graph, m: &str, ops: &[Opnd], k: u8) {
            let scatter = m.contains("scatter");
            let kind = if scatter { "scatter" } else { "gather" };
            let after = &m[m.find(kind).expect("a gather or a scatter") + kind.len()..];
            let elem = if after.ends_with('q') || after.ends_with("pd") { 8 } else { 4 };
            let isz = if after.starts_with('q') { 8 } else { 4 };
            let (mem, data) = if scatter { (&ops[0], &ops[1]) } else { (&ops[1], &ops[0]) };
            let Opnd::Mem {
                base: Some(base),
                index: None,
                disp,
                vindex: Some((ix, ix_bytes, scale)),
                ..
            } = *mem
            else {
                panic!("a VSIB operand: {mem:?}")
            };
            let Opnd::Vec(r, r_bytes) = *data else { panic!("a VSIB data register: {data:?}") };
            let lanes = (usize::from(r_bytes) / elem).min(usize::from(ix_bytes) / isz);
            let b = self.gpr[usize::from(base)];
            let consistent =
                b.iter().enumerate().all(|(i, s)| s.src == b[0].src && s.at == i as i64);
            let (base_bytes, disp) = match b[0].src {
                Src::Ptr(p, off) if consistent => {
                    ((0..8).map(|i| Sym::of(Src::Ptr(p, off + disp), i)).collect::<Vec<_>>(), 0)
                }
                _ => (b.to_vec(), disp),
            };
            let kbits = self.k[usize::from(k)];
            let old = self.vec[usize::from(r)];
            let index = self.vec[usize::from(ix)];
            let mut out = [Sym::constant(0); 64];
            for j in 0..lanes {
                let bit = kbits[j / 8].and(1 << (j % 8));
                let lane = &old[j * elem..(j + 1) * elem];
                let mask = match bit.src {
                    Src::Const if bit.at == 0 => {
                        if !scatter {
                            out[j * elem..(j + 1) * elem].copy_from_slice(lane);
                        }
                        continue;
                    }
                    Src::Const => None,
                    _ => Some(vec![bit]),
                };
                let mut inputs = vec![base_bytes.clone(), index[j * isz..(j + 1) * isz].to_vec()];
                // A scatter's element; a masked gather's pass-through.
                if scatter || mask.is_some() {
                    inputs.push(lane.to_vec());
                }
                let node = Node {
                    mnemonic: kind.to_string(),
                    imm: Some(scale),
                    deco: vec![format!("disp{disp:+}"), format!("e{elem}"), format!("i{isz}")],
                    mask,
                    inputs,
                };
                let id = graph.intern(node);
                if scatter {
                    self.events.push(id);
                } else {
                    for (i, b) in out[j * elem..(j + 1) * elem].iter_mut().enumerate() {
                        *b = Sym::of(Src::Node(id), i as i64);
                    }
                }
            }
            if !scatter {
                self.vec[usize::from(r)] = out;
            }
            self.k[usize::from(k)] = [Sym::constant(0); 8];
        }

        /// Executes one instruction; `false` at `ret`.
        #[allow(clippy::too_many_lines, reason = "one arm per kind of instruction")]
        fn step(&mut self, graph: &mut Graph, l: &Line, mask_lanes: u32, text: &str) -> bool {
            let m = l.mnemonic.as_str();
            let ops = &l.ops;
            let vex = m.starts_with('v') || m.starts_with('k');
            match m {
                "ret" => return false,
                "nop" | "endbr64" | "int3" | "cmp" | "test" | "bt" => return true,
                _ if m.starts_with("kortest") || m.starts_with("ktest") => return true,
                _ if m.starts_with('j') || m == "call" => panic!("control flow: `{text}`"),
                "push" => {
                    let v = self.read(&ops[0]);
                    let (b, off) = self.rsp();
                    self.set_ptr(4, b, off - 8);
                    for (i, &x) in v.iter().take(8).enumerate() {
                        self.mem.insert((b, off - 8 + i as i64), x);
                    }
                    return true;
                }
                "pop" => {
                    let (b, off) = self.rsp();
                    let v: Vec<Sym> = (0..8).map(|i| self.load_byte(b, off + i)).collect();
                    self.write(&ops[0], &v, false);
                    self.set_ptr(4, b, off + 8);
                    return true;
                }
                "leave" => {
                    self.gpr[4] = self.gpr[5];
                    let (b, off) = self.rsp();
                    let v: Vec<Sym> = (0..8).map(|i| self.load_byte(b, off + i)).collect();
                    self.write(&Opnd::Gpr(5, 0, 8), &v, false);
                    self.set_ptr(4, b, off + 8);
                    return true;
                }
                "lea" => {
                    let (b, off) = self.address(&ops[1]);
                    let Opnd::Gpr(r, 0, 8) = ops[0] else { panic!("lea into {:?}", ops[0]) };
                    self.set_ptr(r, b, off);
                    return true;
                }
                "vzeroupper" => {
                    for v in &mut self.vec[..16] {
                        for b in &mut v[16..] {
                            *b = Sym::constant(0);
                        }
                    }
                    return true;
                }
                _ => {}
            }
            // Pointer arithmetic, and a realignment of `rsp`.
            if let (Some(Opnd::Gpr(r, 0, 8)), Some(Opnd::Imm(v))) = (ops.first(), ops.get(1))
                && let Some((b, off)) = self.ptr_of(*r)
            {
                match m {
                    "add" => {
                        self.set_ptr(*r, b, off + v);
                        return true;
                    }
                    "sub" => {
                        self.set_ptr(*r, b, off - v);
                        return true;
                    }
                    "and" => {
                        self.frames += 1;
                        let f = self.frames;
                        self.set_ptr(*r, f, 0);
                        return true;
                    }
                    _ => {}
                }
            }
            // A register cleared by itself.
            if ops.len() >= 2
                && matches!(
                    m,
                    "xor" | "sub" | "pxor" | "xorps" | "xorpd" | "vpxor" | "vpxord" | "vpxorq"
                        | "vxorps" | "vxorpd" | "kxorb" | "kxorw" | "kxord" | "kxorq"
                )
                // `op a, a` or `op d, a, a`.
                && format!("{:?}", ops[ops.len() - 1]) == format!("{:?}", ops[ops.len() - 2])
            {
                let w = Self::width(&ops[0]);
                self.write(&ops[0], &vec![Sym::constant(0); w], vex);
                return true;
            }
            // WP-224: an opmask set by itself (`kxnorw k1, k0, k0`): the
            // bits of the size set, those above cleared.
            if let Some(size) = m.strip_prefix("kxnor")
                && ops.len() == 3
                && format!("{:?}", ops[1]) == format!("{:?}", ops[2])
            {
                let n = match size {
                    "b" => 1,
                    "w" => 2,
                    "d" => 4,
                    _ => 8,
                };
                let v: Vec<Sym> =
                    (0..8).map(|i| Sym::constant(if i < n { 0xff } else { 0 })).collect();
                self.write(&ops[0], &v, vex);
                return true;
            }
            // WP-224: a gather or a scatter through a vector of indices.
            if let Some(k) = l.mask
                && ops.iter().any(|o| matches!(o, Opnd::Mem { vindex: Some(_), .. }))
            {
                self.vsib(graph, m, ops, k);
                return true;
            }
            // `and` with a constant: the bits kept.
            if m == "and"
                && let Some(Opnd::Imm(v)) = ops.get(1)
            {
                let a = self.read(&ops[0]);
                let r: Vec<Sym> =
                    a.iter().enumerate().map(|(i, b)| b.and((v >> (8 * i.min(7))) as u8)).collect();
                self.write(&ops[0], &r, false);
                return true;
            }
            // Moves.
            let unmasked = l.mask.is_none();
            if unmasked && (is_move(m) || m == "mov" || m == "movabs" || m.starts_with("kmov")) {
                let w = if let Opnd::Imm(_) = ops[1] {
                    Self::width(&ops[0])
                } else {
                    Self::width(&ops[1])
                };
                let w = if m.starts_with("kmov") {
                    match m {
                        "kmovb" => 1,
                        "kmovw" => 2,
                        "kmovd" => 4,
                        _ => 8,
                    }
                } else {
                    w.min(Self::width(&ops[0]))
                };
                let mut v = self.read(&ops[1]);
                v.truncate(w);
                // A move into a wider register zero-extends (a `kmov`
                // into a general register, a `kmov` into an opmask).
                let dw = Self::width(&ops[0]);
                v.resize(dw.max(w), Sym::constant(0));
                self.write(&ops[0], &v, vex);
                return true;
            }
            // A scalar store: the low element (WP-208: an element of a
            // vector passed element by element).
            if unmasked
                && matches!(m, "movss" | "movsd" | "vmovss" | "vmovsd")
                && ops.len() == 2
                && matches!(ops[0], Opnd::Mem { .. })
            {
                let mut v = self.read(&ops[1]);
                v.truncate(if m.ends_with("ss") { 4 } else { 8 });
                self.write(&ops[0], &v, false);
                return true;
            }
            let scalar_load = matches!(m, "movss" | "movsd" | "vmovss" | "vmovsd")
                && ops.len() == 2
                && matches!(ops[1], Opnd::Mem { .. });
            if unmasked
                && (scalar_load || matches!(m, "movzx" | "movd" | "movq" | "vmovd" | "vmovq"))
            {
                let src_w = match (m, &ops[1]) {
                    ("movd" | "vmovd" | "movss" | "vmovss", _) => 4,
                    ("movq" | "vmovq" | "movsd" | "vmovsd", _) => 8,
                    (_, o) => Self::width(o),
                };
                let mut v = self.read(&ops[1]);
                v.truncate(src_w);
                let dw = Self::width(&ops[0]);
                // `movd`/`movq` into an `xmm` clear its low 16 bytes;
                // their VEX forms the whole register.
                v.resize(dw, Sym::constant(0));
                self.write(&ops[0], &v, vex || !matches!(ops[0], Opnd::Vec(..)));
                return true;
            }
            // A lane taken out of a vector or put into one (WP-208: how
            // `llc` stores the top of a `zmm`), unmasked: bytes moved.
            if unmasked
                && (m.starts_with("vextract") || m.starts_with("vinsert"))
                && !m.ends_with("ps")
                && let Some(Opnd::Imm(at)) = ops.last()
            {
                let at = usize::try_from(*at).expect("a lane");
                if m.starts_with("vextract") {
                    let w = Self::width(&ops[0]);
                    let v = self.read(&ops[1])[at * w..(at + 1) * w].to_vec();
                    self.write(&ops[0], &v, vex);
                } else {
                    let mut v = self.read(&ops[1]);
                    let piece = self.read(&ops[2]);
                    let w = piece.len();
                    v[at * w..(at + 1) * w].copy_from_slice(&piece);
                    self.write(&ops[0], &v, vex);
                }
                return true;
            }
            // The shuffles `llc` assembles a `<12 x float>` with: an
            // element inserted (`vinsertps`, its count_s, count_d and
            // zero mask), quadwords blended, an element broadcast.
            if unmasked && ops.len() == 4 && m == "vinsertps" {
                let Opnd::Imm(imm) = ops[3] else { panic!("`{text}`") };
                let mut v = self.read(&ops[1]);
                v.truncate(16);
                let src = self.read(&ops[2]);
                let from =
                    if matches!(ops[2], Opnd::Mem { .. }) { 0 } else { (imm >> 6) as usize & 3 };
                let to = (imm >> 4) as usize & 3;
                v[to * 4..to * 4 + 4].copy_from_slice(&src[from * 4..from * 4 + 4]);
                for e in 0..4 {
                    if imm >> e & 1 == 1 {
                        v[e * 4..e * 4 + 4].fill(Sym::constant(0));
                    }
                }
                self.write(&ops[0], &v, vex);
                return true;
            }
            if unmasked && ops.len() == 4 && m == "vblendpd" {
                let Opnd::Imm(imm) = ops[3] else { panic!("`{text}`") };
                let (a, b) = (self.read(&ops[1]), self.read(&ops[2]));
                let w = Self::width(&ops[0]);
                let v: Vec<Sym> =
                    (0..w).map(|i| if imm >> (i / 8) & 1 == 1 { b[i] } else { a[i] }).collect();
                self.write(&ops[0], &v, vex);
                return true;
            }
            if unmasked && ops.len() == 2 && m == "vbroadcastss" {
                let src = self.read(&ops[1]);
                let w = Self::width(&ops[0]);
                let v: Vec<Sym> = (0..w).map(|i| src[i % 4]).collect();
                self.write(&ops[0], &v, vex);
                return true;
            }
            // Everything else is a node.
            let dest = &ops[0];
            let mut inputs = Vec::new();
            let mut deco = Vec::new();
            let mut imm = None;
            let legacy = !vex && !matches!(m, "movsx" | "movsxd");
            let (mut mask, mut merge) = (None, false);
            if let Some(k) = l.mask {
                let mut bits: Vec<Sym> = self.k[usize::from(k)].to_vec();
                let n = mask_lanes.div_ceil(8) as usize;
                bits.truncate(n);
                if !mask_lanes.is_multiple_of(8) {
                    bits[n - 1] = bits[n - 1].and((1u8 << (mask_lanes % 8)) - 1);
                }
                let full = |i: usize| {
                    if i + 1 == n && !mask_lanes.is_multiple_of(8) {
                        (1u8 << (mask_lanes % 8)) - 1
                    } else {
                        0xff
                    }
                };
                let all = bits.iter().enumerate().all(|(i, b)| *b == Sym::constant(full(i)));
                let none = bits.iter().all(|b| *b == Sym::constant(0));
                if none && matches!(dest, Opnd::Vec(..)) {
                    // Nothing is written: the destination stays (merging)
                    // or is cleared (zeroing).
                    let w = Self::width(dest);
                    let v = if l.zero { vec![Sym::constant(0); w] } else { self.read(dest) };
                    self.write(dest, &v, vex);
                    return true;
                }
                if !all {
                    merge = !l.zero;
                    mask = Some(bits);
                }
            }
            // Merging into a register that holds nothing (an undefined
            // pass-through, which `llc` leaves where it is) is zeroing,
            // what an undefined value may be.
            let mut zero = l.zero;
            if merge && self.read(dest).iter().all(|b| matches!(b.src, Src::Entry(r) if r >= 16)) {
                (merge, zero) = (false, true);
            }
            if zero && mask.is_some() {
                deco.push("{z}".to_string());
            }
            if legacy || tied(m) || merge {
                inputs.push(self.read(dest));
            }
            for op in &ops[1..] {
                match op {
                    Opnd::Imm(v) => imm = Some(*v),
                    Opnd::Deco(d) => deco.push(d.clone()),
                    Opnd::Mem { bcst: true, .. } => {
                        deco.push("{bcst}".to_string());
                        inputs.push(self.read(op));
                    }
                    _ => {
                        let mut v = self.read(op);
                        // A widening conversion reads half the
                        // destination's width.
                        if half_source(m) && matches!(op, Opnd::Vec(..) | Opnd::Mem { .. }) {
                            v.truncate(Self::width(dest) / 2);
                        }
                        // `vcvtph2pd` reads a quarter.
                        if m == "vcvtph2pd" && matches!(op, Opnd::Vec(..) | Opnd::Mem { .. }) {
                            v.truncate(Self::width(dest) / 4);
                        }
                        inputs.push(v);
                    }
                }
            }
            if is_move(m) && mask.is_none() {
                // A masked move whose mask normalized away.
                let v = inputs.pop().unwrap();
                self.write(dest, &v, vex);
                return true;
            }
            // A masked move of the whole unmasked result of a node with
            // lanes of its size is that node masked (`llc` keeps
            // `vcvtph2pd` and its mask apart).
            if is_move(m) && mask.is_some() {
                let w = Self::width(dest);
                let moved = inputs.last().unwrap();
                if let Some(Src::Node(n)) = moved.first().map(|s| s.src) {
                    let whole = moved.len() == w
                        && moved
                            .iter()
                            .enumerate()
                            .all(|(i, s)| *s == Sym::of(Src::Node(n), i as i64));
                    let inner = graph.nodes[n as usize].clone();
                    if whole && inner.mask.is_none() && lane_bytes(&inner.mnemonic) == lane_bytes(m)
                    {
                        let mut ins = inputs[..inputs.len() - 1].to_vec();
                        ins.extend(inner.inputs);
                        let mut d: Vec<String> =
                            deco.iter().filter(|d| *d == "{z}").cloned().collect();
                        d.extend(inner.deco);
                        let node = Node {
                            mnemonic: inner.mnemonic,
                            imm: inner.imm,
                            deco: d,
                            mask,
                            inputs: ins,
                        };
                        let id = graph.intern(node);
                        let out: Vec<Sym> =
                            (0..w as i64).map(|i| Sym::of(Src::Node(id), i)).collect();
                        self.write(dest, &out, vex);
                        return true;
                    }
                }
            }
            let id = graph.intern(Node { mnemonic: m.to_string(), imm, deco, mask, inputs });
            let w = Self::width(dest);
            let mut out: Vec<Sym> = (0..w as i64).map(|i| Sym::of(Src::Node(id), i)).collect();
            if let Opnd::K(_) = dest {
                // The lanes of the source; the bits above are cleared.
                let src =
                    ops.iter().skip(1).find(|o| matches!(o, Opnd::Vec(..) | Opnd::Mem { .. }));
                let lanes = src.map_or(64, |o| Self::width(o) as u32 / lane_bytes(m));
                for (i, b) in out.iter_mut().enumerate() {
                    let lo = 8 * i as u32;
                    *b = if lanes >= lo + 8 {
                        *b
                    } else if lanes <= lo {
                        Sym::constant(0)
                    } else {
                        b.and((1u8 << (lanes - lo)) - 1)
                    };
                }
            }
            self.write(dest, &out, vex);
            true
        }
    }
}

/// The AVX-512 families of a name: the feature of clang's header
/// function (with `avx512vl` too for a 128- or 256-bit form).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[allow(clippy::upper_case_acronyms)] // The features' own spellings.
enum Fam {
    F,
    BW,
    DQ,
    /// `avx512vbmi2`: the compresses of bytes and words, which share F's
    /// overloaded name.
    VBMI2,
    /// `avx512vbmi` (WP-220).
    VBMI,
    /// `gfni`, `vaes`, `vpclmulqdq`, each with `avx512f`.
    GFNI,
    VAES,
    VPCLMUL,
    /// `avx512bf16`.
    BF16,
    /// `avx512fp16`: needs BW, DQ and VL, so it is refused without VL at
    /// every width.
    FP16,
    /// A VEX form of the 128- or 256-bit GFNI, VAES or VPCLMULQDQ names
    /// (`x86::ext`), by its feature: compared here by dataflow, as no
    /// emulator here has GFNI or VPCLMULQDQ (`tests/x86_ext.rs` executes
    /// the VAES ones).
    Vex(&'static str),
}
use Fam::{BF16, BW, DQ, F, FP16, GFNI, VAES, VBMI, VBMI2, VPCLMUL, Vex};

/// The census's F/VL, BW and DQ names (design 0007 §5.1): the family,
/// the role of each operand, the declaration. Roles: `v` a vector, `p`
/// the pass-through, `m` the mask, `i` an immediate, `e` a rounding mode
/// (`{er}`), `s` exception suppression (`{sae}`), `c` a shift count.
const NAMES: &[(Fam, &str, &str)] = &[
    (
        F,
        "vpms",
        "<16 x float> @llvm.x86.avx512.mask.getexp.ps.512(<16 x float>, <16 x float>, i16, i32)",
    ),
    (
        F,
        "vipms",
        "<16 x float> @llvm.x86.avx512.mask.rndscale.ps.512(<16 x float>, i32, <16 x float>, i16, i32)",
    ),
    (F, "vvs", "<16 x float> @llvm.x86.avx512.max.ps.512(<16 x float>, <16 x float>, i32)"),
    (F, "vvs", "<16 x float> @llvm.x86.avx512.min.ps.512(<16 x float>, <16 x float>, i32)"),
    (F, "vv", "<16 x float> @llvm.x86.avx512.permvar.sf.512(<16 x float>, <16 x i32>)"),
    (F, "vpm", "<16 x float> @llvm.x86.avx512.rcp14.ps.512(<16 x float>, <16 x float>, i16)"),
    (F, "vpm", "<16 x float> @llvm.x86.avx512.rsqrt14.ps.512(<16 x float>, <16 x float>, i16)"),
    (
        F,
        "vvve",
        "<16 x float> @llvm.x86.avx512.vfmaddsub.ps.512(<16 x float>, <16 x float>, <16 x float>, i32)",
    ),
    (
        F,
        "vvv",
        "<16 x float> @llvm.x86.avx512.vpermi2var.ps.512(<16 x float>, <16 x i32>, <16 x float>)",
    ),
    (
        F,
        "vipm",
        "<16 x i16> @llvm.x86.avx512.mask.vcvtps2ph.512(<16 x float>, i32, <16 x i16>, i16)",
    ),
    (
        F,
        "vpm",
        "<16 x i32> @llvm.x86.avx512.mask.compress.v16i32(<16 x i32>, <16 x i32>, <16 x i1>)",
    ),
    (
        F,
        "vpme",
        "<16 x i32> @llvm.x86.avx512.mask.cvtps2dq.512(<16 x float>, <16 x i32>, i16, i32)",
    ),
    (
        F,
        "vpms",
        "<16 x i32> @llvm.x86.avx512.mask.cvttps2dq.512(<16 x float>, <16 x i32>, i16, i32)",
    ),
    (
        F,
        "vpms",
        "<16 x i32> @llvm.x86.avx512.mask.cvttps2udq.512(<16 x float>, <16 x i32>, i16, i32)",
    ),
    (F, "vv", "<16 x i32> @llvm.x86.avx512.permvar.si.512(<16 x i32>, <16 x i32>)"),
    (F, "vv", "<16 x i32> @llvm.x86.avx512.psll.d.512(<16 x i32>, <4 x i32>)"),
    (F, "vc", "<16 x i32> @llvm.x86.avx512.pslli.d.512(<16 x i32>, i32)"),
    (F, "vv", "<16 x i32> @llvm.x86.avx512.psllv.d.512(<16 x i32>, <16 x i32>)"),
    (F, "vv", "<16 x i32> @llvm.x86.avx512.psra.d.512(<16 x i32>, <4 x i32>)"),
    (F, "vc", "<16 x i32> @llvm.x86.avx512.psrai.d.512(<16 x i32>, i32)"),
    (F, "vv", "<16 x i32> @llvm.x86.avx512.psrav.d.512(<16 x i32>, <16 x i32>)"),
    (F, "vv", "<16 x i32> @llvm.x86.avx512.psrl.d.512(<16 x i32>, <4 x i32>)"),
    (F, "vc", "<16 x i32> @llvm.x86.avx512.psrli.d.512(<16 x i32>, i32)"),
    (F, "vv", "<16 x i32> @llvm.x86.avx512.psrlv.d.512(<16 x i32>, <16 x i32>)"),
    (
        F,
        "vvvi",
        "<16 x i32> @llvm.x86.avx512.pternlog.d.512(<16 x i32>, <16 x i32>, <16 x i32>, i32)",
    ),
    (F, "vvv", "<16 x i32> @llvm.x86.avx512.vpermi2var.d.512(<16 x i32>, <16 x i32>, <16 x i32>)"),
    (F, "vpm", "<16 x i8> @llvm.x86.avx512.mask.pmovs.qb.128(<2 x i64>, <16 x i8>, i8)"),
    (F, "vpm", "<16 x i8> @llvm.x86.avx512.mask.pmovs.qb.256(<4 x i64>, <16 x i8>, i8)"),
    (F, "vpm", "<16 x i8> @llvm.x86.avx512.mask.pmovs.qb.512(<8 x i64>, <16 x i8>, i8)"),
    (F, "vpm", "<16 x i8> @llvm.x86.avx512.mask.pmovus.db.128(<4 x i32>, <16 x i8>, i8)"),
    (F, "vpm", "<16 x i8> @llvm.x86.avx512.mask.pmovus.db.256(<8 x i32>, <16 x i8>, i8)"),
    (F, "vpm", "<16 x i8> @llvm.x86.avx512.mask.pmovus.db.512(<16 x i32>, <16 x i8>, i16)"),
    (F, "vpm", "<16 x i8> @llvm.x86.avx512.mask.pmovus.qb.128(<2 x i64>, <16 x i8>, i8)"),
    (F, "vpm", "<16 x i8> @llvm.x86.avx512.mask.pmovus.qb.256(<4 x i64>, <16 x i8>, i8)"),
    (F, "vpm", "<16 x i8> @llvm.x86.avx512.mask.pmovus.qb.512(<8 x i64>, <16 x i8>, i8)"),
    (F, "vpm", "<2 x double> @llvm.x86.avx512.mask.getexp.pd.128(<2 x double>, <2 x double>, i8)"),
    (F, "vpm", "<2 x double> @llvm.x86.avx512.rcp14.pd.128(<2 x double>, <2 x double>, i8)"),
    (
        F,
        "vvpm",
        "<2 x double> @llvm.x86.avx512.rcp14.sd(<2 x double>, <2 x double>, <2 x double>, i8)",
    ),
    (F, "vpm", "<2 x double> @llvm.x86.avx512.rsqrt14.pd.128(<2 x double>, <2 x double>, i8)"),
    (
        F,
        "vvpm",
        "<2 x double> @llvm.x86.avx512.rsqrt14.sd(<2 x double>, <2 x double>, <2 x double>, i8)",
    ),
    (
        F,
        "vvv",
        "<2 x double> @llvm.x86.avx512.vpermi2var.pd.128(<2 x double>, <2 x i64>, <2 x double>)",
    ),
    (F, "vv", "<2 x i64> @llvm.x86.avx512.psra.q.128(<2 x i64>, <2 x i64>)"),
    (F, "vc", "<2 x i64> @llvm.x86.avx512.psrai.q.128(<2 x i64>, i32)"),
    (F, "vv", "<2 x i64> @llvm.x86.avx512.psrav.q.128(<2 x i64>, <2 x i64>)"),
    (F, "vvvi", "<2 x i64> @llvm.x86.avx512.pternlog.q.128(<2 x i64>, <2 x i64>, <2 x i64>, i32)"),
    (F, "vvv", "<2 x i64> @llvm.x86.avx512.vpermi2var.q.128(<2 x i64>, <2 x i64>, <2 x i64>)"),
    (F, "vpm", "<4 x double> @llvm.x86.avx512.mask.getexp.pd.256(<4 x double>, <4 x double>, i8)"),
    (F, "vv", "<4 x double> @llvm.x86.avx512.permvar.df.256(<4 x double>, <4 x i64>)"),
    (F, "vpm", "<4 x double> @llvm.x86.avx512.rcp14.pd.256(<4 x double>, <4 x double>, i8)"),
    (F, "vpm", "<4 x double> @llvm.x86.avx512.rsqrt14.pd.256(<4 x double>, <4 x double>, i8)"),
    (
        F,
        "vvv",
        "<4 x double> @llvm.x86.avx512.vpermi2var.pd.256(<4 x double>, <4 x i64>, <4 x double>)",
    ),
    (
        F,
        "vpm",
        "<4 x float> @llvm.x86.avx512.mask.compress.v4f32(<4 x float>, <4 x float>, <4 x i1>)",
    ),
    (F, "vpm", "<4 x float> @llvm.x86.avx512.mask.getexp.ps.128(<4 x float>, <4 x float>, i8)"),
    (
        F,
        "vvv",
        "<4 x float> @llvm.x86.avx512.vpermi2var.ps.128(<4 x float>, <4 x i32>, <4 x float>)",
    ),
    (F, "vpm", "<4 x i32> @llvm.x86.avx512.mask.compress.v4i32(<4 x i32>, <4 x i32>, <4 x i1>)"),
    (F, "vpm", "<4 x i32> @llvm.x86.avx512.mask.cvttpd2udq.128(<2 x double>, <4 x i32>, i8)"),
    (F, "vpm", "<4 x i32> @llvm.x86.avx512.mask.cvttpd2udq.256(<4 x double>, <4 x i32>, i8)"),
    (F, "vpm", "<4 x i32> @llvm.x86.avx512.mask.cvttps2udq.128(<4 x float>, <4 x i32>, i8)"),
    (F, "vpm", "<4 x i32> @llvm.x86.avx512.mask.pmovs.qd.128(<2 x i64>, <4 x i32>, i8)"),
    (F, "vpm", "<4 x i32> @llvm.x86.avx512.mask.pmovs.qd.256(<4 x i64>, <4 x i32>, i8)"),
    (F, "vpm", "<4 x i32> @llvm.x86.avx512.mask.pmovus.qd.128(<2 x i64>, <4 x i32>, i8)"),
    (F, "vpm", "<4 x i32> @llvm.x86.avx512.mask.pmovus.qd.256(<4 x i64>, <4 x i32>, i8)"),
    (F, "vvvi", "<4 x i32> @llvm.x86.avx512.pternlog.d.128(<4 x i32>, <4 x i32>, <4 x i32>, i32)"),
    (F, "vvv", "<4 x i32> @llvm.x86.avx512.vpermi2var.d.128(<4 x i32>, <4 x i32>, <4 x i32>)"),
    (F, "vv", "<4 x i64> @llvm.x86.avx512.permvar.di.256(<4 x i64>, <4 x i64>)"),
    (F, "vv", "<4 x i64> @llvm.x86.avx512.psra.q.256(<4 x i64>, <2 x i64>)"),
    (F, "vc", "<4 x i64> @llvm.x86.avx512.psrai.q.256(<4 x i64>, i32)"),
    (F, "vv", "<4 x i64> @llvm.x86.avx512.psrav.q.256(<4 x i64>, <4 x i64>)"),
    (F, "vvvi", "<4 x i64> @llvm.x86.avx512.pternlog.q.256(<4 x i64>, <4 x i64>, <4 x i64>, i32)"),
    (F, "vvv", "<4 x i64> @llvm.x86.avx512.vpermi2var.q.256(<4 x i64>, <4 x i64>, <4 x i64>)"),
    (
        F,
        "vpms",
        "<8 x double> @llvm.x86.avx512.mask.getexp.pd.512(<8 x double>, <8 x double>, i8, i32)",
    ),
    (
        F,
        "vipms",
        "<8 x double> @llvm.x86.avx512.mask.rndscale.pd.512(<8 x double>, i32, <8 x double>, i8, i32)",
    ),
    (F, "vvs", "<8 x double> @llvm.x86.avx512.max.pd.512(<8 x double>, <8 x double>, i32)"),
    (F, "vvs", "<8 x double> @llvm.x86.avx512.min.pd.512(<8 x double>, <8 x double>, i32)"),
    (F, "vv", "<8 x double> @llvm.x86.avx512.permvar.df.512(<8 x double>, <8 x i64>)"),
    (F, "vpm", "<8 x double> @llvm.x86.avx512.rcp14.pd.512(<8 x double>, <8 x double>, i8)"),
    (F, "vpm", "<8 x double> @llvm.x86.avx512.rsqrt14.pd.512(<8 x double>, <8 x double>, i8)"),
    (
        F,
        "vvve",
        "<8 x double> @llvm.x86.avx512.vfmaddsub.pd.512(<8 x double>, <8 x double>, <8 x double>, i32)",
    ),
    (
        F,
        "vvv",
        "<8 x double> @llvm.x86.avx512.vpermi2var.pd.512(<8 x double>, <8 x i64>, <8 x double>)",
    ),
    (
        F,
        "vpm",
        "<8 x float> @llvm.x86.avx512.mask.compress.v8f32(<8 x float>, <8 x float>, <8 x i1>)",
    ),
    (
        F,
        "vpme",
        "<8 x float> @llvm.x86.avx512.mask.cvtpd2ps.512(<8 x double>, <8 x float>, i8, i32)",
    ),
    (F, "vpm", "<8 x float> @llvm.x86.avx512.mask.getexp.ps.256(<8 x float>, <8 x float>, i8)"),
    (
        F,
        "vvv",
        "<8 x float> @llvm.x86.avx512.vpermi2var.ps.256(<8 x float>, <8 x i32>, <8 x float>)",
    ),
    (F, "vpm", "<8 x i16> @llvm.x86.avx512.mask.pmovs.qw.128(<2 x i64>, <8 x i16>, i8)"),
    (F, "vpm", "<8 x i16> @llvm.x86.avx512.mask.pmovs.qw.256(<4 x i64>, <8 x i16>, i8)"),
    (F, "vpm", "<8 x i16> @llvm.x86.avx512.mask.pmovs.qw.512(<8 x i64>, <8 x i16>, i8)"),
    (F, "vpm", "<8 x i16> @llvm.x86.avx512.mask.pmovus.qw.128(<2 x i64>, <8 x i16>, i8)"),
    (F, "vpm", "<8 x i16> @llvm.x86.avx512.mask.pmovus.qw.256(<4 x i64>, <8 x i16>, i8)"),
    (F, "vpm", "<8 x i16> @llvm.x86.avx512.mask.pmovus.qw.512(<8 x i64>, <8 x i16>, i8)"),
    (F, "vpm", "<8 x i32> @llvm.x86.avx512.mask.compress.v8i32(<8 x i32>, <8 x i32>, <8 x i1>)"),
    (F, "vpme", "<8 x i32> @llvm.x86.avx512.mask.cvtpd2dq.512(<8 x double>, <8 x i32>, i8, i32)"),
    (F, "vpms", "<8 x i32> @llvm.x86.avx512.mask.cvttpd2dq.512(<8 x double>, <8 x i32>, i8, i32)"),
    (F, "vpms", "<8 x i32> @llvm.x86.avx512.mask.cvttpd2udq.512(<8 x double>, <8 x i32>, i8, i32)"),
    (F, "vpm", "<8 x i32> @llvm.x86.avx512.mask.cvttps2udq.256(<8 x float>, <8 x i32>, i8)"),
    (F, "vpm", "<8 x i32> @llvm.x86.avx512.mask.pmovs.qd.512(<8 x i64>, <8 x i32>, i8)"),
    (F, "vpm", "<8 x i32> @llvm.x86.avx512.mask.pmovus.qd.512(<8 x i64>, <8 x i32>, i8)"),
    (F, "vvvi", "<8 x i32> @llvm.x86.avx512.pternlog.d.256(<8 x i32>, <8 x i32>, <8 x i32>, i32)"),
    (F, "vvv", "<8 x i32> @llvm.x86.avx512.vpermi2var.d.256(<8 x i32>, <8 x i32>, <8 x i32>)"),
    (F, "vv", "<8 x i64> @llvm.x86.avx512.permvar.di.512(<8 x i64>, <8 x i64>)"),
    (F, "vv", "<8 x i64> @llvm.x86.avx512.psll.q.512(<8 x i64>, <2 x i64>)"),
    (F, "vc", "<8 x i64> @llvm.x86.avx512.pslli.q.512(<8 x i64>, i32)"),
    (F, "vv", "<8 x i64> @llvm.x86.avx512.psllv.q.512(<8 x i64>, <8 x i64>)"),
    (F, "vv", "<8 x i64> @llvm.x86.avx512.psra.q.512(<8 x i64>, <2 x i64>)"),
    (F, "vc", "<8 x i64> @llvm.x86.avx512.psrai.q.512(<8 x i64>, i32)"),
    (F, "vv", "<8 x i64> @llvm.x86.avx512.psrav.q.512(<8 x i64>, <8 x i64>)"),
    (F, "vv", "<8 x i64> @llvm.x86.avx512.psrl.q.512(<8 x i64>, <2 x i64>)"),
    (F, "vc", "<8 x i64> @llvm.x86.avx512.psrli.q.512(<8 x i64>, i32)"),
    (F, "vv", "<8 x i64> @llvm.x86.avx512.psrlv.q.512(<8 x i64>, <8 x i64>)"),
    (F, "vvvi", "<8 x i64> @llvm.x86.avx512.pternlog.q.512(<8 x i64>, <8 x i64>, <8 x i64>, i32)"),
    (F, "vvv", "<8 x i64> @llvm.x86.avx512.vpermi2var.q.512(<8 x i64>, <8 x i64>, <8 x i64>)"),
    (BW, "vvi", "<16 x i16> @llvm.x86.avx512.dbpsadbw.256(<32 x i8>, <32 x i8>, i32)"),
    (BW, "vv", "<16 x i16> @llvm.x86.avx512.permvar.hi.256(<16 x i16>, <16 x i16>)"),
    (BW, "vv", "<16 x i16> @llvm.x86.avx512.psllv.w.256(<16 x i16>, <16 x i16>)"),
    (BW, "vv", "<16 x i16> @llvm.x86.avx512.psrav.w.256(<16 x i16>, <16 x i16>)"),
    (BW, "vv", "<16 x i16> @llvm.x86.avx512.psrlv.w.256(<16 x i16>, <16 x i16>)"),
    (
        BW,
        "vvv",
        "<16 x i16> @llvm.x86.avx512.vpermi2var.hi.256(<16 x i16>, <16 x i16>, <16 x i16>)",
    ),
    (BW, "vvi", "<32 x i16> @llvm.x86.avx512.dbpsadbw.512(<64 x i8>, <64 x i8>, i32)"),
    (BW, "vv", "<32 x i16> @llvm.x86.avx512.packssdw.512(<16 x i32>, <16 x i32>)"),
    (BW, "vv", "<32 x i16> @llvm.x86.avx512.packusdw.512(<16 x i32>, <16 x i32>)"),
    (BW, "vv", "<32 x i16> @llvm.x86.avx512.pavg.w.512(<32 x i16>, <32 x i16>)"),
    (BW, "vv", "<32 x i16> @llvm.x86.avx512.permvar.hi.512(<32 x i16>, <32 x i16>)"),
    (BW, "vv", "<32 x i16> @llvm.x86.avx512.pmul.hr.sw.512(<32 x i16>, <32 x i16>)"),
    (BW, "vv", "<32 x i16> @llvm.x86.avx512.pmulh.w.512(<32 x i16>, <32 x i16>)"),
    (BW, "vv", "<32 x i16> @llvm.x86.avx512.pmulhu.w.512(<32 x i16>, <32 x i16>)"),
    (BW, "vv", "<32 x i16> @llvm.x86.avx512.psll.w.512(<32 x i16>, <8 x i16>)"),
    (BW, "vc", "<32 x i16> @llvm.x86.avx512.pslli.w.512(<32 x i16>, i32)"),
    (BW, "vv", "<32 x i16> @llvm.x86.avx512.psllv.w.512(<32 x i16>, <32 x i16>)"),
    (BW, "vv", "<32 x i16> @llvm.x86.avx512.psra.w.512(<32 x i16>, <8 x i16>)"),
    (BW, "vc", "<32 x i16> @llvm.x86.avx512.psrai.w.512(<32 x i16>, i32)"),
    (BW, "vv", "<32 x i16> @llvm.x86.avx512.psrav.w.512(<32 x i16>, <32 x i16>)"),
    (BW, "vv", "<32 x i16> @llvm.x86.avx512.psrl.w.512(<32 x i16>, <8 x i16>)"),
    (BW, "vc", "<32 x i16> @llvm.x86.avx512.psrli.w.512(<32 x i16>, i32)"),
    (BW, "vv", "<32 x i16> @llvm.x86.avx512.psrlv.w.512(<32 x i16>, <32 x i16>)"),
    (
        BW,
        "vvv",
        "<32 x i16> @llvm.x86.avx512.vpermi2var.hi.512(<32 x i16>, <32 x i16>, <32 x i16>)",
    ),
    (BW, "vv", "<64 x i8> @llvm.x86.avx512.packsswb.512(<32 x i16>, <32 x i16>)"),
    (BW, "vv", "<64 x i8> @llvm.x86.avx512.packuswb.512(<32 x i16>, <32 x i16>)"),
    (BW, "vv", "<64 x i8> @llvm.x86.avx512.pavg.b.512(<64 x i8>, <64 x i8>)"),
    (BW, "vv", "<64 x i8> @llvm.x86.avx512.pshuf.b.512(<64 x i8>, <64 x i8>)"),
    (BW, "vvi", "<8 x i16> @llvm.x86.avx512.dbpsadbw.128(<16 x i8>, <16 x i8>, i32)"),
    (BW, "vv", "<8 x i16> @llvm.x86.avx512.permvar.hi.128(<8 x i16>, <8 x i16>)"),
    (BW, "vv", "<8 x i16> @llvm.x86.avx512.psllv.w.128(<8 x i16>, <8 x i16>)"),
    (BW, "vv", "<8 x i16> @llvm.x86.avx512.psrav.w.128(<8 x i16>, <8 x i16>)"),
    (BW, "vv", "<8 x i16> @llvm.x86.avx512.psrlv.w.128(<8 x i16>, <8 x i16>)"),
    (BW, "vvv", "<8 x i16> @llvm.x86.avx512.vpermi2var.hi.128(<8 x i16>, <8 x i16>, <8 x i16>)"),
    (BW, "vv", "<8 x i64> @llvm.x86.avx512.psad.bw.512(<64 x i8>, <64 x i8>)"),
    (DQ, "vi", "<16 x i1> @llvm.x86.avx512.fpclass.ps.512(<16 x float>, i32)"),
    (DQ, "vi", "<2 x i1> @llvm.x86.avx512.fpclass.pd.128(<2 x double>, i32)"),
    (DQ, "vpm", "<2 x i64> @llvm.x86.avx512.mask.cvtpd2qq.128(<2 x double>, <2 x i64>, i8)"),
    (DQ, "vpm", "<2 x i64> @llvm.x86.avx512.mask.cvttpd2qq.128(<2 x double>, <2 x i64>, i8)"),
    (DQ, "vpm", "<2 x i64> @llvm.x86.avx512.mask.cvttpd2uqq.128(<2 x double>, <2 x i64>, i8)"),
    (DQ, "vpm", "<2 x i64> @llvm.x86.avx512.mask.cvttps2qq.128(<4 x float>, <2 x i64>, i8)"),
    (DQ, "vpm", "<2 x i64> @llvm.x86.avx512.mask.cvttps2uqq.128(<4 x float>, <2 x i64>, i8)"),
    (DQ, "vpm", "<4 x float> @llvm.x86.avx512.mask.cvtqq2ps.128(<2 x i64>, <4 x float>, i8)"),
    (DQ, "vpm", "<4 x float> @llvm.x86.avx512.mask.cvtuqq2ps.128(<2 x i64>, <4 x float>, i8)"),
    (DQ, "vi", "<4 x i1> @llvm.x86.avx512.fpclass.pd.256(<4 x double>, i32)"),
    (DQ, "vi", "<4 x i1> @llvm.x86.avx512.fpclass.ps.128(<4 x float>, i32)"),
    (DQ, "vpm", "<4 x i64> @llvm.x86.avx512.mask.cvtpd2qq.256(<4 x double>, <4 x i64>, i8)"),
    (DQ, "vpm", "<4 x i64> @llvm.x86.avx512.mask.cvttpd2qq.256(<4 x double>, <4 x i64>, i8)"),
    (DQ, "vpm", "<4 x i64> @llvm.x86.avx512.mask.cvttpd2uqq.256(<4 x double>, <4 x i64>, i8)"),
    (DQ, "vpm", "<4 x i64> @llvm.x86.avx512.mask.cvttps2qq.256(<4 x float>, <4 x i64>, i8)"),
    (DQ, "vpm", "<4 x i64> @llvm.x86.avx512.mask.cvttps2uqq.256(<4 x float>, <4 x i64>, i8)"),
    (DQ, "vi", "<8 x i1> @llvm.x86.avx512.fpclass.pd.512(<8 x double>, i32)"),
    (DQ, "vi", "<8 x i1> @llvm.x86.avx512.fpclass.ps.256(<8 x float>, i32)"),
    (DQ, "vpme", "<8 x i64> @llvm.x86.avx512.mask.cvtpd2qq.512(<8 x double>, <8 x i64>, i8, i32)"),
    (DQ, "vpms", "<8 x i64> @llvm.x86.avx512.mask.cvttpd2qq.512(<8 x double>, <8 x i64>, i8, i32)"),
    (
        DQ,
        "vpms",
        "<8 x i64> @llvm.x86.avx512.mask.cvttpd2uqq.512(<8 x double>, <8 x i64>, i8, i32)",
    ),
    (DQ, "vpms", "<8 x i64> @llvm.x86.avx512.mask.cvttps2qq.512(<8 x float>, <8 x i64>, i8, i32)"),
    (DQ, "vpms", "<8 x i64> @llvm.x86.avx512.mask.cvttps2uqq.512(<8 x float>, <8 x i64>, i8, i32)"),
    (
        VBMI2,
        "vpm",
        "<8 x i16> @llvm.x86.avx512.mask.compress.v8i16(<8 x i16>, <8 x i16>, <8 x i1>)",
    ),
    (
        VBMI2,
        "vpm",
        "<16 x i16> @llvm.x86.avx512.mask.compress.v16i16(<16 x i16>, <16 x i16>, <16 x i1>)",
    ),
    (
        VBMI2,
        "vpm",
        "<32 x i16> @llvm.x86.avx512.mask.compress.v32i16(<32 x i16>, <32 x i16>, <32 x i1>)",
    ),
    (
        VBMI2,
        "vpm",
        "<16 x i8> @llvm.x86.avx512.mask.compress.v16i8(<16 x i8>, <16 x i8>, <16 x i1>)",
    ),
    (
        VBMI2,
        "vpm",
        "<32 x i8> @llvm.x86.avx512.mask.compress.v32i8(<32 x i8>, <32 x i8>, <32 x i1>)",
    ),
    (
        VBMI2,
        "vpm",
        "<64 x i8> @llvm.x86.avx512.mask.compress.v64i8(<64 x i8>, <64 x i8>, <64 x i1>)",
    ),
    // WP-220.
    (VBMI, "vv", "<16 x i8> @llvm.x86.avx512.permvar.qi.128(<16 x i8>, <16 x i8>)"),
    (VBMI, "vv", "<32 x i8> @llvm.x86.avx512.permvar.qi.256(<32 x i8>, <32 x i8>)"),
    (VBMI, "vv", "<64 x i8> @llvm.x86.avx512.permvar.qi.512(<64 x i8>, <64 x i8>)"),
    (VBMI, "vvv", "<16 x i8> @llvm.x86.avx512.vpermi2var.qi.128(<16 x i8>, <16 x i8>, <16 x i8>)"),
    (VBMI, "vvv", "<32 x i8> @llvm.x86.avx512.vpermi2var.qi.256(<32 x i8>, <32 x i8>, <32 x i8>)"),
    (VBMI, "vvv", "<64 x i8> @llvm.x86.avx512.vpermi2var.qi.512(<64 x i8>, <64 x i8>, <64 x i8>)"),
    (VBMI, "vv", "<16 x i8> @llvm.x86.avx512.pmultishift.qb.128(<16 x i8>, <16 x i8>)"),
    (VBMI, "vv", "<32 x i8> @llvm.x86.avx512.pmultishift.qb.256(<32 x i8>, <32 x i8>)"),
    (VBMI, "vv", "<64 x i8> @llvm.x86.avx512.pmultishift.qb.512(<64 x i8>, <64 x i8>)"),
    (GFNI, "vvi", "<64 x i8> @llvm.x86.vgf2p8affineqb.512(<64 x i8>, <64 x i8>, i8)"),
    (GFNI, "vv", "<64 x i8> @llvm.x86.vgf2p8mulb.512(<64 x i8>, <64 x i8>)"),
    (VAES, "vv", "<8 x i64> @llvm.x86.aesni.aesenc.512(<8 x i64>, <8 x i64>)"),
    (VAES, "vv", "<8 x i64> @llvm.x86.aesni.aesenclast.512(<8 x i64>, <8 x i64>)"),
    (VAES, "vv", "<8 x i64> @llvm.x86.aesni.aesdec.512(<8 x i64>, <8 x i64>)"),
    (VAES, "vv", "<8 x i64> @llvm.x86.aesni.aesdeclast.512(<8 x i64>, <8 x i64>)"),
    (VPCLMUL, "vvi", "<8 x i64> @llvm.x86.pclmulqdq.512(<8 x i64>, <8 x i64>, i8)"),
    (Vex("vaes"), "vv", "<4 x i64> @llvm.x86.aesni.aesenc.256(<4 x i64>, <4 x i64>)"),
    (Vex("vaes"), "vv", "<4 x i64> @llvm.x86.aesni.aesenclast.256(<4 x i64>, <4 x i64>)"),
    (Vex("vaes"), "vv", "<4 x i64> @llvm.x86.aesni.aesdec.256(<4 x i64>, <4 x i64>)"),
    (Vex("vaes"), "vv", "<4 x i64> @llvm.x86.aesni.aesdeclast.256(<4 x i64>, <4 x i64>)"),
    (Vex("vpclmulqdq"), "vvi", "<4 x i64> @llvm.x86.pclmulqdq.256(<4 x i64>, <4 x i64>, i8)"),
    (Vex("gfni"), "vvi", "<16 x i8> @llvm.x86.vgf2p8affineqb.128(<16 x i8>, <16 x i8>, i8)"),
    (Vex("gfni"), "vvi", "<32 x i8> @llvm.x86.vgf2p8affineqb.256(<32 x i8>, <32 x i8>, i8)"),
    (Vex("gfni"), "vv", "<16 x i8> @llvm.x86.vgf2p8mulb.128(<16 x i8>, <16 x i8>)"),
    (Vex("gfni"), "vv", "<32 x i8> @llvm.x86.vgf2p8mulb.256(<32 x i8>, <32 x i8>)"),
    (BF16, "vv", "<8 x bfloat> @llvm.x86.avx512bf16.cvtne2ps2bf16.128(<4 x float>, <4 x float>)"),
    (BF16, "vv", "<16 x bfloat> @llvm.x86.avx512bf16.cvtne2ps2bf16.256(<8 x float>, <8 x float>)"),
    (
        BF16,
        "vv",
        "<32 x bfloat> @llvm.x86.avx512bf16.cvtne2ps2bf16.512(<16 x float>, <16 x float>)",
    ),
    (BF16, "v", "<16 x bfloat> @llvm.x86.avx512bf16.cvtneps2bf16.512(<16 x float>)"),
    (BF16, "v", "<8 x bfloat> @llvm.x86.vcvtneps2bf16128(<4 x float>)"),
    (BF16, "v", "<8 x bfloat> @llvm.x86.vcvtneps2bf16256(<8 x float>)"),
    (FP16, "vi", "<8 x i1> @llvm.x86.avx512fp16.fpclass.ph.128(<8 x half>, i32)"),
    (FP16, "vi", "<16 x i1> @llvm.x86.avx512fp16.fpclass.ph.256(<16 x half>, i32)"),
    (FP16, "vi", "<32 x i1> @llvm.x86.avx512fp16.fpclass.ph.512(<32 x half>, i32)"),
    (FP16, "vpm", "<8 x half> @llvm.x86.avx512fp16.mask.getexp.ph.128(<8 x half>, <8 x half>, i8)"),
    (
        FP16,
        "vpm",
        "<16 x half> @llvm.x86.avx512fp16.mask.getexp.ph.256(<16 x half>, <16 x half>, i16)",
    ),
    (
        FP16,
        "vpms",
        "<32 x half> @llvm.x86.avx512fp16.mask.getexp.ph.512(<32 x half>, <32 x half>, i32, i32)",
    ),
    (FP16, "vpm", "<8 x half> @llvm.x86.avx512fp16.mask.rcp.ph.128(<8 x half>, <8 x half>, i8)"),
    (
        FP16,
        "vpm",
        "<16 x half> @llvm.x86.avx512fp16.mask.rcp.ph.256(<16 x half>, <16 x half>, i16)",
    ),
    (
        FP16,
        "vpm",
        "<32 x half> @llvm.x86.avx512fp16.mask.rcp.ph.512(<32 x half>, <32 x half>, i32)",
    ),
    (
        FP16,
        "vipm",
        "<8 x half> @llvm.x86.avx512fp16.mask.rndscale.ph.128(<8 x half>, i32, <8 x half>, i8)",
    ),
    (
        FP16,
        "vipm",
        "<16 x half> @llvm.x86.avx512fp16.mask.rndscale.ph.256(<16 x half>, i32, <16 x half>, i16)",
    ),
    (
        FP16,
        "vipms",
        "<32 x half> @llvm.x86.avx512fp16.mask.rndscale.ph.512(<32 x half>, i32, <32 x half>, i32, i32)",
    ),
    (FP16, "vpm", "<8 x half> @llvm.x86.avx512fp16.mask.rsqrt.ph.128(<8 x half>, <8 x half>, i8)"),
    (
        FP16,
        "vpm",
        "<16 x half> @llvm.x86.avx512fp16.mask.rsqrt.ph.256(<16 x half>, <16 x half>, i16)",
    ),
    (
        FP16,
        "vpm",
        "<32 x half> @llvm.x86.avx512fp16.mask.rsqrt.ph.512(<32 x half>, <32 x half>, i32)",
    ),
    (
        FP16,
        "vpm",
        "<8 x half> @llvm.x86.avx512fp16.mask.vcvtpd2ph.128(<2 x double>, <8 x half>, i8)",
    ),
    (
        FP16,
        "vpm",
        "<8 x half> @llvm.x86.avx512fp16.mask.vcvtpd2ph.256(<4 x double>, <8 x half>, i8)",
    ),
    (
        FP16,
        "vpme",
        "<8 x half> @llvm.x86.avx512fp16.mask.vcvtpd2ph.512(<8 x double>, <8 x half>, i8, i32)",
    ),
    (
        FP16,
        "vpm",
        "<2 x double> @llvm.x86.avx512fp16.mask.vcvtph2pd.128(<8 x half>, <2 x double>, i8)",
    ),
    (
        FP16,
        "vpm",
        "<4 x double> @llvm.x86.avx512fp16.mask.vcvtph2pd.256(<8 x half>, <4 x double>, i8)",
    ),
    (
        FP16,
        "vpms",
        "<8 x double> @llvm.x86.avx512fp16.mask.vcvtph2pd.512(<8 x half>, <8 x double>, i8, i32)",
    ),
    (FP16, "vpm", "<8 x i16> @llvm.x86.avx512fp16.mask.vcvtph2w.128(<8 x half>, <8 x i16>, i8)"),
    (
        FP16,
        "vpm",
        "<16 x i16> @llvm.x86.avx512fp16.mask.vcvtph2w.256(<16 x half>, <16 x i16>, i16)",
    ),
    (
        FP16,
        "vpme",
        "<32 x i16> @llvm.x86.avx512fp16.mask.vcvtph2w.512(<32 x half>, <32 x i16>, i32, i32)",
    ),
    (FP16, "vpm", "<8 x i16> @llvm.x86.avx512fp16.mask.vcvttph2uw.128(<8 x half>, <8 x i16>, i8)"),
    (
        FP16,
        "vpm",
        "<16 x i16> @llvm.x86.avx512fp16.mask.vcvttph2uw.256(<16 x half>, <16 x i16>, i16)",
    ),
    (
        FP16,
        "vpms",
        "<32 x i16> @llvm.x86.avx512fp16.mask.vcvttph2uw.512(<32 x half>, <32 x i16>, i32, i32)",
    ),
    (FP16, "vpm", "<8 x i16> @llvm.x86.avx512fp16.mask.vcvttph2w.128(<8 x half>, <8 x i16>, i8)"),
    (
        FP16,
        "vpm",
        "<16 x i16> @llvm.x86.avx512fp16.mask.vcvttph2w.256(<16 x half>, <16 x i16>, i16)",
    ),
    (
        FP16,
        "vpms",
        "<32 x i16> @llvm.x86.avx512fp16.mask.vcvttph2w.512(<32 x half>, <32 x i16>, i32, i32)",
    ),
    (FP16, "vv", "<8 x half> @llvm.x86.avx512fp16.max.ph.128(<8 x half>, <8 x half>)"),
    (FP16, "vv", "<16 x half> @llvm.x86.avx512fp16.max.ph.256(<16 x half>, <16 x half>)"),
    (FP16, "vvs", "<32 x half> @llvm.x86.avx512fp16.max.ph.512(<32 x half>, <32 x half>, i32)"),
    (FP16, "vv", "<8 x half> @llvm.x86.avx512fp16.min.ph.128(<8 x half>, <8 x half>)"),
    (FP16, "vv", "<16 x half> @llvm.x86.avx512fp16.min.ph.256(<16 x half>, <16 x half>)"),
    (FP16, "vvs", "<32 x half> @llvm.x86.avx512fp16.min.ph.512(<32 x half>, <32 x half>, i32)"),
    (
        FP16,
        "vvv",
        "<8 x half> @llvm.x86.avx512fp16.vfmaddsub.ph.128(<8 x half>, <8 x half>, <8 x half>)",
    ),
    (
        FP16,
        "vvv",
        "<16 x half> @llvm.x86.avx512fp16.vfmaddsub.ph.256(<16 x half>, <16 x half>, <16 x half>)",
    ),
    (
        FP16,
        "vvve",
        "<32 x half> @llvm.x86.avx512fp16.vfmaddsub.ph.512(<32 x half>, <32 x half>, <32 x half>, i32)",
    ),
];

/// A declaration's result type, name and parameter types.
fn decl(text: &str) -> (String, String, Vec<String>) {
    let (ret, rest) = text.split_once(" @").unwrap();
    let (name, params) = rest.split_once('(').unwrap();
    let params = params.trim_end_matches(')');
    let mut list = Vec::new();
    let (mut depth, mut start) = (0, 0);
    for (i, ch) in params.char_indices() {
        match ch {
            '<' => depth += 1,
            '>' => depth -= 1,
            ',' if depth == 0 => {
                list.push(params[start..i].trim().to_string());
                start = i + 1;
            }
            _ => {}
        }
    }
    list.push(params[start..].trim().to_string());
    (ret.to_string(), name.to_string(), list)
}

/// `<N x T>`: `N` and `T`.
fn vector_of(ty: &str) -> Option<(u32, &str)> {
    let inner = ty.strip_prefix('<')?.strip_suffix('>')?;
    let (n, elem) = inner.split_once(" x ")?;
    Some((n.parse().ok()?, elem))
}

/// The bits of a scalar type.
fn bits_of(ty: &str) -> u32 {
    match ty {
        "float" => 32,
        "double" => 64,
        "half" | "bfloat" => 16,
        t => t.trim_start_matches('i').parse().unwrap(),
    }
}

/// The bytes a value of `ty` is stored in.
fn bytes_of(ty: &str) -> usize {
    match vector_of(ty) {
        Some((n, elem)) => (n * bits_of(elem)).div_ceil(8) as usize,
        None => bits_of(ty).div_ceil(8) as usize,
    }
}

/// What one operand of a test function is.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Arg {
    /// Loaded from the next of `%a`, `%b`, `%c`.
    Load,
    /// The low bits of `%m`.
    Var,
    /// A constant integer, or for `<N x i1>` the constant of these bits.
    Const(i64),
    /// `zeroinitializer`.
    Zero,
    /// `undef`.
    Undef,
}

/// One test function: a row of [`NAMES`] and its operands.
#[derive(Clone, Debug)]
struct Case {
    row: usize,
    args: Vec<Arg>,
}

/// How many lanes of a mask row `row`'s instruction reads: the lanes of
/// the result or of the first operand, the fewer (a scalar `sd` one).
fn mask_lanes(row: usize) -> u32 {
    let (ret, name, params) = decl(NAMES[row].2);
    if name.ends_with(".sd") {
        return 1;
    }
    let lanes = |t: &str| vector_of(t).map_or(u32::MAX, |(n, _)| n);
    lanes(&ret).min(lanes(&params[0]))
}

/// The test functions of row `row`: every operand as usual, then one
/// operand at a time another way (constant masks, a zero or undefined
/// pass-through, each rounding mode, edge immediates and counts).
fn cases_of(row: usize) -> Vec<Case> {
    let (fam, roles, text) = NAMES[row];
    let _ = fam;
    let (_, name, params) = decl(text);
    let base: Vec<Arg> = roles
        .chars()
        .map(|r| match r {
            'v' | 'p' => Arg::Load,
            'm' => Arg::Var,
            'i' => Arg::Const(0x5a),
            'e' | 's' => Arg::Const(4),
            'c' => Arg::Const(5),
            other => panic!("role {other}"),
        })
        .collect();
    let mut cases = vec![Case { row, args: base.clone() }];
    let lanes = mask_lanes(row);
    for (j, r) in roles.chars().enumerate() {
        let mut other = |arg: Arg| {
            let mut args = base.clone();
            args[j] = arg;
            cases.push(Case { row, args });
        };
        match r {
            'm' => {
                let bits = vector_of(&params[j]).map_or_else(|| bits_of(&params[j]), |(n, _)| n);
                other(Arg::Const(-1));
                other(Arg::Const(5));
                if lanes < bits {
                    other(Arg::Const((1 << lanes) - 1));
                }
                if !name.ends_with(".sd") {
                    other(Arg::Const(0));
                }
            }
            'p' => {
                other(Arg::Zero);
                other(Arg::Undef);
            }
            'e' => (8..=11).for_each(|v| other(Arg::Const(v))),
            's' => [8, 12].into_iter().for_each(|v| other(Arg::Const(v))),
            // An `i8` immediate (`vgf2p8affineqb`, `vpclmulqdq`): each
            // selection of `vpclmulqdq`'s halves, and all bits set.
            'i' if params[j] == "i8" => {
                for v in [0, 0x01, 0x10, 0x11, -1] {
                    other(Arg::Const(v));
                }
            }
            'i' => [0, 0xff, 0x1a5].into_iter().for_each(|v| other(Arg::Const(v))),
            'c' => {
                let (_, elem) = vector_of(&params[0]).unwrap();
                let w = i64::from(bits_of(elem));
                for v in [0, 1, w - 1, w, 255, 256, -1] {
                    other(Arg::Const(v));
                }
                other(Arg::Var);
            }
            _ => {}
        }
    }
    cases
}

/// The features every AVX-512 test function has (as clang 22 writes them
/// for `-mavx512f -mavx512vl -mavx512bw -mavx512dq -mavx512vbmi2`,
/// without `evex512`; WP-220's families after them).
const AVX512_FEATURES: &str = "+avx2,+avx,+avx512f,+avx512vl,+avx512bw,+avx512dq,+avx512vbmi2,+fma,+f16c,+avx512vbmi,\
     +aes,+pclmul,+gfni,+vaes,+vpclmulqdq,+avx512bf16,+avx512fp16";

/// The module of `cases`: `void <prefix>tK(ptr %out, ptr %a, ptr %b, ptr
/// %c, i64 %m)` each, with `features`.
fn avx512_module(cases: &[Case], prefix: &str, features: &str) -> String {
    let mut text = ll_header();
    let mut declared = std::collections::BTreeSet::new();
    for (k, case) in cases.iter().enumerate() {
        let (ret, name, params) = decl(NAMES[case.row].2);
        if declared.insert(name.clone()) {
            writeln!(text, "declare {}", NAMES[case.row].2).unwrap();
        }
        writeln!(text, "define void @{prefix}t{k}(ptr %out, ptr %a, ptr %b, ptr %c, i64 %m) #0 {{")
            .unwrap();
        let mut ptrs = ["%a", "%b", "%c"].into_iter();
        let mut args = Vec::new();
        for (j, (ty, arg)) in params.iter().zip(&case.args).enumerate() {
            let value = match (*arg, vector_of(ty)) {
                (Arg::Load, _) => {
                    let p = ptrs.next().unwrap();
                    writeln!(text, "  %v{j} = load {ty}, ptr {p}, align 1").unwrap();
                    format!("%v{j}")
                }
                (Arg::Var, Some((n, "i1"))) => {
                    if n == 64 {
                        writeln!(text, "  %v{j} = bitcast i64 %m to {ty}").unwrap();
                    } else {
                        writeln!(text, "  %t{j} = trunc i64 %m to i{n}").unwrap();
                        writeln!(text, "  %v{j} = bitcast i{n} %t{j} to {ty}").unwrap();
                    }
                    format!("%v{j}")
                }
                (Arg::Var, _) if ty == "i64" => "%m".to_string(),
                (Arg::Var, _) => {
                    writeln!(text, "  %v{j} = trunc i64 %m to {ty}").unwrap();
                    format!("%v{j}")
                }
                (Arg::Const(bits), Some((n, "i1"))) => {
                    mask_const(n, if bits == -1 { u64::MAX } else { bits as u64 })
                }
                (Arg::Const(v), _) => v.to_string(),
                (Arg::Zero, _) => "zeroinitializer".to_string(),
                (Arg::Undef, _) => "undef".to_string(),
            };
            args.push(format!("{ty} {value}"));
        }
        writeln!(text, "  %r = call {ret} @{name}({})", args.join(", ")).unwrap();
        writeln!(text, "  store {ret} %r, ptr %out, align 1\n  ret void\n}}").unwrap();
    }
    writeln!(text, "attributes #0 = {{ \"target-features\"=\"{features}\" }}").unwrap();
    text
}

/// `llvm-objdump -d -M intel` of `object`, by function.
fn listings(object: &Path) -> std::collections::HashMap<String, Vec<String>> {
    let objdump = hod_testutil::tool("llvm-objdump").expect("llvm-objdump");
    let mut command = Command::new(objdump);
    command.args(["-d", "-M", "intel", "--no-show-raw-insn", "--no-leading-addr"]).arg(object);
    let outcome = hod_testutil::run_with(&mut command, &[], Duration::from_mins(2));
    assert!(outcome.success(), "llvm-objdump: {}", outcome.stderr_text());
    dataflow::functions(&outcome.stdout_text())
}

/// Every case of every row.
fn all_cases() -> Vec<Case> {
    (0..NAMES.len()).flat_map(cases_of).collect()
}

/// hod's object of `cases` with `stress`, or the first refusal.
fn hod_object(cases: &[Case], prefix: &str, stress: Stress) -> Result<Vec<u8>, String> {
    let (module, bodies) = parse(&avx512_module(cases, prefix, AVX512_FEATURES));
    match hod_x64::compile_to_object(&module, &bodies, &pic(stress)) {
        Ok(object) => Ok(object),
        Err(e) => Err(e.to_string()),
    }
}

/// The results of every case's listing in `listings`.
fn results(
    graph: &mut dataflow::Graph,
    cases: &[Case],
    listings: &std::collections::HashMap<String, Vec<String>>,
) -> Vec<Vec<dataflow::Sym>> {
    cases
        .iter()
        .enumerate()
        .map(|(k, case)| {
            let (ret, _, _) = decl(NAMES[case.row].2);
            let listing = &listings[&format!("t{k}")];
            dataflow::run(graph, listing, bytes_of(&ret), mask_lanes(case.row))
        })
        .collect()
}

/// §5.6.35: for every F/VL, BW and DQ name of the census, in every test
/// function of [`cases_of`], what hod's code stores (plainly and under
/// every stress option) is, as a tree of instructions over the inputs,
/// what `llc -O0` 22's and 19's code stores (design 0007 §4.2).
#[test]
fn avx512_names_agree_with_llc_by_dataflow() {
    let cases = all_cases();
    let scratch = Scratch::new("x64-avx512-dataflow");
    let text = avx512_module(&cases, "", AVX512_FEATURES);
    let mut legs: Vec<(String, std::path::PathBuf)> = Vec::new();
    for major in [22, 19] {
        if let Some(object) = llc(&scratch, major, &format!("llc{major}"), &text) {
            legs.push((format!("llc {major}"), object));
        }
    }
    if legs.is_empty() || hod_testutil::tool("llvm-objdump").is_none() {
        return;
    }
    // Each row alone first, so a refusal names its row.
    let mut refused = Vec::new();
    for row in 0..NAMES.len() {
        if let Err(e) = hod_object(&cases_of(row), "", Stress::default()) {
            refused.push(format!("{}: {e}", decl(NAMES[row].2).1));
        }
    }
    assert!(refused.is_empty(), "{} rows refused:\n{}", refused.len(), refused.join("\n"));
    for (mode, stress) in [("plain", Stress::default()), ("stress", Stress::ALL)] {
        let object = hod_object(&cases, "", stress).unwrap();
        legs.push((format!("hod {mode}"), scratch.write(&format!("hod-{mode}.o"), object)));
    }
    let mut graph = dataflow::Graph::default();
    let all: Vec<(String, Vec<Vec<dataflow::Sym>>)> = legs
        .iter()
        .map(|(leg, object)| (leg.clone(), results(&mut graph, &cases, &listings(object))))
        .collect();
    // hod's legs must equal one of `llc`'s: `llc` 22 folds some constant
    // masks into blends and shuffles where `llc` 19, like hod, keeps the
    // masked instruction (design 0007 §4.2).
    let references = &all[..all.len() - 2];
    let mut wrong = Vec::new();
    let mut counts = Vec::new();
    for (leg, got) in &all[all.len() - 2..] {
        let mut n = 0;
        for (k, got) in got.iter().enumerate() {
            if references.iter().any(|(_, r)| &r[k] == got) {
                continue;
            }
            n += 1;
            let case = &cases[k];
            let mut text = format!("{leg} t{k} {} {:?}:", decl(NAMES[case.row].2).1, case.args);
            for (reference_leg, r) in references {
                write!(text, "\n  {reference_leg}: {}", graph.tree(&r[k])).unwrap();
            }
            write!(text, "\n  {leg}: {}", graph.tree(got)).unwrap();
            wrong.push(text);
        }
        counts.push(format!("{leg}: {n}"));
    }
    let reference = &all[0].1;
    let folded = references.len() == 2
        && (0..cases.len()).filter(|&k| references[0].1[k] != references[1].1[k]).count() > 0;
    let nodes: usize = reference
        .iter()
        .filter(|r| r.iter().any(|s| matches!(s.src, dataflow::Src::Node(_))))
        .count();
    eprintln!(
        "avx512 dataflow: {} names, {} functions ({nodes} computing), references {:?} \
         (differing: {folded}), disagreements {counts:?}",
        NAMES.len(),
        cases.len(),
        references.iter().map(|l| &l.0).collect::<Vec<_>>(),
    );
    assert!(
        wrong.is_empty(),
        "{} disagreements:\n{}",
        wrong.len(),
        wrong[..wrong.len().min(12)].join("\n")
    );
}

/// WP-208 (`spec/abi-x64.md` §1.3, §4.3, §5.1): functions that receive
/// 512-bit vectors (in `zmm`, two `ymm`, or on the stack past the eighth
/// register, widened to 64 bytes, in a class of one) and store them to
/// `%out`, and functions that return them: the bytes stored and the
/// vector registers at the `ret`, as trees over the inputs. `(name,
/// signature and body, bytes stored, bytes of each returned register)`.
const ZMM_FLOWS: [(&str, &str, usize, &[usize]); 9] = [
    (
        "a0",
        "void @a0(ptr %out, <16 x float> %v) #0 {\n  store <16 x float> %v, ptr %out, align 1\n",
        64,
        &[],
    ),
    (
        "a1",
        "void @a1(ptr %out, <8 x double> %v0, <8 x double> %v1, <8 x double> %v2, <8 x double> %v3, \
         <8 x double> %v4, <8 x double> %v5, <8 x double> %v6, <8 x double> %v7, <8 x double> %v8) #0 {\n  \
         store <8 x double> %v8, ptr %out, align 1\n  %o = getelementptr i8, ptr %out, i64 64\n  \
         store <8 x double> %v1, ptr %o, align 1\n",
        128,
        &[],
    ),
    (
        "a2",
        "void @a2(ptr %out, <12 x float> %v) #0 {\n  store <12 x float> %v, ptr %out, align 1\n",
        48,
        &[],
    ),
    (
        "a3",
        "void @a3(ptr %out, i32 %k, <16 x float> %v0, <16 x float> %v1, <16 x float> %v2, \
         <16 x float> %v3, <16 x float> %v4, <16 x float> %v5, <16 x float> %v6, <16 x float> %v7, \
         <6 x double> %w, <32 x i16> %x) #0 {\n  store <6 x double> %w, ptr %out, align 1\n  \
         %o = getelementptr i8, ptr %out, i64 48\n  store <32 x i16> %x, ptr %o, align 1\n",
        112,
        &[],
    ),
    (
        "a4",
        "void @a4(ptr %out, { <16 x float> } %s, <64 x i8> %b) #0 {\n  \
         %v = extractvalue { <16 x float> } %s, 0\n  store <16 x float> %v, ptr %out, align 1\n  \
         %o = getelementptr i8, ptr %out, i64 64\n  store <64 x i8> %b, ptr %o, align 1\n",
        128,
        &[],
    ),
    (
        "r0",
        "<16 x float> @r0(ptr %out, ptr %a) #0 {\n  %v = load <16 x float>, ptr %a, align 1\n  ret <16 x float> %v\n}\n",
        0,
        &[64],
    ),
    (
        "r1",
        "<12 x float> @r1(ptr %out, ptr %a) #0 {\n  %v = load <12 x float>, ptr %a, align 1\n  ret <12 x float> %v\n}\n",
        0,
        &[48],
    ),
    (
        "r2",
        "{ <16 x float>, <4 x float> } @r2(ptr %out, ptr %a) #0 {\n  \
         %v = load { <16 x float>, <4 x float> }, ptr %a, align 1\n  ret { <16 x float>, <4 x float> } %v\n}\n",
        0,
        &[64, 16],
    ),
    (
        "r3",
        "<32 x float> @r3(ptr %out, ptr %a) #0 {\n  %v = load <32 x float>, ptr %a, align 1\n  ret <32 x float> %v\n}\n",
        0,
        &[64, 64],
    ),
];

/// The machines of [`zmm_flows_agree_with_llc_by_dataflow`]: 512-bit
/// vectors legal (one `zmm` per 64 bytes) and not (two `ymm`).
const ZMM_FLOW_MACHINES: [&str; 2] = [
    "\"target-features\"=\"+avx512f\"",
    "\"target-features\"=\"+avx512f,+avx512vl,+prefer-256-bit\" \"min-legal-vector-width\"=\"0\"",
];

/// The module of [`ZMM_FLOWS`] under `attrs`.
fn zmm_flow_module(attrs: &str) -> String {
    let mut text = ll_header();
    for (_, body, _, rets) in ZMM_FLOWS {
        text.push_str("define ");
        text.push_str(body);
        if rets.is_empty() {
            text.push_str("  ret void\n}\n");
        }
    }
    writeln!(text, "attributes #0 = {{ {attrs} }}").unwrap();
    text
}

/// §5.6.35, `spec/abi-x64.md` §11 (WP-208): where nothing can run, hod's
/// code for each function of [`ZMM_FLOWS`] (plainly and under every
/// stress option) stores and returns, byte for byte, what `llc` 22's and
/// 19's does: the same bytes of the same incoming registers and stack
/// slots, under each machine of [`ZMM_FLOW_MACHINES`]. The returned
/// registers are compared over the value's bytes in each; with two `ymm`
/// a 64-byte register's value is its two halves.
#[test]
fn zmm_flows_agree_with_llc_by_dataflow() {
    let scratch = Scratch::new("x64-zmm-dataflow");
    if hod_testutil::tool("llvm-objdump").is_none() {
        return;
    }
    let mut compared = 0;
    for (m, attrs) in ZMM_FLOW_MACHINES.iter().enumerate() {
        let text = zmm_flow_module(attrs);
        let narrow = m == 1;
        let mut legs: Vec<(String, std::path::PathBuf)> = Vec::new();
        for major in [22, 19] {
            if let Some(object) = llc(&scratch, major, &format!("zllc{major}-{m}"), &text) {
                legs.push((format!("llc {major}"), object));
            }
        }
        if legs.is_empty() {
            return;
        }
        let (module, bodies) = parse(&text);
        for (mode, stress) in [("plain", Stress::default()), ("stress", Stress::ALL)] {
            let object = hod_x64::compile_to_object(&module, &bodies, &pic(stress))
                .unwrap_or_else(|e| panic!("{attrs}: {e}"));
            legs.push((
                format!("hod {mode}"),
                scratch.write(&format!("zhod-{mode}-{m}.o"), object),
            ));
        }
        let mut graph = dataflow::Graph::default();
        let mut all = Vec::new();
        for (leg, object) in &legs {
            let listings = listings(object);
            let mut flows = Vec::new();
            for (name, _, bytes, rets) in ZMM_FLOWS {
                // With two `ymm` per 64 bytes, a register's value is in
                // twice as many registers, 32 bytes each.
                // Without `zmm` a `<12 x float>` result goes element by
                // element, which is memory: the bytes `rdi` points at.
                let (bytes, regs): (usize, Vec<usize>) = if narrow && name == "r1" {
                    (48, Vec::new())
                } else if narrow {
                    let halves = |&r: &usize| if r > 32 { vec![32, r - 32] } else { vec![r] };
                    (bytes, rets.iter().flat_map(halves).collect())
                } else {
                    (bytes, rets.to_vec())
                };
                let (out, ret) =
                    dataflow::run_with_ret(&mut graph, &listings[name], bytes, 0, regs.len());
                let ret: Vec<Vec<dataflow::Sym>> =
                    ret.iter().zip(&regs).map(|(v, &n)| v[..n].to_vec()).collect();
                flows.push((out, ret));
            }
            all.push((leg.clone(), flows));
        }
        let references = &all[..all.len() - 2];
        for (leg, got) in &all[all.len() - 2..] {
            for (k, flow) in got.iter().enumerate() {
                let name = ZMM_FLOWS[k].0;
                assert!(
                    references.iter().any(|(_, r)| &r[k] == flow),
                    "{attrs}: {leg} `{name}`:\n  hod: {} returning {}\n  {}: {} returning {}",
                    graph.show(&flow.0),
                    flow.1.iter().map(|r| graph.show(r)).collect::<Vec<_>>().join(", "),
                    references[0].0,
                    graph.show(&references[0].1[k].0),
                    references[0].1[k]
                        .1
                        .iter()
                        .map(|r| graph.show(r))
                        .collect::<Vec<_>>()
                        .join(", ")
                );
                // Nothing of the result is garbage or memory nobody wrote.
                assert!(
                    flow.0.iter().chain(flow.1.iter().flatten()).all(|s| matches!(
                        s.src,
                        dataflow::Src::Entry(_) | dataflow::Src::Leaf(_) | dataflow::Src::Undef(5)
                    )),
                    "{attrs}: {leg} `{name}`: {}",
                    graph.show(&flow.0)
                );
                compared += 1;
            }
        }
    }
    eprintln!("zmm dataflow: {compared} flows equal to llc's");
}

/// The comparison bites (design 0007 §4.2): `llc` 22's listings with
/// one deliberate change each give another result than the unchanged
/// listing, for every kind of change the comparison claims to catch.
#[test]
fn dataflow_comparison_catches_mutations() {
    let cases = all_cases();
    let scratch = Scratch::new("x64-avx512-mutations");
    let text = avx512_module(&cases, "", AVX512_FEATURES);
    let Some(object) = llc(&scratch, 22, "llc22", &text) else { return };
    if hod_testutil::tool("llvm-objdump").is_none() {
        return;
    }
    let listings = listings(&object);
    let mut graph = dataflow::Graph::default();
    let reference = results(&mut graph, &cases, &listings);
    // Each mutation: a name, and how it rewrites the one line it changes
    // (`None` where it does not apply to that line).
    type Mutation = (&'static str, fn(&str) -> Option<String>);
    let mutations: &[Mutation] = &[
        ("swapped sources", |l| {
            let (head, ops) = l.split_once('\t')?;
            let parts: Vec<&str> = ops.split(", ").collect();
            if parts.len() < 3
                || parts[1] == parts[2]
                || !parts[1].contains("mm")
                || !parts[2].contains("mm")
            {
                return None;
            }
            let mut p = parts.clone();
            p.swap(1, 2);
            Some(format!("{head}\t{}", p.join(", ")))
        }),
        ("another immediate", |l| {
            let at = l.rfind(", 0x")?;
            let v = i64::from_str_radix(l[at + 4..].split_whitespace().next()?, 16).ok()?;
            Some(format!("{}, {:#x}", &l[..at], v ^ 1))
        }),
        ("merging instead of zeroing", |l| l.contains(" {z}").then(|| l.replace(" {z}", ""))),
        ("zeroing instead of merging", |l| {
            (l.contains("{k1}") && !l.contains("{z}")).then(|| l.replace("{k1}", "{k1} {z}"))
        }),
        ("no mask", |l| l.contains(" {k1}").then(|| l.replace(" {k1}", "").replace(" {z}", ""))),
        ("another mask register", |l| l.contains("{k1}").then(|| l.replace("{k1}", "{k2}"))),
        ("another rounding", |l| l.contains("{rn-sae}").then(|| l.replace("{rn-sae}", "{rz-sae}"))),
        ("no rounding", |l| l.contains(", {sae}").then(|| l.replace(", {sae}", ""))),
        ("a narrower store", |l| {
            (l.contains("zmmword ptr [rdi]") && l.starts_with("vmov"))
                .then(|| l.replace("zmmword ptr [rdi], zmm", "ymmword ptr [rdi], ymm"))
        }),
        ("a source from another slot", |l| {
            (l.starts_with("vmov") && l.contains("[rdx]"))
                .then(|| l.replace("[rdx]", "[rdx + 0x8]"))
        }),
    ];
    for (what, mutate) in mutations {
        let (mut tried, mut caught) = (0, 0);
        let mut missed = Vec::new();
        for (k, case) in cases.iter().enumerate() {
            // Under a constant mask with every lane set, merging,
            // zeroing and no mask are the same, and the pass-through is
            // dead: the comparison rightly sees no change there.
            let constant_mask = NAMES[case.row]
                .1
                .chars()
                .zip(&case.args)
                .any(|(r, a)| r == 'm' && matches!(a, Arg::Const(_)));
            if constant_mask
                && (what.contains("merging") || *what == "no mask" || what.contains("slot"))
            {
                continue;
            }
            // Merging into an undefined pass-through is zeroing.
            if case.args.contains(&Arg::Undef) && what.contains("merging") {
                continue;
            }
            // Under a mask of 0 the instruction computes nothing.
            if NAMES[case.row]
                .1
                .chars()
                .zip(&case.args)
                .any(|(r, a)| r == 'm' && *a == Arg::Const(0))
            {
                continue;
            }
            let listing = &listings[&format!("t{k}")];
            for (i, l) in listing.iter().enumerate() {
                let Some(changed) = mutate(l) else { continue };
                let mut copy = listing.clone();
                copy[i] = changed;
                tried += 1;
                let (ret, _, _) = decl(NAMES[case.row].2);
                let got = dataflow::run(&mut graph, &copy, bytes_of(&ret), mask_lanes(case.row));
                if got == reference[k] {
                    missed.push(format!("t{k} {} {:?}: {l}", decl(NAMES[case.row].2).1, case.args));
                } else {
                    caught += 1;
                }
            }
        }
        eprintln!("mutation {what}: {caught} of {tried} caught");
        assert!(tried > 0, "{what}: applies nowhere");
        assert!(missed.is_empty(), "{what} missed:\n{}", missed.join("\n"));
    }
}

/// What hod refuses the function of `case` with under `features`, if it
/// refuses it.
fn refusal_under(case: Case, features: &str) -> Option<CgError> {
    let (module, bodies) = parse(&avx512_module(&[case], "", features));
    match hod_x64::compile_to_object(&module, &bodies, &pic(Stress::default())) {
        Ok(_) => None,
        Err(hod_x64::ModuleError::Function { error, .. }) => Some(error),
        Err(other) => panic!("{other}"),
    }
}

/// The stable name row `row` is refused with: `intrinsic:` and its name,
/// an overloaded one without its type suffix.
fn row_refusal(row: usize) -> &'static str {
    let (_, name, _) = decl(NAMES[row].2);
    if name.starts_with("llvm.x86.avx512.mask.compress.") {
        "intrinsic:llvm.x86.avx512.mask.compress"
    } else {
        Box::leak(format!("intrinsic:{name}").into_boxed_str())
    }
}

/// §5.6.35: each name is refused by its name in a function whose
/// features do not enable its family, `avx512vl` for a 128- or 256-bit
/// form (but the scalar `.sd` ones), or that turn `evex512` off for a
/// 512-bit one; a rounding operand `llc` does not select and a call of
/// another signature are `x64:x86-intrinsic-signature`. The rows are
/// x86-64 ones, not built on AArch64.
#[test]
fn avx512_refusals_and_rows() {
    let signature = Some(CgError::unsupported("x64:x86-intrinsic-signature"));
    let x64 = hod_ir::Support::shipped(hod_ir::Arch::X86_64);
    let a64 = hod_ir::Support::shipped(hod_ir::Arch::Aarch64);
    let without = |fam: Fam| match fam {
        F => "+avx2,+avx,+fma,+f16c".to_string(),
        BW => "+avx2,+avx,+avx512f,+avx512vl,+avx512dq,+fma,+f16c".to_string(),
        DQ => "+avx2,+avx,+avx512f,+avx512vl,+avx512bw,+avx512vbmi2,+fma,+f16c".to_string(),
        VBMI2 => "+avx2,+avx,+avx512f,+avx512vl,+avx512bw,+avx512dq,+fma,+f16c".to_string(),
        VBMI => {
            "+avx2,+avx,+avx512f,+avx512vl,+avx512bw,+avx512dq,+avx512vbmi2,+fma,+f16c".to_string()
        }
        GFNI => format!("{AVX512_FEATURES},-gfni"),
        VAES => format!("{AVX512_FEATURES},-vaes"),
        VPCLMUL => format!("{AVX512_FEATURES},-vpclmulqdq"),
        Vex(feature) => format!("{AVX512_FEATURES},-{feature}"),
        BF16 => format!("{AVX512_FEATURES},-avx512bf16"),
        FP16 => format!("{AVX512_FEATURES},-avx512fp16"),
    };
    // GFNI, VAES and VPCLMULQDQ without `avx512f`: their EVEX forms are
    // not there.
    let no_f = "+avx2,+avx,+fma,+f16c,+aes,+pclmul,+gfni,+vaes,+vpclmulqdq";
    let (no_vl, no_512) =
        (format!("{AVX512_FEATURES},-avx512vl"), format!("{AVX512_FEATURES},-evex512"));
    let mut checked = 0;
    for row in 0..NAMES.len() {
        let (fam, roles, text) = NAMES[row];
        let (ret, name, params) = decl(text);
        let refused = Some(CgError::unsupported(row_refusal(row)));
        let base = cases_of(row).swap_remove(0);
        let case = |args: &[Arg]| Case { row, args: args.to_vec() };
        assert_eq!(refusal_under(base.clone(), AVX512_FEATURES), None, "{name}");
        assert_eq!(
            refusal_under(base.clone(), &without(fam)),
            refused,
            "{name} without its family"
        );
        if matches!(fam, GFNI | VAES | VPCLMUL) {
            assert_eq!(refusal_under(base.clone(), no_f), refused, "{name} without avx512f");
        }
        // The VEX forms need no AVX-512.
        if let Vex(_) = fam {
            assert_eq!(refusal_under(base.clone(), no_f), None, "{name} without avx512f");
        }
        let bytes = params.iter().chain([&ret]).map(|t| bytes_of(t)).max().unwrap();
        let vl = bytes < 64 && !name.ends_with(".sd") && !matches!(fam, Vex(_));
        let want = if vl || fam == FP16 { refused.clone() } else { None };
        assert_eq!(refusal_under(base.clone(), &no_vl), want, "{name} without avx512vl");
        let want = if bytes == 64 { refused.clone() } else { None };
        assert_eq!(refusal_under(base.clone(), &no_512), want, "{name} with -evex512");
        for (j, r) in roles.chars().enumerate() {
            let bad: &[i64] = match r {
                'e' => &[0, 3, 12, 16],
                's' => &[0, 9, 11],
                _ => &[],
            };
            for &v in bad {
                let mut args = base.args.clone();
                args[j] = Arg::Const(v);
                assert_eq!(
                    refusal_under(case(&args), AVX512_FEATURES),
                    signature,
                    "{name} rounding {v}"
                );
            }
        }
        assert_eq!(x64.supports(row_refusal(row)), Some(true), "{name}");
        assert_eq!(a64.supports(row_refusal(row)), Some(false), "{name}");
        checked += 1;
    }
    // Another signature than LLVM's: vectors of another length.
    let other = "declare <8 x float> @llvm.x86.avx512.mask.getexp.ps.512(<8 x float>, <8 x float>, i16, i32)\n\
                 define void @f(ptr %p) #0 {\n  %v = load <8 x float>, ptr %p, align 1\n  \
                 %r = call <8 x float> @llvm.x86.avx512.mask.getexp.ps.512(<8 x float> %v, <8 x float> %v, i16 -1, i32 4)\n  \
                 store <8 x float> %r, ptr %p, align 1\n  ret void\n}\n";
    let attrs = format!("attributes #0 = {{ \"target-features\"=\"{AVX512_FEATURES}\" }}\n");
    assert_eq!(compile(&format!("{other}{attrs}")).err(), signature);
    // `vcvtneps2bf16` under AVX-NE-CONVERT alone (its VEX form) is not
    // taken: refused by its name.
    for row in 0..NAMES.len() {
        let (_, name, _) = decl(NAMES[row].2);
        if name.starts_with("llvm.x86.vcvtneps2bf16") {
            let base = cases_of(row).swap_remove(0);
            assert_eq!(
                refusal_under(base, "+avx2,+avx,+fma,+f16c,+avxneconvert"),
                Some(CgError::unsupported(row_refusal(row))),
                "{name} under avxneconvert"
            );
        }
    }
    eprintln!("avx512 refusals: {checked} rows");
}

/// The features [`avx512_driver`] checks beyond `avx512f`, in the order
/// of its `have` bits.
const DRIVER_FEATURES: [&str; 10] = [
    "avx512vl",
    "avx512bw",
    "avx512dq",
    "avx512vbmi2",
    "avx512vbmi",
    "gfni",
    "vaes",
    "vpclmulqdq",
    "avx512bf16",
    "avx512fp16",
];

/// The C driver of the execution test: every function of `cases` as hod
/// built it (`h_tK`) and as `llc` 22 did (`l_tK`), run on the same inputs
/// where the CPU has the function's features (`__builtin_cpu_supports`);
/// prints each disagreement and `compared N skipped M`.
fn avx512_driver(cases: &[Case]) -> String {
    let mut c = String::from("#include <stdio.h>\n#include <string.h>\n#include <stdint.h>\n");
    c.push_str("typedef void (*fn)(void *, void *, void *, void *, uint64_t);\n");
    for k in 0..cases.len() {
        writeln!(c, "void h_t{k}(void *, void *, void *, void *, uint64_t);").unwrap();
        writeln!(c, "void l_t{k}(void *, void *, void *, void *, uint64_t);").unwrap();
    }
    c.push_str("static const fn h[] = {");
    (0..cases.len()).for_each(|k| write!(c, "h_t{k},").unwrap());
    c.push_str("};\nstatic const fn l[] = {");
    (0..cases.len()).for_each(|k| write!(c, "l_t{k},").unwrap());
    // Per function: the bytes of the result, and the features it needs
    // (bits: 1 F, 2 VL, 4 BW, 8 DQ, 16 VBMI2, 32 VBMI, 64 GFNI, 128 VAES,
    // 256 VPCLMULQDQ, 512 BF16, 1024 FP16).
    c.push_str("};\nstatic const int info[][2] = {");
    for case in cases {
        let (fam, _, text) = NAMES[case.row];
        let (ret, name, params) = decl(text);
        let bytes = params.iter().chain([&ret]).map(|t| bytes_of(t)).max().unwrap();
        let mut need = 1;
        if bytes < 64 && !name.ends_with(".sd") {
            need |= 2;
        }
        need |= match fam {
            F => 0,
            BW => 4,
            DQ => 8,
            VBMI2 => 4 | 16,
            VBMI => 4 | 32,
            GFNI | Vex("gfni") => 64,
            VAES | Vex("vaes") => 128,
            VPCLMUL | Vex(_) => 256,
            BF16 => 4 | 512,
            FP16 => 2 | 4 | 8 | 1024,
        };
        if let Vex(_) = fam {
            need &= !3;
        }
        write!(c, "{{{},{need}}},", bytes_of(&ret)).unwrap();
    }
    c.push_str(
        "};\n\
         static uint64_t state = 0x2080208;\n\
         static uint64_t next(void) { state ^= state << 13; state ^= state >> 7; state ^= state << 17; return state; }\n\
         /* Inputs of round r: random bits, small floats, small doubles, special values. */\n\
         static void fill(unsigned char *p, int r) {\n  \
           static const uint32_t sf[] = {0, 0x80000000u, 0x7f800000u, 0xff800000u, 0x7fc00000u, 0x00000001u,\n    \
             0x3f800000u, 0xbfc00000u, 0x4f000000u, 0xcf000001u, 0x5f800000u, 0x3effffffu};\n  \
           static const uint64_t sd[] = {0, 0x8000000000000000ull, 0x7ff0000000000000ull, 0x7ff8000000000000ull,\n    \
             1, 0x3ff0000000000000ull, 0xbff8000000000000ull, 0x43e0000000000000ull, 0xc3e0000000000001ull,\n    \
             0x41e0000000000000ull, 0x3fdfffffffffffffull, 0x4330000000000000ull};\n  \
           for (int i = 0; i < 64; i += 8) {\n    \
             uint64_t v = next();\n    \
             if (r % 4 == 1) { float f[2] = {(float)((int)(v % 2001) - 1000) / 8.0f, (float)((int)(v >> 32) % 70000) * 1.25f}; memcpy(p + i, f, 8); }\n    \
             else if (r % 4 == 2) { double d = (double)((int64_t)(v % 4000001) - 2000000) / 16.0; memcpy(p + i, &d, 8); }\n    \
             else if (r % 4 == 3) { if (v & 1) { uint32_t f[2] = {sf[v % 12], sf[(v >> 8) % 12]}; memcpy(p + i, f, 8); } else memcpy(p + i, &sd[(v >> 1) % 12], 8); }\n    \
             else memcpy(p + i, &v, 8);\n  \
           }\n\
         }\n\
         int main(void) {\n  \
           __builtin_cpu_init();\n  \
           int have = 0;\n  \
           if (__builtin_cpu_supports(\"avx512f\")) have |= 1;\n  \
           if (__builtin_cpu_supports(\"avx512vl\")) have |= 2;\n  \
           if (__builtin_cpu_supports(\"avx512bw\")) have |= 4;\n  \
           if (__builtin_cpu_supports(\"avx512dq\")) have |= 8;\n  \
           if (__builtin_cpu_supports(\"avx512vbmi2\")) have |= 16;\n  \
           if (__builtin_cpu_supports(\"avx512vbmi\")) have |= 32;\n  \
           if (__builtin_cpu_supports(\"gfni\")) have |= 64;\n  \
           if (__builtin_cpu_supports(\"vaes\")) have |= 128;\n  \
           if (__builtin_cpu_supports(\"vpclmulqdq\")) have |= 256;\n  \
           if (__builtin_cpu_supports(\"avx512bf16\")) have |= 512;\n  \
           if (__builtin_cpu_supports(\"avx512fp16\")) have |= 1024;\n  \
           long compared = 0, skipped = 0, wrong = 0;\n  \
           for (unsigned f = 0; f < sizeof h / sizeof *h; f++) {\n    \
             if ((info[f][1] & have) != info[f][1]) { skipped++; continue; }\n    \
             for (int r = 0; r < 12; r++) {\n      \
               unsigned char a[64], b[64], c[64], x[64], y[64];\n      \
               fill(a, r); fill(b, r + 1); fill(c, r + 2);\n      \
               uint64_t m = r == 0 ? ~0ull : next();\n      \
               memset(x, 0xcc, 64); memset(y, 0xcc, 64);\n      \
               h[f](x, a, b, c, m); l[f](y, a, b, c, m);\n      \
               compared++;\n      \
               if (memcmp(x, y, info[f][0])) {\n        \
                 wrong++;\n        \
                 printf(\"t%u round %d: hod \", f, r);\n        \
                 for (int i = 0; i < info[f][0]; i++) printf(\"%02x\", x[i]);\n        \
                 printf(\" llc \");\n        \
                 for (int i = 0; i < info[f][0]; i++) printf(\"%02x\", y[i]);\n        \
                 printf(\"\\n\");\n      \
               }\n    \
             }\n  \
           }\n  \
           printf(\"compared %ld skipped %ld wrong %ld\\n\", compared, skipped, wrong);\n  \
           return 0;\n\
         }\n",
    );
    c
}

/// §5.6.35: on a CPU with AVX-512, hod's code of every name computes the
/// bytes `llc` 22's does, on random inputs, small floats and doubles and
/// special values (zeros, infinities, NaNs, denormals, the edges of the
/// integer conversions). The program is linked statically and run by the
/// command `HOD_X64_AVX512_RUNNER` names (the program's path is appended;
/// `native` runs it directly) or, without one, natively on an x86-64
/// processor whose flags have `avx512f` ([`execute_compared`]); otherwise
/// the test only links it and reports the skip: no emulator this suite
/// has executes AVX-512, and that holds also under `HOD_REQUIRE_TOOLS=1`
/// (`HOD_REQUIRE_AVX512=1` makes it a failure). A function whose
/// features the CPU lacks is skipped (`__builtin_cpu_supports`); a runner
/// on which every function is skipped fails. An undefined pass-through is left out: `llc` leaves
/// what a register held where hod writes zeros.
#[test]
fn avx512_names_agree_with_llc_on_hardware() {
    let cases: Vec<Case> =
        all_cases().into_iter().filter(|c| !c.args.contains(&Arg::Undef)).collect();
    let scratch = Scratch::new("x64-avx512-run");
    let Some(llc22) = llc(&scratch, 22, "llc22", &avx512_module(&cases, "l_", AVX512_FEATURES))
    else {
        return;
    };
    let hod = scratch.write("hod.o", hod_object(&cases, "h_", Stress::default()).unwrap());
    let Some(toolchain) = Toolchain::x86_64(&scratch) else { return };
    let driver = toolchain.compile_c(&scratch, "driver", &avx512_driver(&cases), &[]);
    let program = scratch.join("avx512");
    let linker = &toolchain.linkers[0];
    toolchain.link_ok(linker, &["-static"], &[&driver, &hod, &llc22], &program);
    execute_compared("x86_avx512 avx512_names", &program, cases.len(), &DRIVER_FEATURES);
}

/// Runs a driver `program` of `functions` functions that compares hod's
/// code with `llc`'s on the processor (`avx512_driver`, `vsib_driver`) as
/// `hod_testutil::avx512` decides: by `HOD_X64_AVX512_RUNNER`, else
/// natively on an x86-64 processor with `avx512f` (the driver itself
/// skips each function whose other features the processor lacks), else
/// not at all; and reports the outcome under `name`
/// (`HOD_AVX512_REPORT`). A failure or a disagreement fails the test with
/// the program's whole output: hod's bytes and `llc`'s for each
/// disagreement. WP-238: a runner with its processor's flags
/// (`HOD_X64_AVX512_RUNNER_CPUINFO`, an emulator) is preferred to a host
/// that lacks one of `wanted`, the features the driver checks, so that
/// no function is skipped where it can run.
fn execute_compared(name: &str, program: &Path, functions: usize, wanted: &[&str]) {
    use hod_testutil::avx512::{self, Execution};
    let (prefix, how) = match avx512::execution_wanting(&["avx512f"], wanted) {
        Execution::Run { prefix, how } => (prefix, how),
        Execution::DryRun(why) => return avx512::skip(name, "dry-run", &why),
        Execution::Skip(why) => {
            eprintln!("{name}: {functions} functions linked");
            return avx512::skip(name, "skipped", &why);
        }
    };
    let mut command = match prefix.split_first() {
        None => Command::new(program),
        Some((first, rest)) => {
            let mut command = Command::new(first);
            command.args(rest).arg(program);
            command
        }
    };
    let ran = hod_testutil::run_with(&mut command, &[], Duration::from_mins(10));
    let out = ran.stdout_text();
    let last = out.lines().last().unwrap_or("");
    let good = ran.success() && last.starts_with("compared ") && last.ends_with(" wrong 0");
    // A processor on which every function is skipped is no evidence.
    let evidence = !last.starts_with("compared 0 ");
    let status = ran.status.map_or_else(|| "timed out".to_owned(), |s| s.to_string());
    if !(good && evidence) {
        avx512::report(name, "failed", &format!("{how}; {status}; {last}"));
    }
    assert!(
        good,
        "{name} ({how}): the program failed or disagrees: {status}\nstdout:\n{out}\nstderr:\n{}",
        ran.stderr_text()
    );
    assert!(evidence, "{name} ({how}): the processor ran no function: {last}");
    avx512::report(name, "executed", &format!("{how}; {functions} functions; {last}"));
    eprintln!("{name} executed ({how}): {last}");
}

/// The `rbp` displacement of `line`'s memory operand, as `llvm-objdump`
/// prints it (`[rbp - 0x50]`), if it has one.
fn rbp_displacement(line: &str) -> Option<i64> {
    let rest = &line[line.find("[rbp")? + 4..];
    let rest = &rest[..rest.find(']')?];
    let (sign, digits) = match rest.trim() {
        "" => return Some(0),
        r if r.starts_with('-') => (-1, r[1..].trim()),
        r if r.starts_with('+') => (1, r[1..].trim()),
        _ => return None,
    };
    i64::from_str_radix(digits.trim_start_matches("0x"), 16).ok().map(|v| sign * v)
}

/// §5.6.17 and §5.6.35: an EVEX access to a frame local keeps its
/// `disp32` when the frame is laid out. A raw `disp8` there would be
/// scaled by the instruction's `N` (`disp8*N`) and address another place:
/// so every slot the EVEX moves (`vmovdqu64`) read or write in hod's
/// functions, plainly and under every stress option, is one the
/// non-EVEX code around them fills or empties, at the same displacement,
/// and some of them are within a byte of `rbp` (where the shortening
/// would have applied).
#[test]
fn evex_frame_locals_keep_their_displacement() {
    if hod_testutil::tool("llvm-objdump").is_none() {
        return;
    }
    let cases = all_cases();
    let scratch = Scratch::new("x64-avx512-locals");
    let (mut evex, mut near) = (0, 0);
    let mut wrong = Vec::new();
    for (mode, stress) in [("plain", Stress::default()), ("stress", Stress::ALL)] {
        let object = hod_object(&cases, "", stress).unwrap();
        let object = scratch.write(&format!("hod-{mode}.o"), object);
        for (name, listing) in listings(&object) {
            let (moves, others): (Vec<&String>, Vec<&String>) =
                listing.iter().partition(|l| l.trim_start().starts_with("vmovdqu64"));
            let known: Vec<i64> = others.iter().filter_map(|l| rbp_displacement(l)).collect();
            for line in moves {
                let Some(d) = rbp_displacement(line) else { continue };
                evex += 1;
                near += usize::from(i8::try_from(d).is_ok());
                if !known.contains(&d) {
                    wrong.push(format!("{mode} {name}: {}", line.trim()));
                }
            }
        }
    }
    eprintln!("avx512 frame locals: {evex} EVEX accesses, {near} within a byte of rbp");
    assert!(near > 0, "no EVEX access the shortening would have reached");
    assert!(
        wrong.is_empty(),
        "{} EVEX accesses elsewhere:\n{}",
        wrong.len(),
        wrong[..wrong.len().min(12)].join("\n")
    );
}

/// WP-224 (§5.6.35 item 10): the masked gathers and scatters, as LLVM 19
/// and 22 declare them. A gather is `(pass-through, base, indices, mask,
/// scale)`, a scatter `(base, mask, indices, data, scale)`; the mask has
/// a lane per element moved, the fewer of the data's and the indices'.
const VSIB: [&str; 48] = [
    "<8 x double> @llvm.x86.avx512.mask.gather.dpd.512(<8 x double>, ptr, <8 x i32>, <8 x i1>, i32)",
    "<16 x float> @llvm.x86.avx512.mask.gather.dps.512(<16 x float>, ptr, <16 x i32>, <16 x i1>, i32)",
    "<8 x double> @llvm.x86.avx512.mask.gather.qpd.512(<8 x double>, ptr, <8 x i64>, <8 x i1>, i32)",
    "<8 x float> @llvm.x86.avx512.mask.gather.qps.512(<8 x float>, ptr, <8 x i64>, <8 x i1>, i32)",
    "<8 x i64> @llvm.x86.avx512.mask.gather.dpq.512(<8 x i64>, ptr, <8 x i32>, <8 x i1>, i32)",
    "<16 x i32> @llvm.x86.avx512.mask.gather.dpi.512(<16 x i32>, ptr, <16 x i32>, <16 x i1>, i32)",
    "<8 x i64> @llvm.x86.avx512.mask.gather.qpq.512(<8 x i64>, ptr, <8 x i64>, <8 x i1>, i32)",
    "<8 x i32> @llvm.x86.avx512.mask.gather.qpi.512(<8 x i32>, ptr, <8 x i64>, <8 x i1>, i32)",
    "<2 x double> @llvm.x86.avx512.mask.gather3div2.df(<2 x double>, ptr, <2 x i64>, <2 x i1>, i32)",
    "<2 x i64> @llvm.x86.avx512.mask.gather3div2.di(<2 x i64>, ptr, <2 x i64>, <2 x i1>, i32)",
    "<4 x double> @llvm.x86.avx512.mask.gather3div4.df(<4 x double>, ptr, <4 x i64>, <4 x i1>, i32)",
    "<4 x i64> @llvm.x86.avx512.mask.gather3div4.di(<4 x i64>, ptr, <4 x i64>, <4 x i1>, i32)",
    "<4 x float> @llvm.x86.avx512.mask.gather3div4.sf(<4 x float>, ptr, <2 x i64>, <2 x i1>, i32)",
    "<4 x i32> @llvm.x86.avx512.mask.gather3div4.si(<4 x i32>, ptr, <2 x i64>, <2 x i1>, i32)",
    "<4 x float> @llvm.x86.avx512.mask.gather3div8.sf(<4 x float>, ptr, <4 x i64>, <4 x i1>, i32)",
    "<4 x i32> @llvm.x86.avx512.mask.gather3div8.si(<4 x i32>, ptr, <4 x i64>, <4 x i1>, i32)",
    "<2 x double> @llvm.x86.avx512.mask.gather3siv2.df(<2 x double>, ptr, <4 x i32>, <2 x i1>, i32)",
    "<2 x i64> @llvm.x86.avx512.mask.gather3siv2.di(<2 x i64>, ptr, <4 x i32>, <2 x i1>, i32)",
    "<4 x double> @llvm.x86.avx512.mask.gather3siv4.df(<4 x double>, ptr, <4 x i32>, <4 x i1>, i32)",
    "<4 x i64> @llvm.x86.avx512.mask.gather3siv4.di(<4 x i64>, ptr, <4 x i32>, <4 x i1>, i32)",
    "<4 x float> @llvm.x86.avx512.mask.gather3siv4.sf(<4 x float>, ptr, <4 x i32>, <4 x i1>, i32)",
    "<4 x i32> @llvm.x86.avx512.mask.gather3siv4.si(<4 x i32>, ptr, <4 x i32>, <4 x i1>, i32)",
    "<8 x float> @llvm.x86.avx512.mask.gather3siv8.sf(<8 x float>, ptr, <8 x i32>, <8 x i1>, i32)",
    "<8 x i32> @llvm.x86.avx512.mask.gather3siv8.si(<8 x i32>, ptr, <8 x i32>, <8 x i1>, i32)",
    "void @llvm.x86.avx512.mask.scatter.dpd.512(ptr, <8 x i1>, <8 x i32>, <8 x double>, i32)",
    "void @llvm.x86.avx512.mask.scatter.dps.512(ptr, <16 x i1>, <16 x i32>, <16 x float>, i32)",
    "void @llvm.x86.avx512.mask.scatter.qpd.512(ptr, <8 x i1>, <8 x i64>, <8 x double>, i32)",
    "void @llvm.x86.avx512.mask.scatter.qps.512(ptr, <8 x i1>, <8 x i64>, <8 x float>, i32)",
    "void @llvm.x86.avx512.mask.scatter.dpq.512(ptr, <8 x i1>, <8 x i32>, <8 x i64>, i32)",
    "void @llvm.x86.avx512.mask.scatter.dpi.512(ptr, <16 x i1>, <16 x i32>, <16 x i32>, i32)",
    "void @llvm.x86.avx512.mask.scatter.qpq.512(ptr, <8 x i1>, <8 x i64>, <8 x i64>, i32)",
    "void @llvm.x86.avx512.mask.scatter.qpi.512(ptr, <8 x i1>, <8 x i64>, <8 x i32>, i32)",
    "void @llvm.x86.avx512.mask.scatterdiv2.df(ptr, <2 x i1>, <2 x i64>, <2 x double>, i32)",
    "void @llvm.x86.avx512.mask.scatterdiv2.di(ptr, <2 x i1>, <2 x i64>, <2 x i64>, i32)",
    "void @llvm.x86.avx512.mask.scatterdiv4.df(ptr, <4 x i1>, <4 x i64>, <4 x double>, i32)",
    "void @llvm.x86.avx512.mask.scatterdiv4.di(ptr, <4 x i1>, <4 x i64>, <4 x i64>, i32)",
    "void @llvm.x86.avx512.mask.scatterdiv4.sf(ptr, <2 x i1>, <2 x i64>, <4 x float>, i32)",
    "void @llvm.x86.avx512.mask.scatterdiv4.si(ptr, <2 x i1>, <2 x i64>, <4 x i32>, i32)",
    "void @llvm.x86.avx512.mask.scatterdiv8.sf(ptr, <4 x i1>, <4 x i64>, <4 x float>, i32)",
    "void @llvm.x86.avx512.mask.scatterdiv8.si(ptr, <4 x i1>, <4 x i64>, <4 x i32>, i32)",
    "void @llvm.x86.avx512.mask.scattersiv2.df(ptr, <2 x i1>, <4 x i32>, <2 x double>, i32)",
    "void @llvm.x86.avx512.mask.scattersiv2.di(ptr, <2 x i1>, <4 x i32>, <2 x i64>, i32)",
    "void @llvm.x86.avx512.mask.scattersiv4.df(ptr, <4 x i1>, <4 x i32>, <4 x double>, i32)",
    "void @llvm.x86.avx512.mask.scattersiv4.di(ptr, <4 x i1>, <4 x i32>, <4 x i64>, i32)",
    "void @llvm.x86.avx512.mask.scattersiv4.sf(ptr, <4 x i1>, <4 x i32>, <4 x float>, i32)",
    "void @llvm.x86.avx512.mask.scattersiv4.si(ptr, <4 x i1>, <4 x i32>, <4 x i32>, i32)",
    "void @llvm.x86.avx512.mask.scattersiv8.sf(ptr, <8 x i1>, <8 x i32>, <8 x float>, i32)",
    "void @llvm.x86.avx512.mask.scattersiv8.si(ptr, <8 x i1>, <8 x i32>, <8 x i32>, i32)",
];

/// One test function of a gather or a scatter: `void tK(ptr %out, ptr %a,
/// ptr %b, ptr %c, i64 %m)`, the indices loaded from `%b`. A gather
/// reads through `%a`, its pass-through from `%c`, and stores to `%out`;
/// a scatter stores the data at `%c` through `%out`. `twice`: the call
/// again with the same mask (a gather with the first result as its
/// pass-through, stored after it; a scatter through `%a` first), so the
/// mask, which the instruction clears, must be a copy.
#[derive(Clone, Copy, Debug)]
struct VsibCase {
    row: usize,
    /// [`Arg::Var`] (the low bits of `%m`) or [`Arg::Const`].
    mask: Arg,
    /// A gather's pass-through: [`Arg::Load`], [`Arg::Zero`], [`Arg::Undef`].
    pass: Arg,
    scale: i64,
    twice: bool,
}

/// Row `row`'s name, whether it gathers, and its data, index and mask types.
fn vsib_types(row: usize) -> (String, bool, String, String, String) {
    let (ret, name, p) = decl(VSIB[row]);
    if ret == "void" {
        (name, false, p[3].clone(), p[2].clone(), p[1].clone())
    } else {
        (name, true, p[0].clone(), p[2].clone(), p[3].clone())
    }
}

/// The test functions of every row: a variable mask, constant masks (all
/// set, lanes 0 and 2, none), a zero and an undefined pass-through, each
/// other scale, the call twice.
fn vsib_cases() -> Vec<VsibCase> {
    let mut cases = Vec::new();
    for row in 0..VSIB.len() {
        let gather = vsib_types(row).1;
        let base = VsibCase { row, mask: Arg::Var, pass: Arg::Load, scale: 4, twice: false };
        cases.push(base);
        for bits in [-1, 5, 0] {
            cases.push(VsibCase { mask: Arg::Const(bits), ..base });
        }
        if gather {
            for pass in [Arg::Zero, Arg::Undef] {
                cases.push(VsibCase { pass, ..base });
            }
        }
        for scale in [1, 2, 8] {
            cases.push(VsibCase { scale, ..base });
        }
        cases.push(VsibCase { twice: true, ..base });
    }
    cases
}

/// The module of `cases`, with `features`.
fn vsib_module(cases: &[VsibCase], prefix: &str, features: &str) -> String {
    let mut text = ll_header();
    let mut declared = std::collections::BTreeSet::new();
    for (k, c) in cases.iter().enumerate() {
        let (name, gather, data, index, mask) = vsib_types(c.row);
        if declared.insert(name.clone()) {
            writeln!(text, "declare {}", VSIB[c.row]).unwrap();
        }
        writeln!(text, "define void @{prefix}t{k}(ptr %out, ptr %a, ptr %b, ptr %c, i64 %m) #0 {{")
            .unwrap();
        writeln!(text, "  %ix = load {index}, ptr %b, align 1").unwrap();
        let (n, _) = vector_of(&mask).unwrap();
        let bits = match c.mask {
            Arg::Var => {
                writeln!(text, "  %t = trunc i64 %m to i{n}\n  %k = bitcast i{n} %t to {mask}")
                    .unwrap();
                "%k".to_string()
            }
            Arg::Const(bits) => mask_const(n, if bits == -1 { u64::MAX } else { bits as u64 }),
            other => panic!("a mask {other:?}"),
        };
        let scale = c.scale;
        if gather {
            let pass = match c.pass {
                Arg::Load => {
                    writeln!(text, "  %p = load {data}, ptr %c, align 1").unwrap();
                    "%p"
                }
                Arg::Zero => "zeroinitializer",
                Arg::Undef => "undef",
                other => panic!("a pass-through {other:?}"),
            };
            writeln!(
                text,
                "  %r = call {data} @{name}({data} {pass}, ptr %a, {index} %ix, {mask} {bits}, i32 {scale})\n  \
                 store {data} %r, ptr %out, align 1"
            )
            .unwrap();
            if c.twice {
                writeln!(
                    text,
                    "  %r2 = call {data} @{name}({data} %r, ptr %a, {index} %ix, {mask} {bits}, i32 {scale})\n  \
                     %o2 = getelementptr i8, ptr %out, i64 {}\n  \
                     store {data} %r2, ptr %o2, align 1",
                    bytes_of(&data)
                )
                .unwrap();
            }
        } else {
            writeln!(text, "  %d = load {data}, ptr %c, align 1").unwrap();
            let call = |to: &str| {
                format!(
                    "  call void @{name}(ptr {to}, {mask} {bits}, {index} %ix, {data} %d, i32 {scale})"
                )
            };
            if c.twice {
                writeln!(text, "{}", call("%a")).unwrap();
            }
            writeln!(text, "{}", call("%out")).unwrap();
        }
        writeln!(text, "  ret void\n}}").unwrap();
    }
    writeln!(text, "attributes #0 = {{ \"target-features\"=\"{features}\" }}").unwrap();
    text
}

/// hod's object of `cases` with `stress` and `features`, or the refusal.
fn vsib_object(
    cases: &[VsibCase],
    prefix: &str,
    features: &str,
    stress: Stress,
) -> Result<Vec<u8>, hod_x64::ModuleError> {
    let (module, bodies) = parse(&vsib_module(cases, prefix, features));
    hod_x64::compile_to_object(&module, &bodies, &pic(stress))
}

/// What a listing of case `c` does: the bytes stored to `%out` by a
/// gather (both results when `twice`), and the lanes the scatters stored.
type VsibResult = (Vec<dataflow::Sym>, Vec<u32>);

fn vsib_run(graph: &mut dataflow::Graph, c: &VsibCase, listing: &[String]) -> VsibResult {
    let (_, gather, data, _, _) = vsib_types(c.row);
    let bytes = if gather { bytes_of(&data) * if c.twice { 2 } else { 1 } } else { 0 };
    dataflow::run_with_events(graph, listing, bytes, 64)
}

fn vsib_results(
    graph: &mut dataflow::Graph,
    cases: &[VsibCase],
    listings: &std::collections::HashMap<String, Vec<String>>,
) -> Vec<VsibResult> {
    cases.iter().enumerate().map(|(k, c)| vsib_run(graph, c, &listings[&format!("t{k}")])).collect()
}

/// §5.6.35 item 10: for every gather and scatter, in every test function
/// of [`vsib_cases`], hod's code (plainly and under every stress option)
/// stores what `llc -O0` 22's and 19's code stores: the gathered bytes as
/// trees over the inputs, each lane a `gather` over the base pointer, its
/// index, the scale, the displacement, the element and index sizes and
/// (under a mask bit that is not constant) the mask bit and the
/// pass-through; and the scattered lanes in order, alike. hod must agree
/// with each `llc`, not merely one of them.
#[test]
fn vsib_names_agree_with_llc_by_dataflow() {
    let cases = vsib_cases();
    let scratch = Scratch::new("x64-avx512-vsib-dataflow");
    let text = vsib_module(&cases, "", AVX512_FEATURES);
    let mut legs: Vec<(String, std::path::PathBuf)> = Vec::new();
    for major in [22, 19] {
        if let Some(object) = llc(&scratch, major, &format!("llc{major}"), &text) {
            legs.push((format!("llc {major}"), object));
        }
    }
    if legs.is_empty() || hod_testutil::tool("llvm-objdump").is_none() {
        return;
    }
    let mut refused = Vec::new();
    for row in 0..VSIB.len() {
        let one: Vec<VsibCase> = cases.iter().filter(|c| c.row == row).copied().collect();
        if let Err(e) = vsib_object(&one, "", AVX512_FEATURES, Stress::default()) {
            refused.push(format!("{}: {e}", vsib_types(row).0));
        }
    }
    assert!(refused.is_empty(), "{} rows refused:\n{}", refused.len(), refused.join("\n"));
    let references = legs.len();
    for (mode, stress) in [("plain", Stress::default()), ("stress", Stress::ALL)] {
        let object = vsib_object(&cases, "", AVX512_FEATURES, stress).unwrap();
        legs.push((format!("hod {mode}"), scratch.write(&format!("hod-{mode}.o"), object)));
    }
    let mut graph = dataflow::Graph::default();
    let all: Vec<(String, Vec<VsibResult>)> = legs
        .iter()
        .map(|(leg, object)| (leg.clone(), vsib_results(&mut graph, &cases, &listings(object))))
        .collect();
    let mut wrong = Vec::new();
    let mut counts = Vec::new();
    for (leg, got) in &all[references..] {
        let mut n = 0;
        for (k, got) in got.iter().enumerate() {
            let differ: Vec<&String> =
                all[..references].iter().filter(|(_, r)| &r[k] != got).map(|(l, _)| l).collect();
            if differ.is_empty() {
                continue;
            }
            n += 1;
            let mut text = format!("{leg} t{k} {:?} differs from {differ:?}:", cases[k]);
            for (reference_leg, r) in &all[..references] {
                write!(text, "\n  {reference_leg}: {} events {:?}", graph.tree(&r[k].0), r[k].1)
                    .unwrap();
            }
            write!(text, "\n  {leg}: {} events {:?}", graph.tree(&got.0), got.1).unwrap();
            wrong.push(text);
        }
        counts.push(format!("{leg}: {n}"));
    }
    // Not vacuous: the lanes gathered and scattered in `llc` 22's code.
    let reference = &all[0].1;
    let gathered: usize = reference
        .iter()
        .map(|(bytes, _)| {
            let mut ids: Vec<u32> = bytes
                .iter()
                .filter_map(|s| if let dataflow::Src::Node(n) = s.src { Some(n) } else { None })
                .collect();
            ids.dedup();
            ids.len()
        })
        .sum();
    let scattered: usize = reference.iter().map(|(_, e)| e.len()).sum();
    eprintln!(
        "vsib dataflow: {} names, {} functions, {gathered} lanes gathered and {scattered} scattered \
         (in {}), references {:?}, disagreements {counts:?}",
        VSIB.len(),
        cases.len(),
        all[0].0,
        all[..references].iter().map(|l| &l.0).collect::<Vec<_>>(),
    );
    assert!(gathered > 0 && scattered > 0);
    assert!(
        wrong.is_empty(),
        "{} disagreements:\n{}",
        wrong.len(),
        wrong[..wrong.len().min(12)].join("\n")
    );
}

/// The parts of a VSIB line of `llvm-objdump`: mnemonic, operands.
fn vsib_line(l: &str) -> Option<(&str, &str)> {
    let (head, ops) = l.split_once('\t')?;
    (head.contains("gather") || head.contains("scatter")).then_some((head, ops))
}

/// The memory operand's `[...]` of a VSIB line: start and end of the
/// text inside.
fn vsib_brackets(l: &str) -> Option<(usize, usize)> {
    Some((l.find('[')? + 1, l.find(']')?))
}

/// The vector index register's number and where its name starts in `l`.
fn vsib_index(l: &str) -> Option<(u8, usize)> {
    let (open, close) = vsib_brackets(l)?;
    let at = open + l[open..close].find("mm")? - 1;
    let digits: String = l[at + 3..close].chars().take_while(char::is_ascii_digit).collect();
    Some((digits.parse().ok()?, at))
}

/// The data register of a VSIB line (a gather's destination, a scatter's
/// source): its name's start and length.
fn vsib_data(l: &str) -> Option<(usize, usize)> {
    let (head, ops) = vsib_line(l)?;
    let base = head.len() + 1;
    let (at, name) = if head.contains("scatter") {
        let src = ops.rsplit(", ").next()?;
        (ops.len() - src.len(), src)
    } else {
        (0, ops.split_whitespace().next()?)
    };
    Some((base + at, name.len()))
}

/// The opmask register of a VSIB line.
fn vsib_k(l: &str) -> Option<u8> {
    let at = l.find("{k")?;
    l[at + 2..at + 3].parse().ok()
}

/// §5.6.35 item 10: the comparison bites. `llc` 22's listings with one
/// deliberate change to a gather or a scatter give another result than
/// the unchanged listing, for every kind of change named: another scale,
/// another index register, another mask register, the mask not copied
/// (the first of two calls reading the opmask the second reads, which the
/// first clears), another element size, the index and data registers
/// swapped, another base register, another displacement. The base and
/// the index cannot be swapped as such (a VSIB base is a general
/// register, the index a vector one; the swap does not assemble): the
/// two mutations of the base and of the index registers stand for it.
#[test]
fn vsib_comparison_catches_mutations() {
    let cases = vsib_cases();
    let scratch = Scratch::new("x64-avx512-vsib-mutations");
    let text = vsib_module(&cases, "", AVX512_FEATURES);
    let Some(object) = llc(&scratch, 22, "llc22", &text) else { return };
    if hod_testutil::tool("llvm-objdump").is_none() {
        return;
    }
    let listings = listings(&object);
    let mut graph = dataflow::Graph::default();
    let reference = vsib_results(&mut graph, &cases, &listings);
    // Each mutation rewrites a listing, once per line it applies to
    // (`None` where it does not), given the listing and the line.
    type Mutation = (&'static str, fn(&[String], usize) -> Option<String>);
    let mutations: &[Mutation] = &[
        ("another scale", |ls, i| {
            let l = &ls[i];
            vsib_line(l)?;
            let (open, close) = vsib_brackets(l)?;
            if let Some(star) = l[open..close].find('*') {
                let at = open + star - 1;
                let to = match &l[at..=at] {
                    "1" => "2",
                    "2" => "4",
                    "4" => "8",
                    _ => "1",
                };
                Some(format!("{}{to}{}", &l[..at], &l[at + 1..]))
            } else {
                let (_, at) = vsib_index(l)?;
                Some(format!("{}2*{}", &l[..at], &l[at..]))
            }
        }),
        ("another index register", |ls, i| {
            let l = &ls[i];
            vsib_line(l)?;
            let (n, at) = vsib_index(l)?;
            let digits = n.to_string().len();
            Some(format!("{}{}{}", &l[..at + 3], n ^ 8, &l[at + 3 + digits..]))
        }),
        ("index and data registers swapped", |ls, i| {
            let l = &ls[i];
            vsib_line(l)?;
            let (n, at) = vsib_index(l)?;
            let ilen = 3 + n.to_string().len();
            let (d, dlen) = vsib_data(l)?;
            let data: u8 = l[d + 3..d + dlen].parse().ok()?;
            if data == n {
                return None;
            }
            // Each name keeps its width; only the numbers trade places.
            let iname = format!("{}{data}", &l[at..at + 3]);
            let dname = format!("{}{n}", &l[d..d + 3]);
            let mut s = l.clone();
            if d > at {
                s.replace_range(d..d + dlen, &dname);
                s.replace_range(at..at + ilen, &iname);
            } else {
                s.replace_range(at..at + ilen, &iname);
                s.replace_range(d..d + dlen, &dname);
            }
            Some(s)
        }),
        ("another mask register", |ls, i| {
            let l = &ls[i];
            vsib_line(l)?;
            let k = vsib_k(l)?;
            let to = if k == 7 { 6 } else { 7 };
            Some(l.replace(&format!("{{k{k}}}"), &format!("{{k{to}}}")))
        }),
        ("mask not copied", |ls, i| {
            let l = &ls[i];
            vsib_line(l)?;
            let later = ls[i + 1..].iter().find(|x| vsib_line(x).is_some())?;
            let (k, k2) = (vsib_k(l)?, vsib_k(later)?);
            (k != k2).then(|| l.replace(&format!("{{k{k}}}"), &format!("{{k{k2}}}")))
        }),
        ("another element size", |ls, i| {
            let l = &ls[i];
            let (head, ops) = vsib_line(l)?;
            let swapped = if let Some(h) = head.strip_suffix("pd") {
                format!("{h}ps")
            } else if let Some(h) = head.strip_suffix("ps") {
                format!("{h}pd")
            } else if let Some(h) = head.strip_suffix('d') {
                format!("{h}q")
            } else {
                format!("{}d", head.strip_suffix('q')?)
            };
            Some(format!("{swapped}\t{ops}"))
        }),
        ("another base register", |ls, i| {
            let l = &ls[i];
            vsib_line(l)?;
            let (open, _) = vsib_brackets(l)?;
            let base = l[open..].split_whitespace().next()?;
            let to = if base == "rdx" { "rsi" } else { "rdx" };
            Some(format!("{}{to}{}", &l[..open], &l[open + base.len()..]))
        }),
        ("another displacement", |ls, i| {
            let l = &ls[i];
            vsib_line(l)?;
            let (_, close) = vsib_brackets(l)?;
            Some(format!("{} + 0x40{}", &l[..close], &l[close..]))
        }),
    ];
    for (what, mutate) in mutations {
        let (mut tried, mut caught) = (0, 0);
        let mut missed = Vec::new();
        for (k, c) in cases.iter().enumerate() {
            // Under a mask of 0 the instruction moves nothing: only
            // another mask register changes that.
            if c.mask == Arg::Const(0) && *what != "another mask register" {
                continue;
            }
            let listing = &listings[&format!("t{k}")];
            for i in 0..listing.len() {
                let Some(changed) = mutate(listing, i) else { continue };
                let mut copy = listing.clone();
                copy[i] = changed;
                tried += 1;
                if vsib_run(&mut graph, c, &copy) == reference[k] {
                    missed.push(format!("t{k} {c:?}: {} -> {}", listing[i], copy[i]));
                } else {
                    caught += 1;
                }
            }
        }
        eprintln!("vsib mutation {what}: {caught} of {tried} caught");
        assert!(tried > 0, "{what}: applies nowhere");
        assert!(missed.is_empty(), "{what} missed:\n{}", missed.join("\n"));
    }
}

/// §5.6.35 item 10: each gather and scatter is refused by its name in a
/// function without `avx512f`, without `avx512vl` for a 128- or 256-bit
/// form, or with `evex512` off for a 512-bit one; a scale other than 1,
/// 2, 4 or 8 is `x64:x86-intrinsic-signature`. The rows are x86-64 ones,
/// not built on AArch64.
#[test]
fn vsib_refusals_and_rows() {
    let signature = Some(CgError::unsupported("x64:x86-intrinsic-signature"));
    let x64 = hod_ir::Support::shipped(hod_ir::Arch::X86_64);
    let a64 = hod_ir::Support::shipped(hod_ir::Arch::Aarch64);
    let under =
        |c: VsibCase, features: &str| match vsib_object(&[c], "", features, Stress::default()) {
            Ok(_) => None,
            Err(hod_x64::ModuleError::Function { error, .. }) => Some(error),
            Err(other) => panic!("{other}"),
        };
    let no_f = "+avx2,+avx,+fma,+f16c";
    let (no_vl, no_512) =
        (format!("{AVX512_FEATURES},-avx512vl"), format!("{AVX512_FEATURES},-evex512"));
    let mut checked = 0;
    for row in 0..VSIB.len() {
        let (name, _, data, index, _) = vsib_types(row);
        let refusal: &'static str = Box::leak(format!("intrinsic:{name}").into_boxed_str());
        let refused = Some(CgError::unsupported(refusal));
        let base = VsibCase { row, mask: Arg::Var, pass: Arg::Load, scale: 4, twice: false };
        assert_eq!(under(base, AVX512_FEATURES), None, "{name}");
        assert_eq!(under(base, no_f), refused, "{name} without avx512f");
        let wide = bytes_of(&data).max(bytes_of(&index)) == 64;
        assert_eq!(under(base, &no_vl), if wide { None } else { refused.clone() }, "{name} -vl");
        assert_eq!(under(base, &no_512), if wide { refused.clone() } else { None }, "{name} -512");
        for scale in [0, 3, 16] {
            assert_eq!(
                under(VsibCase { scale, ..base }, AVX512_FEATURES),
                signature,
                "{name} {scale}"
            );
        }
        assert_eq!(x64.supports(refusal), Some(true), "{name}");
        assert_eq!(a64.supports(refusal), Some(false), "{name}");
        checked += 1;
    }
    eprintln!("vsib refusals: {checked} rows");
}

/// The C driver of the gathers' and scatters' execution test: every
/// function of `cases` as hod built it (`h_tK`) and as `llc` 22 did
/// (`l_tK`), run where the CPU has the features. Each call gets a table
/// of 1024 random bytes (`%a` its middle), indices in -32..32 (so every
/// lane, at any scale, stays in the tables), random data or
/// pass-through and a random mask (all set in the first round), and an
/// output table of 1024 bytes (`%out` its middle); both tables are
/// compared after the calls. Prints each disagreement and `compared N
/// skipped M wrong W`.
fn vsib_driver(cases: &[VsibCase]) -> String {
    let mut c = String::from("#include <stdio.h>\n#include <string.h>\n#include <stdint.h>\n");
    c.push_str("typedef void (*fn)(void *, void *, void *, void *, uint64_t);\n");
    for k in 0..cases.len() {
        writeln!(c, "void h_t{k}(void *, void *, void *, void *, uint64_t);").unwrap();
        writeln!(c, "void l_t{k}(void *, void *, void *, void *, uint64_t);").unwrap();
    }
    c.push_str("static const fn h[] = {");
    (0..cases.len()).for_each(|k| write!(c, "h_t{k},").unwrap());
    c.push_str("};\nstatic const fn l[] = {");
    (0..cases.len()).for_each(|k| write!(c, "l_t{k},").unwrap());
    // Per function: the bytes of an index, the features (1 F, 2 VL).
    c.push_str("};\nstatic const int info[][2] = {");
    for case in cases {
        let (_, _, data, index, _) = vsib_types(case.row);
        let (_, elem) = vector_of(&index).unwrap();
        let need = if bytes_of(&data).max(bytes_of(&index)) == 64 { 1 } else { 3 };
        write!(c, "{{{},{need}}},", bits_of(elem) / 8).unwrap();
    }
    c.push_str(
        "};\n\
         static uint64_t state = 0x2240224;\n\
         static uint64_t next(void) { state ^= state << 13; state ^= state >> 7; state ^= state << 17; return state; }\n\
         static unsigned char oh[1024], ol[1024], th[1024], tl[1024], ix[64], d[64];\n\
         int main(void) {\n  \
           __builtin_cpu_init();\n  \
           int have = 0;\n  \
           if (__builtin_cpu_supports(\"avx512f\")) have |= 1;\n  \
           if (__builtin_cpu_supports(\"avx512vl\")) have |= 2;\n  \
           long compared = 0, skipped = 0, wrong = 0;\n  \
           for (unsigned f = 0; f < sizeof h / sizeof *h; f++) {\n    \
             if ((info[f][1] & have) != info[f][1]) { skipped++; continue; }\n    \
             for (int r = 0; r < 12; r++) {\n      \
               for (int i = 0; i < 1024; i += 8) { uint64_t v = next(); memcpy(th + i, &v, 8); }\n      \
               memcpy(tl, th, 1024); memset(oh, 0xcc, 1024); memset(ol, 0xcc, 1024);\n      \
               for (int i = 0; i < 64; i += info[f][0]) { int64_t v = (int64_t)(next() % 64) - 32; memcpy(ix + i, &v, info[f][0]); }\n      \
               for (int i = 0; i < 64; i += 8) { uint64_t v = next(); memcpy(d + i, &v, 8); }\n      \
               uint64_t m = r == 0 ? ~0ull : next();\n      \
               h[f](oh + 512, th + 512, ix, d, m); l[f](ol + 512, tl + 512, ix, d, m);\n      \
               compared++;\n      \
               if (memcmp(oh, ol, 1024) || memcmp(th, tl, 1024)) {\n        \
                 wrong++;\n        \
                 printf(\"t%u round %d differs\\n\", f, r);\n      \
               }\n    \
             }\n  \
           }\n  \
           printf(\"compared %ld skipped %ld wrong %ld\\n\", compared, skipped, wrong);\n  \
           return 0;\n\
         }\n",
    );
    c
}

/// §5.6.35 item 10: on a CPU with AVX-512, hod's gathers and scatters
/// move the bytes `llc` 22's do (see [`vsib_driver`]). As for
/// [`avx512_names_agree_with_llc_on_hardware`], the program is linked
/// statically and run as [`execute_compared`] decides: by the command
/// `HOD_X64_AVX512_RUNNER` names, or natively on a processor with
/// `avx512f`; otherwise the test only links it. An
/// undefined pass-through is left out.
#[test]
fn vsib_names_agree_with_llc_on_hardware() {
    let cases: Vec<VsibCase> = vsib_cases().into_iter().filter(|c| c.pass != Arg::Undef).collect();
    let scratch = Scratch::new("x64-avx512-vsib-run");
    let Some(llc22) = llc(&scratch, 22, "llc22", &vsib_module(&cases, "l_", AVX512_FEATURES))
    else {
        return;
    };
    let hod = vsib_object(&cases, "h_", AVX512_FEATURES, Stress::default()).unwrap();
    let hod = scratch.write("hod.o", hod);
    let Some(toolchain) = Toolchain::x86_64(&scratch) else { return };
    let driver = toolchain.compile_c(&scratch, "driver", &vsib_driver(&cases), &[]);
    let program = scratch.join("vsib");
    let linker = &toolchain.linkers[0];
    toolchain.link_ok(linker, &["-static"], &[&driver, &hod, &llc22], &program);
    execute_compared("x86_avx512 vsib", &program, cases.len(), &["avx512vl"]);
}
