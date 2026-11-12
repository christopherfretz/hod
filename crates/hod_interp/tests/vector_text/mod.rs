//! What the vector tests share (`spec/interp.md` §5.10): programs written
//! as LLVM text, read by `hod_ll`, and run in the interpreter.

// Each test file uses its own part of this.
#![allow(dead_code)]

use hod_interp::{Config, Interp, Outcome, Program, Stop, Trap, Val, Vector};
use hod_ir::{Body, Module, SymId};
use hod_ll::{BodyParser, ParseOptions, parse_module};

/// A module read from text, with the bodies of all its definitions.
pub struct Text {
    pub module: Module,
    pub bodies: Vec<Body>,
}

/// The module of `source`, which must be LLVM text `hod_ll` reads in full.
#[track_caller]
pub fn parse(source: &str) -> Text {
    let mut parsed = parse_module(source.as_bytes(), &ParseOptions::default())
        .unwrap_or_else(|error| panic!("{error}\n{source}"));
    let mut parser = BodyParser::new(&parsed.text);
    let mut bodies = Vec::new();
    for function in parsed.text.functions() {
        let body = parser
            .parse(&mut parsed.module, function)
            .unwrap_or_else(|error| panic!("{error}"))
            .unwrap_or_else(|| {
                let reasons: Vec<&str> = parser.unsupported().collect();
                panic!("hod_ll refuses: {reasons:?}")
            });
        body.verify(&parsed.module).unwrap_or_else(|error| panic!("{error}"));
        bodies.push(body);
    }
    Text { module: parsed.module, bodies }
}

/// The text of a module for the host's target: `body` after the triple.
pub fn module(body: &str) -> String {
    format!("target triple = \"aarch64-unknown-linux-gnu\"\n\n{body}")
}

impl Text {
    pub fn sym(&self, name: &str) -> SymId {
        self.module.find_symbol(name.as_bytes()).unwrap_or_else(|| panic!("no symbol {name}"))
    }

    pub fn program(&self) -> Program<'_> {
        Program::with_bodies(&self.module, &self.bodies).expect("bodies of the module")
    }

    /// Runs `name` on `args`.
    pub fn run(&self, name: &str, args: &[Val]) -> Outcome {
        let program = self.program();
        Interp::new(&program).run(self.sym(name), args)
    }

    pub fn run_with(&self, name: &str, args: &[Val], config: Config) -> Outcome {
        let program = self.program();
        Interp::new(&program).with_config(config).run(self.sym(name), args)
    }

    /// The vector `name` returns for `args`.
    #[track_caller]
    pub fn vector(&self, name: &str, args: &[Val]) -> Vector {
        match self.run(name, args).result {
            Ok(Some(Val::Agg(vector))) if vector.is_vector() => vector,
            other => panic!("{name}: {other:?}"),
        }
    }

    /// The scalar `name` returns for `args`.
    #[track_caller]
    pub fn scalar(&self, name: &str, args: &[Val]) -> Val {
        match self.run(name, args).result {
            Ok(Some(val)) => val,
            other => panic!("{name}: {other:?}"),
        }
    }

    /// The trap `name` stops with.
    #[track_caller]
    pub fn trap(&self, name: &str, args: &[Val]) -> Trap {
        match self.run(name, args).result {
            Err(Stop::Trap(trap)) => trap,
            other => panic!("{name}: expected a trap, got {other:?}"),
        }
    }

    /// The unsupported reason `name` stops with.
    #[track_caller]
    pub fn unsupported(&self, name: &str, args: &[Val]) -> String {
        match self.run(name, args).result {
            Err(Stop::Unsupported(reason)) => reason,
            other => panic!("{name}: expected unsupported, got {other:?}"),
        }
    }
}

/// The lanes of `vector` as text: `1 2 _ 4`, `_` for poison; integers
/// unsigned, floats by their bits.
pub fn lanes(vector: &Vector) -> String {
    let lanes: Vec<String> = (0..vector.lane_count())
        .map(|index| match vector.lane(index) {
            None => "_".to_owned(),
            Some(Val::Int { value, .. }) => value.to_string(),
            Some(Val::Ptr(addr)) => format!("{addr:#x}"),
            Some(Val::Float(bits)) => f32::from_bits(bits).to_string(),
            Some(Val::Double(bits)) => f64::from_bits(bits).to_string(),
            Some(other) => other.to_string(),
        })
        .collect();
    lanes.join(" ")
}
