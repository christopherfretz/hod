//! Layout differential tests (`spec/ir.md` §3): the type table's sizes,
//! alignments and offsets against LLVM's, for several hundred types on both
//! targets, with LLVM 22 and LLVM 19.
//!
//! The oracle is `llc`: a global array of `ptrtoint (getelementptr (T, ptr
//! null, ...))` expressions is folded to numbers by LLVM's own `DataLayout`
//! and read back from `.data`; common symbols carry the alignment LLVM gives
//! a global variable of each type (`llvm-readelf -s` shows it as the value)
//! and its alloc size. One `llc` run per target and LLVM version.
//!
//! Not covered here: store sizes (no constant expression observes them;
//! `flatten_diff.rs` does, through the bytes of emitted values) and scalable
//! vectors (LLVM rejects them in constant `getelementptr`s).

mod common;

use std::fmt::Write as _;

use common::{CONFIGS, Config, Place, Ty, type_corpus};
use hod_ir::TypeTable;
use hod_testutil::Scratch;

/// Types per configuration: the hand-picked ones and random ones.
const TYPE_COUNT: usize = 400;

/// A number `llc` computes and the type table must agree with.
struct Query {
    what: String,
    expected: u64,
}

/// A common symbol and what the type table says of it.
struct Common {
    symbol: String,
    what: String,
    align: u64,
    size: u64,
}

/// The module `llc` compiles and the answers expected from it.
#[derive(Default)]
struct Plan {
    /// The elements of `@q`, one `i64` per query.
    elements: String,
    queries: Vec<Query>,
    /// The definitions of the common symbols `@c<n>` and `@d<n>`.
    definitions: String,
    commons: Vec<Common>,
}

impl Plan {
    fn ask(&mut self, what: String, gep: &str, expected: u64) {
        let separator = if self.queries.is_empty() { "" } else { ",\n" };
        let _ = write!(self.elements, "{separator}  i64 ptrtoint (ptr {gep} to i64)");
        self.queries.push(Query { what, expected });
    }

    /// Adds the questions about one type.
    fn add(&mut self, types: &mut TypeTable, index: usize, ty: &Ty) {
        let id = ty.intern(types);
        let text = ty.text();
        assert!(types.is_sized(id) && !types.is_scalable(id));
        self.ask(
            format!("alloc size of {text}"),
            &format!("getelementptr ({text}, ptr null, i32 1)"),
            types.alloc_size_of(id),
        );
        // A `T` after one byte sits at T's ABI alignment.
        self.ask(
            format!("ABI alignment of {text}"),
            &format!("getelementptr ({{ i8, {text} }}, ptr null, i32 0, i32 1)"),
            types.align_of(id),
        );
        match ty {
            Ty::Struct(fields, _) => {
                for field in 0..fields.len() as u32 {
                    self.ask(
                        format!("offset of field {field} of {text}"),
                        &format!("getelementptr ({text}, ptr null, i32 0, i32 {field})"),
                        types.field_offset(id, field),
                    );
                }
            }
            Ty::Array(..) | Ty::Vector(..) => {
                let len = match ty {
                    Ty::Array(_, len) => *len,
                    Ty::Vector(_, len) => u64::from(*len),
                    _ => 0,
                };
                // A vector whose elements are not whole bytes has no
                // element addresses.
                if let (Some(last), Some(_)) = (len.checked_sub(1), types.element_stride(id)) {
                    self.ask(
                        format!("offset of element {last} of {text}"),
                        &format!("getelementptr ({text}, ptr null, i32 0, i64 {last})"),
                        types.element_offset(id, last).expect("a stride"),
                    );
                }
            }
            _ => {}
        }
        // LLVM gives a zero-sized common symbol size 1; leave those out.
        let size = types.alloc_size_of(id);
        if size > 0 {
            let _ = writeln!(self.definitions, "@c{index} = common global {text} zeroinitializer");
            self.commons.push(Common {
                symbol: format!("c{index}"),
                what: text.clone(),
                align: types.global_align(id, None, false),
                size,
            });
            let explicit = 1u64 << (index % 6);
            let _ = writeln!(
                self.definitions,
                "@d{index} = common global {text} zeroinitializer, align {explicit}"
            );
            self.commons.push(Common {
                symbol: format!("d{index}"),
                what: format!("{text}, align {explicit}"),
                align: types.global_align(id, Some(explicit), false),
                size,
            });
        }
    }
}

fn check(config: &Config) {
    let Some(tools) = common::tools(config.llvm) else { return };
    let mut types = config.type_table();
    let corpus = type_corpus(0x1a70_07d1, TYPE_COUNT);
    let mut plan = Plan::default();
    for (index, ty) in corpus.iter().enumerate() {
        plan.add(&mut types, index, ty);
    }
    let ir = format!(
        "{}@q = global [{} x i64] [\n{}\n]\n{}",
        config.header(),
        plan.queries.len(),
        plan.elements,
        plan.definitions
    );

    let scratch = Scratch::new(&format!("layout-{}", config.name()));
    let object_path = tools.compile(&scratch, "layout", &ir, false).unwrap_or_else(|error| {
        panic!("{}: llc rejected the layout module:\n{error}", config.name())
    });
    let data = tools.section_bytes(&object_path, ".data");
    let object = tools.read_object(&object_path);
    let table = object.symbol("q");
    assert_eq!(table.size, plan.queries.len() as u64 * 8, "size of the query table");
    let values = data[table.value as usize..][..plan.queries.len() * 8]
        .chunks_exact(8)
        .map(|chunk| u64::from_le_bytes(chunk.try_into().expect("eight bytes")));

    let mut failures = Vec::new();
    for (query, actual) in plan.queries.iter().zip(values) {
        if query.expected != actual {
            failures.push(format!("{}: hod {}, LLVM {actual}", query.what, query.expected));
        }
    }
    for Common { symbol, what, align, size } in &plan.commons {
        let llvm = object.symbol(symbol);
        assert_eq!(llvm.place, Place::Common, "{symbol} is not a common symbol");
        if llvm.value != *align {
            failures
                .push(format!("alignment of a global {what}: hod {align}, LLVM {}", llvm.value));
        }
        if llvm.size != *size {
            failures.push(format!("size of a global {what}: hod {size}, LLVM {}", llvm.size));
        }
    }
    eprintln!(
        "layout_diff {}: {} types, {} sizes/alignments/offsets, {} global alignments and sizes",
        config.name(),
        corpus.len(),
        plan.queries.len(),
        plan.commons.len() * 2,
    );
    assert!(
        failures.is_empty(),
        "{}: {} layout mismatches, the first of them:\n{}",
        config.name(),
        failures.len(),
        failures.iter().take(40).cloned().collect::<Vec<_>>().join("\n")
    );
}

/// §3 layout on aarch64 against LLVM 22.
#[test]
fn layout_matches_llvm22_on_aarch64() {
    check(&CONFIGS[0]);
}

/// §3 layout on x86-64 against LLVM 22.
#[test]
fn layout_matches_llvm22_on_x86_64() {
    check(&CONFIGS[1]);
}

/// §3 layout on aarch64 against LLVM 19 and its data-layout string.
#[test]
fn layout_matches_llvm19_on_aarch64() {
    check(&CONFIGS[2]);
}

/// §3 layout on x86-64 against LLVM 19.
#[test]
fn layout_matches_llvm19_on_x86_64() {
    check(&CONFIGS[3]);
}
