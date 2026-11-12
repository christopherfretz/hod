//! A measurement, not a gate (`spec/ir.md` §2.2): declaring 100,000
//! symbols with names as long as mangled C++ names are, describing them,
//! and looking every one up again. The times are printed (`cargo test
//! --release -p hod_ir --test symbol_perf -- --nocapture`); what the
//! test asserts is only that the table gives back what went in.

use std::time::Instant;

use hod_ir::{FunctionDesc, Linkage, Module, SymId, TargetSpec, TypeId, VariableDesc};

const SYMBOLS: usize = 100_000;
const SYMBOLS_F64: f64 = 100_000.0;

/// A deterministic generator (a 64-bit LCG, high bits).
struct Lcg(u64);

impl Lcg {
    fn next(&mut self) -> u64 {
        self.0 =
            self.0.wrapping_mul(6_364_136_223_846_793_005).wrapping_add(1_442_695_040_888_963_407);
        self.0 >> 33
    }
}

/// Names shaped like Itanium manglings: `_ZN`, nested length-prefixed
/// identifiers, `E`, a parameter encoding, and a serial so that all are
/// distinct. Three in four are short (about 25 to 70 bytes, a plain
/// member function); one in four is a template instantiation of 100 to
/// 400 bytes. The mean is near 100 bytes, as in a libc++-heavy object.
fn names() -> Vec<Vec<u8>> {
    const WORDS: [&str; 12] = [
        "std",
        "__1",
        "basic_string",
        "allocator",
        "char_traits",
        "vector",
        "detail",
        "impl",
        "unique_ptr",
        "__tree_node",
        "operator",
        "value_type",
    ];
    let mut rng = Lcg(0x5eed);
    let mut names = Vec::with_capacity(SYMBOLS);
    for serial in 0..SYMBOLS {
        let target = if rng.next().is_multiple_of(4) {
            100 + rng.next() % 300
        } else {
            25 + rng.next() % 45
        };
        let mut name = String::from("_ZN");
        while (name.len() as u64) < target {
            let word = WORDS[(rng.next() % 12) as usize];
            name.push_str(&word.len().to_string());
            name.push_str(word);
            if rng.next().is_multiple_of(5) {
                name.push_str("IcNS_");
            }
        }
        let serial = format!("f{serial}");
        name.push_str(&serial.len().to_string());
        name.push_str(&serial);
        name.push_str("EPKcm");
        names.push(name.into_bytes());
    }
    names
}

/// §2.2: 100,000 symbols are declared by name, described, and found
/// again by name; the time of each step is printed.
#[test]
fn declare_and_look_up_100k_symbols() {
    let names = names();
    let name_bytes: usize = names.iter().map(Vec::len).sum();
    let mut module = Module::new(TargetSpec::x86_64_linux_gnu());
    let fn_ty = module.types.function(TypeId::I32, &[TypeId::PTR, TypeId::I64], false).unwrap();

    // A producer meets most names first as a reference.
    let start = Instant::now();
    let mut ids: Vec<SymId> = Vec::with_capacity(SYMBOLS);
    for name in &names {
        ids.push(module.symbol_named(name).unwrap());
    }
    let declare = start.elapsed();

    // Then describes them: two in three functions (half of those with a
    // body), the rest variable declarations.
    let start = Instant::now();
    for (index, &id) in ids.iter().enumerate() {
        if index % 3 == 2 {
            let mut desc = VariableDesc::new(TypeId::I64);
            desc.align = Some(8);
            module.set_variable(id, &desc).unwrap();
        } else {
            let mut desc = FunctionDesc::new(fn_ty);
            desc.has_body = index % 2 == 0;
            if desc.has_body && index % 4 == 0 {
                desc.linkage = Linkage::LinkOnceOdr;
            }
            module.set_function(id, &desc).unwrap();
        }
    }
    let describe = start.elapsed();

    // Every name again, as the second and later references to it are.
    let start = Instant::now();
    let mut found = 0usize;
    for (name, &id) in names.iter().zip(&ids) {
        found += usize::from(module.find_symbol(name) == Some(id));
    }
    let lookup = start.elapsed();
    assert_eq!(found, SYMBOLS);

    // Names that are not there.
    let start = Instant::now();
    let mut missing = 0usize;
    let mut probe = Vec::new();
    for name in &names {
        probe.clear();
        probe.extend_from_slice(name);
        probe.push(b'_');
        missing += usize::from(module.find_symbol(&probe).is_none());
    }
    let miss = start.elapsed();
    assert_eq!(missing, SYMBOLS);

    // Reading every symbol back through its view.
    let start = Instant::now();
    let mut functions = 0usize;
    let mut definitions = 0usize;
    let mut read_bytes = 0usize;
    for &id in &ids {
        let symbol = module.symbol(id);
        functions += usize::from(symbol.is_function());
        definitions += usize::from(symbol.is_definition());
        read_bytes += symbol.name().len();
    }
    let read = start.elapsed();
    assert_eq!(module.symbol_count(), SYMBOLS);
    assert_eq!(functions, SYMBOLS - SYMBOLS / 3);
    assert_eq!(read_bytes, name_bytes);
    let with_body = (0..SYMBOLS).filter(|index| index % 3 != 2 && index % 2 == 0).count();
    assert_eq!(definitions, with_body);
    module.verify_symbols().unwrap();

    let per = |time: std::time::Duration| time.as_secs_f64() * 1e9 / SYMBOLS_F64;
    eprintln!(
        "symbol_perf ({}): {SYMBOLS} symbols, mean name {} bytes; per symbol: declare {:.0} ns, \
         describe {:.0} ns, look up {:.0} ns, look up a missing name {:.0} ns, read {:.0} ns",
        if cfg!(debug_assertions) { "unoptimized" } else { "optimized" },
        name_bytes / SYMBOLS,
        per(declare),
        per(describe),
        per(lookup),
        per(miss),
        per(read),
    );
}
