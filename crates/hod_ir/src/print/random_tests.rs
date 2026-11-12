//! `spec/ir.md` §9: the printer against LLVM on the bodies of the random
//! generator (`crate::random`).

use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;
use std::path::Path;
use std::process::Command;
use std::time::Instant;

use hod_testutil::{DEFAULT_TIMEOUT, LLVM_MAJORS, run_with, tool};

use super::{Dialect, PrintError};
use crate::body::{Body, Inst};
use crate::ids::{SymId, TypeId};
use crate::inst::Opcode;
use crate::module::Module;
use crate::random::generate;
use crate::symbol::{FunctionDesc, VariableDesc};

/// The first seed: the generator's bodies name the symbols 0 to 99, and a
/// body's own symbol has its seed as id.
const FIRST_SEED: u64 = 100;

/// The body of `seed` in a module that has every symbol the body names:
/// `@g<id>` variables, the function `@f<seed>`, and a personality.
fn generated(seed: u64) -> (Module, Body) {
    let (mut m, body) = generate(seed, None, true);
    let body = body.unwrap();
    let function = body.symbol();
    assert_eq!(u64::from(function.raw()), seed);
    for id in 0..function.raw() {
        let made = m.add_variable(format!("g{id}").as_bytes(), &VariableDesc::new(TypeId::I8));
        assert_eq!(made.unwrap(), SymId::new(id));
    }
    let made = m.symbol_named(format!("f{seed}").as_bytes()).unwrap();
    assert_eq!(made, function);
    let personality_ty = m.types.function(TypeId::I32, &[], true).unwrap();
    let personality =
        m.add_function(b"__gxx_personality_v0", &FunctionDesc::new(personality_ty)).unwrap();
    let desc = FunctionDesc {
        personality: body.has_landing_pads().then_some(personality),
        has_body: true,
        ..FunctionDesc::new(body.fn_type())
    };
    m.set_function(function, &desc).unwrap();
    m.verify_symbols().unwrap();
    body.verify(&m).unwrap();
    (m, body)
}

/// Many printed modules as one: the lines that are not part of a
/// definition once each, then the definitions. Each module's numbered
/// metadata is renumbered past the ones before it; a named metadata list
/// gathers every module's entries, except the module flags, which are the
/// first module's that has any (every generated module has the same).
#[derive(Default)]
struct Batch {
    shared: BTreeSet<String>,
    named: BTreeMap<String, Vec<String>>,
    nodes: String,
    defines: String,
    bodies: usize,
    next_node: u64,
}

impl Batch {
    fn add(&mut self, text: &str) {
        let offset = self.next_node;
        let text = renumber(text, offset, &mut self.next_node);
        let mut in_define = false;
        for line in text.lines() {
            in_define |= line.starts_with("define ");
            if in_define {
                self.defines.push_str(line);
                self.defines.push('\n');
                in_define = line != "}";
            } else if let Some((name, list)) = named_metadata(line) {
                let entries = self.named.entry(name.to_string()).or_default();
                if name != "llvm.module.flags" || entries.is_empty() {
                    entries.extend(list.split(", ").filter(|e| !e.is_empty()).map(str::to_string));
                }
            } else if line.starts_with('!') {
                self.nodes.push_str(line);
                self.nodes.push('\n');
            } else if !line.is_empty() {
                self.shared.insert(line.to_string());
            }
        }
        self.bodies += 1;
    }

    fn text(&self) -> String {
        let mut text = String::new();
        // LLVM wants the `target` lines first.
        let (target, rest): (Vec<&String>, Vec<&String>) =
            self.shared.iter().partition(|line| line.starts_with("target "));
        for line in target.into_iter().chain(rest) {
            writeln!(text, "{line}").unwrap();
        }
        text.push_str(&self.defines);
        for (name, entries) in &self.named {
            writeln!(text, "!{name} = !{{{}}}", entries.join(", ")).unwrap();
        }
        text.push_str(&self.nodes);
        text
    }
}

/// `!name = !{list}`, as `(name, list)`.
fn named_metadata(line: &str) -> Option<(&str, &str)> {
    let rest = line.strip_prefix('!')?;
    let (name, list) = rest.split_once(" = !{")?;
    let first = name.chars().next()?;
    (!first.is_ascii_digit()).then_some(())?;
    Some((name, list.strip_suffix('}')?))
}

/// `text` with every metadata number `!n` made `!(n + offset)`, raising
/// `next` past the largest.
fn renumber(text: &str, offset: u64, next: &mut u64) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(at) = rest.find('!') {
        out.push_str(&rest[..=at]);
        rest = &rest[at + 1..];
        let digits = rest.bytes().take_while(u8::is_ascii_digit).count();
        if digits > 0 {
            let n: u64 = rest[..digits].parse().unwrap();
            write!(out, "{}", n + offset).unwrap();
            *next = (*next).max(n + offset + 1);
            rest = &rest[digits..];
        }
    }
    out.push_str(rest);
    out
}

/// One dialect's printed bodies: every body `Body::verify` accepts is
/// printed or holds a construct the dialect cannot express.
#[derive(Default)]
struct Sorted {
    /// The printed bodies.
    clean: Batch,
    /// The constructs the dialect cannot express, with how many of each.
    unsupported: BTreeMap<String, usize>,
    /// The opcodes of the printed bodies.
    opcodes: BTreeSet<Opcode>,
    /// The bytes of text printed.
    bytes: usize,
}

impl Sorted {
    /// Prints the body of `seed` for `dialect` and sorts it.
    fn add(&mut self, dialect: Dialect, seed: u64, m: &Module, body: &Body) {
        let mut text = String::new();
        match m.print(&mut text, dialect, [body]) {
            Ok(()) => {}
            Err(PrintError::Unsupported { construct, symbol }) => {
                assert_eq!(symbol, Some(body.symbol()), "seed {seed}: {construct}");
                *self.unsupported.entry(construct).or_default() += 1;
                return;
            }
            Err(error) => panic!("seed {seed}: {error}"),
        }
        // The body alone is its definition in the module's text, and
        // printing it again gives the same text.
        let mut alone = String::new();
        m.print_body(&mut alone, dialect, body).unwrap();
        assert!(text.contains(alone.as_str()), "seed {seed}:\n{alone}\n{text}");
        let mut again = String::new();
        m.print_body(&mut again, dialect, body).unwrap();
        assert_eq!(alone, again, "seed {seed}");
        self.bytes += text.len();
        self.clean.add(&text);
        self.opcodes.extend(body.insts().map(Inst::opcode));
    }
}

/// Whether `construct` is one a random body may hold that `dialect`
/// cannot express (§9.6).
fn expected_unsupported(dialect: Dialect, construct: &str) -> bool {
    match construct {
        "atomicrmw:usub_cond" | "atomicrmw:usub_sat" => dialect.major() < 20,
        "atomicrmw:fmaximum" | "atomicrmw:fminimum" => dialect.major() < 21,
        _ => false,
    }
}

/// Runs an LLVM tool on `text` and requires it to succeed.
fn accept(tool: &Path, args: &[&str], text: &str, what: &str) {
    let outcome = run_with(Command::new(tool).args(args), text.as_bytes(), DEFAULT_TIMEOUT * 5);
    let stderr = outcome.stderr_text();
    let shown: String = stderr.chars().take(4000).collect();
    assert!(outcome.success(), "{} refused {what}:\n{shown}", tool.display());
    // A module whose debug tables LLVM drops instead of checking.
    assert!(!stderr.contains("ignoring debug info"), "{}: {shown}", tool.display());
}

/// `spec/ir.md` §9.9: LLVM 19 and LLVM 22 take every random body
/// `Body::verify` accepts: `llvm-as` and `opt -passes=verify` accept each
/// printed one, and a body that is not printed holds a construct its
/// dialect lacks.
#[test]
fn random_bodies_are_accepted_by_llvm() {
    const SEEDS: u64 = 1500;
    let start = Instant::now();
    let mut sorted = LLVM_MAJORS.map(|major| (Dialect::new(major).unwrap(), Sorted::default()));
    for seed in FIRST_SEED..FIRST_SEED + SEEDS {
        let (m, body) = generated(seed);
        for (dialect, all) in &mut sorted {
            all.add(*dialect, seed, &m, &body);
        }
    }
    eprintln!("{SEEDS} bodies generated, printed and sorted in {:.2?}", start.elapsed());
    for (dialect, all) in &sorted {
        let major = dialect.major();
        for construct in all.unsupported.keys() {
            assert!(expected_unsupported(*dialect, construct), "LLVM {major}: {construct}");
        }
        assert!(all.clean.bodies >= 1000, "LLVM {major}: {} clean bodies", all.clean.bodies);
        // Every opcode but the funclet ones (not printed, §9.5).
        for &opcode in Opcode::ALL {
            let wanted = !opcode.is_funclet();
            assert_eq!(all.opcodes.contains(&opcode), wanted, "LLVM {major}: {}", opcode.name());
        }
        eprintln!(
            "LLVM {major}: {} bytes of text: {} bodies verified, {} not expressible {:?}",
            all.bytes,
            all.clean.bodies,
            all.unsupported.values().sum::<usize>(),
            all.unsupported,
        );
        let (Some(llvm_as), Some(opt)) =
            (tool(&format!("llvm-as-{major}")), tool(&format!("opt-{major}")))
        else {
            continue;
        };
        let start = Instant::now();
        let clean = all.clean.text();
        assert_eq!(clean.matches("\ndefine ").count(), all.clean.bodies);
        accept(&llvm_as, &["-o", "/dev/null", "-"], &clean, "a verified body");
        accept(&opt, &["-passes=verify", "-disable-output", "-"], &clean, "a verified body");
        eprintln!(
            "LLVM {major}: llvm-as and opt on {} bytes in {:.2?}",
            clean.len(),
            start.elapsed()
        );
    }
}
