//! `spec/ir.md` §9: the printer against LLVM on the bodies of the random
//! generator (`crate::random`).

use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;
use std::path::Path;
use std::process::Command;
use std::time::Instant;

use hod_testutil::{DEFAULT_TIMEOUT, LLVM_MAJORS, run_with, tool};

use super::{Dialect, PrintError};
use crate::attrs::ParamAttrs;
use crate::body::{Body, InstKind};
use crate::constant::Const;
use crate::ids::{BlockId, SymId, TypeId};
use crate::inst::{Opcode, TailKind};
use crate::intrinsic::Intrinsic;
use crate::module::Module;
use crate::random::generate;
use crate::symbol::{FunctionDesc, VariableDesc};
use crate::value::{Value, ValueKind};

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
/// definition once each, then the definitions.
#[derive(Default)]
struct Batch {
    shared: BTreeSet<String>,
    defines: String,
    bodies: usize,
}

impl Batch {
    fn add(&mut self, text: &str) {
        let mut in_define = false;
        for line in text.lines() {
            in_define |= line.starts_with("define ");
            if in_define {
                self.defines.push_str(line);
                self.defines.push('\n');
                in_define = line != "}";
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
        text
    }
}

/// Why LLVM's verifier refuses a body that `Body::verify` accepts, if it
/// does: the rules LLVM has and `hod_ir` does not (the list is §9.8's).
/// `None` is a prediction that LLVM accepts the printed body.
fn llvm_objection(m: &Module, body: &Body) -> Option<&'static str> {
    for call in body.calls() {
        let asm = matches!(call.callee_value().kind(), ValueKind::Asm(_));
        if call.opcode() == Opcode::CallBr && !asm {
            // LLVM 22's verifier crashes on a `callbr` of an unknown
            // intrinsic instead of refusing it.
            return Some("callbr of something that is not inline assembly");
        }
        if call.opcode() == Opcode::Invoke && call.intrinsic().is_some() {
            return Some("invoke of an intrinsic");
        }
        // LLVM 19 takes `undef` for an immediate argument; LLVM 22 does not.
        let immediate = |flag: &Value| {
            flag.as_const().is_some_and(|id| {
                !matches!(m.consts.get(id), Const::Undef(_) | Const::Poison(_))
            })
        };
        if call.intrinsic() == Some(Intrinsic::Memcpy)
            && call.args().get(3).is_some_and(|flag| !immediate(flag))
        {
            return Some("an immediate argument that is not a constant");
        }
        if call.bundles().any(|bundle| bundle.tag == "kcfi" && bundle.inputs.len() != 1) {
            return Some("a kcfi bundle without its one operand");
        }
        if call.tail() == TailKind::MustTail {
            // LLVM wants the caller's signature and convention, and a
            // `ret` of the result next.
            return Some("musttail");
        }
        let signext = |attrs: ParamAttrs, ty: TypeId| {
            attrs.has(ParamAttrs::SIGNEXT) && m.types.int_bits(ty).is_none()
        };
        let args = call.args().iter().enumerate();
        if signext(call.ret_attrs(), call.ty())
            || args.into_iter().any(|(at, &arg)| signext(call.arg_attrs(at), body.value_type(m, arg)))
        {
            return Some("signext on a value that is not an integer");
        }
    }
    dominance_objection(body)
}

/// LLVM's rule that a definition dominates its uses, which `Body::verify`
/// does not have. A use of the result of an `invoke` or `callbr` counts
/// as a violation: where LLVM allows one depends on the edge.
fn dominance_objection(body: &Body) -> Option<&'static str> {
    let count = body.block_count();
    assert!(count <= 64);
    let successors = |block: usize| {
        body.block(BlockId::new(block as u32)).successors().map(BlockId::index)
    };
    // Which blocks control reaches, and each one's dominators as a set.
    let entry = body.entry().index();
    let mut reachable = 1u64 << entry;
    let mut work = vec![entry];
    while let Some(block) = work.pop() {
        for next in successors(block) {
            if reachable & (1 << next) == 0 {
                reachable |= 1 << next;
                work.push(next);
            }
        }
    }
    let mut dominators = vec![reachable; count];
    dominators[entry] = 1 << entry;
    let mut changed = true;
    while changed {
        changed = false;
        for block in (0..count).filter(|&block| reachable & (1 << block) != 0) {
            let mut preds = u64::MAX;
            for pred in (0..count).filter(|&pred| reachable & (1 << pred) != 0) {
                if successors(pred).any(|next| next == block) {
                    preds &= dominators[pred];
                }
            }
            let new = if block == entry { 1 << entry } else { preds | (1 << block) };
            changed |= new != dominators[block];
            dominators[block] = new;
        }
    }
    // Where each instruction is.
    let mut home = vec![(0usize, 0usize); body.inst_count()];
    for block in body.blocks() {
        for (position, inst) in block.insts().enumerate() {
            home[inst.id().index()] = (block.id().index(), position);
        }
    }
    let is_reachable = |block: usize| reachable & (1 << block) != 0;
    for block in body.blocks() {
        let here = block.id().index();
        for (position, inst) in block.insts().enumerate() {
            let phi = match inst.kind() {
                InstKind::Phi(phi) => Some(phi),
                _ => None,
            };
            for (at, &operand) in inst.operands().iter().enumerate() {
                let ValueKind::Inst(def) = operand.kind() else { continue };
                let (def_block, def_position) = home[def.index()];
                let edge_result =
                    matches!(body.inst(def).opcode(), Opcode::Invoke | Opcode::CallBr);
                // A `phi` uses its value at the end of the incoming block.
                let (use_block, in_block_ok) = match phi {
                    Some(phi) => (phi.blocks().nth(at).unwrap().index(), !edge_result),
                    None => (here, def_position < position),
                };
                // LLVM asks nothing of a use in a block control cannot reach.
                if !is_reachable(use_block) {
                    continue;
                }
                if def == inst.id() && phi.is_none() {
                    return Some("an instruction that uses its own result");
                }
                let dominated = if def_block == use_block {
                    in_block_ok
                } else {
                    !edge_result && dominators[use_block] & (1 << def_block) != 0
                };
                if !dominated {
                    return Some("a use its definition does not dominate");
                }
            }
        }
    }
    None
}

/// One dialect's printed bodies, sorted by what LLVM's verifier is
/// predicted to say of them.
#[derive(Default)]
struct Sorted {
    /// Bodies LLVM accepts.
    clean: Batch,
    /// Bodies LLVM's parser accepts and its verifier refuses, with how
    /// many of each objection.
    refused: Batch,
    objections: BTreeMap<&'static str, usize>,
    /// The constructs the dialect cannot express, with how many of each.
    unsupported: BTreeMap<String, usize>,
    /// The opcodes of the clean bodies.
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
        match llvm_objection(m, body) {
            None => {
                self.clean.add(&text);
                self.opcodes.extend(body.insts().map(|inst| inst.opcode()));
            }
            Some(objection) => {
                self.refused.add(&text);
                *self.objections.entry(objection).or_default() += 1;
            }
        }
    }
}

/// Whether `construct` is one a random body may hold that `dialect`
/// cannot express (§9.6).
fn expected_unsupported(dialect: Dialect, construct: &str) -> bool {
    match construct {
        // `Body::verify` allows a tail marker on any call; LLVM has them
        // on `call` alone.
        "invoke:tail" | "invoke:musttail" | "invoke:notail" | "callbr:tail" | "callbr:musttail"
        | "callbr:notail" => true,
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
}

/// `spec/ir.md` §9.9: LLVM 19 and LLVM 22 take every printed random body.
/// `llvm-as` and `opt -passes=verify` accept the ones that keep the rules
/// LLVM has and `Body::verify` lacks; `llvm-as -disable-verify` parses the
/// others; and a body that is not printed holds a construct its dialect
/// lacks.
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
        assert!(all.clean.bodies >= 300, "LLVM {major}: {} clean bodies", all.clean.bodies);
        // Every opcode but the funclet ones (not printed, §9.6) and
        // `callbr`, which LLVM has for inline assembly alone and the
        // generator makes with other callees.
        for &opcode in Opcode::ALL {
            let wanted = !opcode.is_funclet() && opcode != Opcode::CallBr;
            assert_eq!(all.opcodes.contains(&opcode), wanted, "LLVM {major}: {}", opcode.name());
        }
        eprintln!(
            "LLVM {major}: {} bytes of text: {} bodies verified, {} parsed {:?}, \
             {} not expressible {:?}",
            all.bytes,
            all.clean.bodies,
            all.refused.bodies,
            all.objections,
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
        let refused = all.refused.text();
        assert_eq!(refused.matches("\ndefine ").count(), all.refused.bodies);
        accept(&llvm_as, &["-disable-verify", "-o", "/dev/null", "-"], &refused, "a parsed body");
        eprintln!(
            "LLVM {major}: llvm-as and opt on {} bytes, llvm-as -disable-verify on {} bytes, \
             in {:.2?}",
            clean.len(),
            refused.len(),
            start.elapsed()
        );
    }
}
