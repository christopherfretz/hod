//! WP-066's native table runner (included by its test files with
//! `#[path]`, not by `common/mod.rs`): functions of up to six `i64`
//! parameters returning `i64`, called from a C driver through a table,
//! compiled by hod in several modes and by `llc -O0 -global-isel=0
//! -fast-isel=0`, linked under every linker present and run. Every build
//! must print what the reference prints: the expected lines when the test
//! has an oracle of its own, else the `llc` build's.

use std::fmt::Write as _;

use hod_a64::compile_module;
use hod_cg::Options;

use crate::common::linking::Toolchain;
use crate::common::{ll_header, llc, llc_major, parse, write_object};

/// One call: the function's name and six arguments (AAPCS64 passes them
/// in `x0`..`x5` whatever the callee declares).
pub type Call = (String, [u64; 6]);

/// Builds `body` (`.ll` functions, without the header) in each of
/// `modes` and with `llc`, runs `calls` under every linker, and panics at
/// the first build whose output differs from `want` (one line per call)
/// or, when `want` is `None`, from the `llc` build's. Returns the number
/// of calls checked (0 without the tools).
pub fn run_table(
    label: &str,
    body: &str,
    calls: &[Call],
    modes: &[(&'static str, Options)],
    want: Option<&str>,
) -> usize {
    let mut c = String::from(
        "#include <stdio.h>\n#include <stdint.h>\n\
         typedef uint64_t (*fn6)(uint64_t, uint64_t, uint64_t, uint64_t, uint64_t, uint64_t);\n",
    );
    let mut declared: Vec<&str> = Vec::new();
    for (name, _) in calls {
        if !declared.contains(&name.as_str()) {
            writeln!(c, "uint64_t {name}(void);").unwrap();
            declared.push(name);
        }
    }
    c.push_str("static const struct { fn6 f; uint64_t a[6]; } calls[] = {\n");
    for (name, args) in calls {
        let list: Vec<String> = args.iter().map(|a| format!("{a}ULL")).collect();
        writeln!(c, "  {{ (fn6){name}, {{ {} }} }},", list.join(", ")).unwrap();
    }
    c.push_str(
        "};\nint main(void) {\n  for (unsigned i = 0; i < sizeof calls / sizeof calls[0]; i++) {\n    \
         const uint64_t *a = calls[i].a;\n    \
         printf(\"%llu\\n\", (unsigned long long)calls[i].f(a[0], a[1], a[2], a[3], a[4], a[5]));\n  \
         }\n  return 0;\n}\n",
    );
    let dir = hod_testutil::Scratch::new(label);
    let Some(toolchain) = Toolchain::host(&dir) else { return 0 };
    let driver = toolchain.compile_c(&dir, "driver", &c, &[]);
    let (module, bodies) = parse(&format!("{}{body}", ll_header(22)));
    let mut objects = Vec::new();
    let Some(llc_object) = llc(&dir, "llc", &format!("{}{body}", ll_header(llc_major())), &[])
    else {
        return 0;
    };
    objects.push(("llc", llc_object));
    for (name, options) in modes {
        let emitted = compile_module(&module, &bodies, options)
            .unwrap_or_else(|e| panic!("{label} {name}: {e}"));
        objects.push((name, write_object(&dir, &format!("{name}.o"), &emitted)));
    }
    let mut reference = want.map(str::to_owned);
    for linker in &toolchain.linkers {
        for (name, object) in &objects {
            let exe = dir.join(format!("{name}-{}", linker.name));
            let ran = toolchain.link_and_run(linker, &[], &[&driver, object], &exe);
            assert!(ran.success(), "{label} {name} under {}: {}", linker.name, ran.stderr_text());
            let got = ran.stdout_text();
            let reference = reference.get_or_insert_with(|| got.clone());
            if &got != reference {
                let at = got.lines().zip(reference.lines()).position(|(g, w)| g != w);
                let at = at.unwrap_or(0);
                let (function, args) = &calls[at.min(calls.len() - 1)];
                let g = got.lines().nth(at).unwrap_or_default();
                let w = reference.lines().nth(at).unwrap_or_default();
                let text = function_text(body, function);
                panic!(
                    "{label}: {name} under {}: call {at} @{function}{args:?} gives {g}, the reference {w}\n{text}",
                    linker.name
                );
            }
        }
    }
    calls.len()
}

/// The definition of `@name` in `body`, for a report.
pub fn function_text<'b>(body: &'b str, name: &str) -> &'b str {
    let head = format!("define i64 @{name}(");
    let Some(start) = body.find(&head) else { return "" };
    let end = body[start..].find("\n}\n").map_or(body.len(), |e| start + e + 3);
    &body[start..end]
}
