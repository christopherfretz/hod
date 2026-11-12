//! A throughput sanity check for the object writer, not a gate
//! (`spec/object.md` "Performance").
//!
//! ```text
//! cargo run --release -p hod_obj --example throughput [functions [relocations-per-function]]
//! ```
//!
//! Builds an object shaped like a large C++ translation unit, once with a
//! section per function and once with one `.text`, and reports the time to
//! build the model and to write it, the file size, and resident memory.

use std::io::Write;
use std::time::Instant;

use hod_obj::{A64Reloc, Arch, Object, SectionKind, Symbol, SymbolKind, elf};

/// A sink that counts.
struct Count(u64);

impl Write for Count {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.0 += buf.len() as u64;
        Ok(buf.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

/// `VmRSS` and `VmHWM` of this process in KiB; zeros where `/proc` has no
/// such file.
fn memory() -> (u64, u64) {
    let status = std::fs::read_to_string("/proc/self/status").unwrap_or_default();
    let field = |name: &str| {
        status
            .lines()
            .find_map(|line| line.strip_prefix(name))
            .and_then(|rest| rest.split_whitespace().next())
            .and_then(|value| value.parse().ok())
            .unwrap_or(0)
    };
    (field("VmRSS:"), field("VmHWM:"))
}

/// Bytes as mebibytes, for display.
#[allow(clippy::cast_precision_loss, reason = "a figure printed with two decimals")]
fn mib(bytes: u64) -> f64 {
    bytes as f64 / f64::from(1 << 20)
}

fn build(
    functions: u32,
    relocations: u32,
    function_sections: bool,
) -> Result<Object, hod_obj::Error> {
    const CODE: [u8; 64] = [0x1f; 64];
    let mut obj = Object::new(Arch::AArch64);
    let sections = if function_sections { functions as usize } else { 1 };
    obj.reserve(sections, functions as usize + 1, (functions * relocations) as usize, 0);
    let shared = obj.add_section(b".text", SectionKind::Text, 4)?;
    let callee = obj.add_string(b"_ZN6callee4callEv")?;
    let callee = obj.add_symbol(Symbol::undefined(callee));
    let mut name = Vec::new();
    for index in 0..functions {
        name.clear();
        write!(name, ".text._ZN9namespace5Class{index}6methodEiPKc").unwrap_or_default();
        let section =
            if function_sections { obj.add_section(&name, SectionKind::Text, 4)? } else { shared };
        let at = obj.append(section, &CODE);
        let symbol = obj.add_string(&name[6..])?;
        obj.add_symbol(Symbol::defined(symbol, SymbolKind::Func, section, at, 64));
        for slot in 0..relocations {
            let offset = at + u64::from(slot) * 4;
            obj.add_relocation(section, offset, A64Reloc::Call26, callee, 0);
        }
    }
    Ok(obj)
}

fn measure(
    functions: u32,
    relocations: u32,
    function_sections: bool,
) -> Result<(), hod_obj::Error> {
    let (rss_before, _) = memory();
    let start = Instant::now();
    let obj = build(functions, relocations, function_sections)?;
    let built = start.elapsed();
    let (rss_model, _) = memory();

    // Fastest of five, into a sink that discards: the writer alone.
    let mut write = std::time::Duration::MAX;
    let mut size = 0;
    for _ in 0..5 {
        let start = Instant::now();
        let mut sink = Count(0);
        if let Err(error) = elf::write(&obj, &mut sink) {
            eprintln!("write failed: {error}");
            return Ok(());
        }
        write = write.min(start.elapsed());
        size = sink.0;
    }
    let (_, peak_stream) = memory();

    let start = Instant::now();
    let bytes = elf::to_vec(&obj)?;
    let to_vec = start.elapsed();
    let (_, peak_vec) = memory();

    println!(
        "{} functions, {} relocations, {} sections:",
        functions,
        functions * relocations,
        obj.section_count()
    );
    let ms = |time: std::time::Duration| time.as_secs_f64() * 1e3;
    println!("  build model      {:>8.2} ms", ms(built));
    println!(
        "  elf::write       {:>8.2} ms  ({:.0} MiB/s)",
        ms(write),
        mib(size) / write.as_secs_f64()
    );
    println!("  elf::to_vec      {:>8.2} ms", ms(to_vec));
    println!("  file size        {:>8.2} MiB", mib(size));
    println!("  model RSS        {:>8.2} MiB", mib(rss_model.saturating_sub(rss_before) * 1024));
    println!("  peak RSS, write  {:>8.2} MiB (process: model included)", mib(peak_stream * 1024));
    println!(
        "  peak RSS, to_vec {:>8.2} MiB (process: model and file included)",
        mib(peak_vec * 1024)
    );
    assert_eq!(bytes.len() as u64, size);
    Ok(())
}

fn main() -> Result<(), hod_obj::Error> {
    let mut args = std::env::args().skip(1).map(|arg| arg.parse::<u32>().ok());
    let functions = args.next().flatten().unwrap_or(100_000);
    let relocations = args.next().flatten().unwrap_or(5);
    let mode = std::env::var("HOD_OBJ_MODE").unwrap_or_default();
    // One mode per process, so that the peak belongs to it.
    match mode.as_str() {
        "one-text" => measure(functions, relocations, false),
        _ => measure(functions, relocations, true),
    }
}
