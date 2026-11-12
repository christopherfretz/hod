//! Shared support for `hod_a64`'s tests: modules from the builder or
//! from `.ll` text, objects, `llc`, `llvm-objdump`, and native runs.
//! Each test file uses part of it.

#![allow(dead_code, reason = "each test binary uses part of the support")]

#[path = "../../../hod_obj/tests/common/linking.rs"]
pub mod linking;

use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

use hod_a64::{A64, compile_module};
use hod_cg::{CgError, Chunk, Emitted, Options, Scratch, compile_function};
use hod_ir::{
    Body, FunctionBuilder, FunctionDesc, Linkage, Module, SymId, TargetSpec, TypeId, Value,
};
use hod_testutil::{run_with, tool};

/// A module for AArch64 Linux.
pub fn module() -> Module {
    Module::new(TargetSpec::aarch64_linux_gnu())
}

/// Declares a defined external function `name` of `ret (params)`.
pub fn define(m: &mut Module, name: &str, ret: TypeId, params: &[TypeId]) -> (SymId, TypeId) {
    let ty = m.types.function(ret, params, false).unwrap();
    let desc = FunctionDesc { has_body: true, dso_local: true, ..FunctionDesc::new(ty) };
    (m.add_function(name.as_bytes(), &desc).unwrap(), ty)
}

/// Declares a defined internal function.
pub fn define_internal(
    m: &mut Module,
    name: &str,
    ret: TypeId,
    params: &[TypeId],
) -> (SymId, TypeId) {
    let ty = m.types.function(ret, params, false).unwrap();
    let desc = FunctionDesc {
        has_body: true,
        dso_local: true,
        linkage: Linkage::Internal,
        ..FunctionDesc::new(ty)
    };
    (m.add_function(name.as_bytes(), &desc).unwrap(), ty)
}

/// An integer constant operand.
pub fn int(m: &mut Module, ty: TypeId, value: u64) -> Value {
    Value::constant(m.consts.int(&m.types, ty, value).unwrap()).unwrap()
}

/// `i32 main() { ret i32 <value> }`, built with `FunctionBuilder`.
pub fn returns(value: u32) -> (Module, Vec<Body>) {
    let mut m = module();
    let (sym, ty) = define(&mut m, "main", TypeId::I32, &[]);
    let v = int(&mut m, TypeId::I32, u64::from(value));
    let mut b = FunctionBuilder::new(&m, sym, ty).unwrap();
    let entry = b.create_block().unwrap();
    b.begin_block(entry).unwrap();
    b.ret(Some(v)).unwrap();
    let body = b.finish().unwrap();
    body.verify(&m).unwrap();
    (m, vec![body])
}

/// A module and its bodies from `.ll` text (target lines included).
pub fn parse(text: &str) -> (Module, Vec<Body>) {
    let parsed = hod_ll::parse_module(text.as_bytes(), &hod_ll::ParseOptions::default())
        .unwrap_or_else(|e| panic!("hod_ll: {e}\n{text}"));
    let text_side = parsed.text;
    let mut module = parsed.module;
    let mut bodies = Vec::new();
    hod_ll::parse_bodies(&mut module, &text_side, |_, parsed| {
        let body = parsed.body.unwrap_or_else(|| {
            panic!("hod_ll: unsupported {:?} in\n{text}", parsed.unsupported)
        });
        bodies.push(body.clone());
        Ok::<(), hod_ll::LlError>(())
    })
    .unwrap_or_else(|e| panic!("hod_ll: {e}\n{text}"));
    (module, bodies)
}

/// The header of a `.ll` module for AArch64 Linux, for `llc` `major`.
pub fn ll_header(major: u32) -> String {
    let layout = if major >= 20 {
        hod_ir::AARCH64_LINUX_DATA_LAYOUT
    } else {
        hod_ir::AARCH64_LINUX_DATA_LAYOUT_LLVM19
    };
    format!("target datalayout = \"{layout}\"\ntarget triple = \"aarch64-unknown-linux-gnu\"\n")
}

/// Compiles every body of a module into one chunk.
pub fn chunk(module: &Module, bodies: &[Body], options: &Options) -> Result<Chunk, CgError> {
    let mut scratch = Scratch::new();
    let mut chunk = Chunk::new();
    for body in bodies {
        compile_function(&A64, module, body, options, &mut scratch, &mut chunk)?;
    }
    Ok(chunk)
}

/// Compiles and emits a module, panicking with the reason on failure.
pub fn emit(module: &Module, bodies: &[Body], options: &Options) -> Emitted {
    compile_module(module, bodies, options).unwrap_or_else(|e| panic!("{e}"))
}

/// Writes the module's object into `dir` as `name`.
pub fn write_object(dir: &hod_testutil::Scratch, name: &str, emitted: &Emitted) -> PathBuf {
    dir.write(name, hod_obj::elf::to_vec(&emitted.object).unwrap())
}

/// The code words of the module's first function.
pub fn code_words(module: &Module, bodies: &[Body], options: &Options) -> Vec<u32> {
    let chunk = chunk(module, bodies, options).unwrap_or_else(|e| panic!("{e}"));
    chunk.code.chunks(4).map(|w| u32::from_le_bytes([w[0], w[1], w[2], w[3]])).collect()
}

/// `llvm-objdump -d` of an object, its instruction lines only, as
/// `mnemonic operands` with addresses and bytes dropped.
pub fn objdump(object: &Path) -> Option<Vec<String>> {
    let objdump = tool("llvm-objdump")?;
    let mut command = Command::new(objdump);
    command.args(["-d", "--no-show-raw-insn", "--no-leading-addr"]).arg(object);
    let outcome = run_with(&mut command, &[], Duration::from_mins(1));
    assert!(outcome.success(), "llvm-objdump: {}", outcome.stderr_text());
    Some(
        outcome
            .stdout_text()
            .lines()
            .filter(|line| line.starts_with(char::is_whitespace) && !line.trim().is_empty())
            .map(|line| line.split_whitespace().collect::<Vec<_>>().join(" "))
            .collect(),
    )
}

/// `llc -O0 -global-isel=0 -fast-isel=0` of `text` to an object.
pub fn llc(dir: &hod_testutil::Scratch, name: &str, text: &str, flags: &[&str]) -> Option<PathBuf> {
    let llc = tool("llc")?;
    let input = dir.write(&format!("{name}.ll"), text);
    let output = dir.join(format!("{name}.o"));
    let mut command = Command::new(llc);
    command
        .args(["-O0", "-global-isel=0", "-fast-isel=0", "-filetype=obj"])
        .args(flags)
        .arg(&input)
        .arg("-o")
        .arg(&output);
    let outcome = run_with(&mut command, &[], Duration::from_mins(2));
    assert!(outcome.success(), "llc failed:\n{}", outcome.stderr_text());
    Some(output)
}

/// The `llvm.*` major `find_tool("llc")` resolves to.
pub fn llc_major() -> u32 {
    let Some(llc) = hod_testutil::find_tool("llc") else { return 22 };
    let text = llc.to_string_lossy().into_owned();
    hod_testutil::LLVM_MAJORS
        .into_iter()
        .find(|major| text.contains(&format!("-{major}")))
        .unwrap_or(22)
}
