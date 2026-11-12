//! What the zlib compressor costs and gives, against zlib at level 1 and
//! LLVM's `-gz=zlib` (`spec/object.md` "Compressed sections", WP-061).
//! Ignored: a measurement, never a gate.
//!
//! Two C++ translation units with the standard library are compiled by
//! clang 22 with `-g -O0`, once plain and once with `-gz=zlib`; each
//! `.debug_*` section of the plain object is compressed by hod (best of
//! five, one reused [`Deflater`]) and by Python's `zlib.compress(level=1)`
//! (best of five), and its size in the `-gz=zlib` object is LLVM's. It
//! prints, per section and in total: the input size, each compressor's
//! output size and ratio, and the MB/s of hod and of zlib -1.
//!
//! ```text
//! cargo test --release -p hod_obj --test deflate_throughput -- --ignored --nocapture
//! ```

use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, Instant};

use hod_obj::deflate::Deflater;
use hod_testutil::{Scratch, run_with, tool};

const BIG: &str = r#"
#include <algorithm>
#include <functional>
#include <iostream>
#include <map>
#include <memory>
#include <optional>
#include <regex>
#include <set>
#include <sstream>
#include <string>
#include <unordered_map>
#include <variant>
#include <vector>

struct Node { std::string name; std::vector<std::shared_ptr<Node>> kids; std::map<std::string, int> attrs; };

template <class K, class V> V sum_values(const std::unordered_map<K, V>& m) {
    V s{};
    for (auto& [k, v] : m) s += v;
    return s;
}

int main(int argc, char** argv) {
    std::regex re("([a-z]+)=([0-9]+)");
    std::unordered_map<std::string, long> um;
    std::map<int, std::set<std::string>> by;
    std::vector<std::variant<int, double, std::string>> vs;
    std::ostringstream os;
    for (int i = 0; i < argc; ++i) {
        std::smatch m;
        std::string a = argv[i];
        if (std::regex_match(a, m, re)) um[m[1]] += std::stol(m[2]);
        by[i].insert(a);
        vs.emplace_back(a);
        os << a;
    }
    auto n = std::make_shared<Node>();
    n->attrs["x"] = 1;
    std::function<int(int)> f = [&](int x) { return x + static_cast<int>(um.size()); };
    std::optional<int> o = f(3);
    std::sort(vs.begin(), vs.end(), [](auto& a, auto& b) { return a.index() < b.index(); });
    std::cout << os.str() << sum_values(um) << *o << by.size() << std::endl;
    return 0;
}
"#;

fn compile(clang: &Path, scratch: &Scratch, source: &Path, name: &str, gz: bool) -> PathBuf {
    let out = scratch.join(name);
    let mut command = Command::new(clang);
    command.args(["-g", "-O0", "-std=c++20", "-w", "-c"]).arg(source).arg("-o").arg(&out);
    if gz {
        command.arg("-gz=zlib");
    }
    let ran = run_with(&mut command, &[], Duration::from_mins(5));
    assert!(ran.success(), "{command:?}: {}", ran.stderr_text());
    out
}

/// `(name, sh_size)` of the `.debug_*` sections, from `llvm-readelf -S`.
fn debug_sections(readelf: &Path, object: &Path) -> Vec<(String, u64)> {
    let mut command = Command::new(readelf);
    command.args(["-S", "-W"]).arg(object);
    let out = run_with(&mut command, &[], Duration::from_mins(1)).stdout_text();
    out.lines()
        .filter_map(|line| {
            let line = line.split_once(']')?.1;
            let fields: Vec<&str> = line.split_whitespace().collect();
            let name = *fields.first()?;
            if !name.starts_with(".debug_") {
                return None;
            }
            Some((name.to_owned(), u64::from_str_radix(fields.get(4)?, 16).ok()?))
        })
        .collect()
}

const ZLIB1: &str = r"
import sys, time, zlib
for path in sys.argv[1:]:
    data = open(path, 'rb').read()
    best = None
    for _ in range(5):
        t = time.perf_counter()
        out = zlib.compress(data, 1)
        t = time.perf_counter() - t
        best = t if best is None else min(best, t)
    print(len(out), best)
";

#[test]
#[allow(clippy::cast_precision_loss, reason = "sizes of a few megabytes, printed")]
#[ignore = "a measurement: run with --release --ignored --nocapture"]
fn compressor_against_zlib_and_llvm() {
    let (Some(clang), Some(readelf), Some(objcopy), Some(python)) =
        (tool("clang++-22"), tool("llvm-readelf-22"), tool("llvm-objcopy-22"), tool("python3"))
    else {
        return;
    };
    let scratch = Scratch::new("deflate-throughput");
    let stl =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../hod_dwarf/tests/data/types/stl.cpp");
    let big = scratch.write("big.cpp", BIG);
    let mut rows = Vec::new();
    for (tag, source) in [("stl", stl), ("big", big)] {
        let plain = compile(&clang, &scratch, &source, &format!("{tag}.o"), false);
        let gz = compile(&clang, &scratch, &source, &format!("{tag}-gz.o"), true);
        let llvm: Vec<(String, u64)> = debug_sections(&readelf, &gz);
        for (name, _) in debug_sections(&readelf, &plain) {
            let file = scratch.join(format!("{tag}{name}"));
            let mut command = Command::new(&objcopy);
            command.arg(format!("--dump-section={name}={}", file.display()));
            command.arg(&plain).arg(scratch.join("discard.o"));
            assert!(run_with(&mut command, &[], Duration::from_mins(1)).success());
            let llvm_size = llvm.iter().find(|(n, _)| *n == name).map_or(0, |s| s.1);
            rows.push((format!("{tag} {name}"), file, llvm_size));
        }
    }
    let script = scratch.write("zlib1.py", ZLIB1);
    let mut command = Command::new(&python);
    command.arg("-I").arg(&script).args(rows.iter().map(|r| &r.1));
    let ran = run_with(&mut command, &[], Duration::from_mins(5));
    assert!(ran.success(), "{}", ran.stderr_text());
    let zlib1: Vec<(u64, f64)> = ran
        .stdout_text()
        .lines()
        .map(|l| {
            let (a, b) = l.split_once(' ').unwrap();
            (a.parse().unwrap(), b.parse().unwrap())
        })
        .collect();

    let mut deflater = Deflater::new();
    let mut out = Vec::new();
    let mut totals = [0f64; 6];
    eprintln!(
        "{:<26} {:>8} | {:>8} {:>6} {:>7} | {:>8} {:>6} {:>7} | {:>8} {:>6}",
        "section", "bytes", "hod", "ratio", "MB/s", "zlib -1", "ratio", "MB/s", "LLVM", "ratio"
    );
    for ((label, file, llvm), (z_size, z_time)) in rows.iter().zip(&zlib1) {
        let data = std::fs::read(file).unwrap();
        let mut best = f64::MAX;
        for _ in 0..5 {
            out.clear();
            let t = Instant::now();
            deflater.zlib(&data, &mut out).unwrap();
            best = best.min(t.elapsed().as_secs_f64());
        }
        let n = data.len() as f64;
        // LLVM's size includes the 24-byte header; or it is the plain
        // size when LLVM left the section uncompressed.
        let llvm = if *llvm == data.len() as u64 { n } else { (*llvm as f64) - 24.0 };
        eprintln!(
            "{label:<26} {n:>8} | {:>8} {:>6.2} {:>7.0} | {z_size:>8} {:>6.2} {:>7.0} | {llvm:>8} {:>6.2}",
            out.len(),
            n / out.len() as f64,
            n / best / 1e6,
            n / *z_size as f64,
            n / z_time / 1e6,
            n / llvm,
        );
        for (t, v) in
            totals.iter_mut().zip([n, out.len() as f64, best, *z_size as f64, *z_time, llvm])
        {
            *t += v;
        }
    }
    let [n, h, ht, z, zt, l] = totals;
    eprintln!(
        "{:<26} {n:>8} | {h:>8} {:>6.2} {:>7.0} | {z:>8} {:>6.2} {:>7.0} | {l:>8} {:>6.2}",
        "total",
        n / h,
        n / ht / 1e6,
        n / z,
        n / zt / 1e6,
        n / l
    );
}
