//! Shared helpers: IR text to a module and its bodies.

#![allow(dead_code, reason = "each test file uses a part")]

use hod_ir::{Body, Module};

/// One function of a module: its name, and its body unless a construct
/// in it is unsupported (then those constructs).
pub struct Function {
    pub name: String,
    pub body: Result<Body, Vec<String>>,
}

/// The module and every function of `text`.
pub fn parse_all(text: &[u8]) -> (Module, Vec<Function>) {
    let options =
        hod_ll::ParseOptions { default_target: Some(hod_ir::TargetSpec::aarch64_linux_gnu()) };
    let parsed = hod_ll::parse_module(text, &options).unwrap_or_else(|e| panic!("hod_ll: {e}"));
    let (side, mut module) = (parsed.text, parsed.module);
    let mut functions = Vec::new();
    hod_ll::parse_bodies(&mut module, &side, |module, parsed| {
        let name = String::from_utf8_lossy(module.symbol(parsed.function.sym).name()).into_owned();
        let body = match parsed.body {
            Some(body) => Ok(body.clone()),
            None => Err(parsed.unsupported.iter().map(ToString::to_string).collect()),
        };
        functions.push(Function { name, body });
        Ok::<(), hod_ll::LlError>(())
    })
    .unwrap_or_else(|e| panic!("hod_ll: {e}"));
    (module, functions)
}

/// The module and the body of the function `name` in `text`.
pub fn parse_one(text: &str, name: &str) -> (Module, Body) {
    let (module, functions) = parse_all(text.as_bytes());
    let function = functions
        .into_iter()
        .find(|f| f.name == name)
        .unwrap_or_else(|| panic!("no function {name}"));
    let body = function.body.unwrap_or_else(|u| panic!("{name}: unsupported: {u:?}"));
    (module, body)
}

/// `llvm-as` of `major` on `text`; `None` when it is not installed (a
/// failure instead under `HOD_REQUIRE_TOOLS=1`).
pub fn assemble(major: u32, text: &[u8]) -> Option<Vec<u8>> {
    let tool = std::path::PathBuf::from(format!("/usr/lib/llvm-{major}/bin/llvm-as"));
    if !tool.is_file() {
        assert!(!hod_testutil::tools_required(), "no llvm-as {major}");
        return None;
    }
    let mut command = std::process::Command::new(tool);
    command.args(["-", "-o", "-"]);
    let out = hod_testutil::run_with(&mut command, text, hod_testutil::DEFAULT_TIMEOUT);
    assert!(out.success(), "llvm-as {major}: {}", out.stderr_text());
    Some(out.stdout)
}

/// The module and every function of `bitcode`, as [`parse_all`].
pub fn read_bitcode(bitcode: &[u8]) -> (Module, Vec<Function>) {
    let options =
        hod_bc::ReadOptions { default_target: Some(hod_ir::TargetSpec::aarch64_linux_gnu()) };
    let read = hod_bc::read_module(bitcode, &options).unwrap_or_else(|e| panic!("hod_bc: {e}"));
    let (bits, mut module) = (read.bits, read.module);
    let mut functions = Vec::new();
    hod_bc::read_bodies(&mut module, &bits, |module, read| {
        let name = String::from_utf8_lossy(module.symbol(read.function.sym).name()).into_owned();
        let body = match read.body {
            Some(body) => Ok(body.clone()),
            None => Err(read.unsupported.iter().map(ToString::to_string).collect()),
        };
        functions.push(Function { name, body });
        Ok::<(), hod_bc::BcError>(())
    })
    .unwrap_or_else(|e| panic!("hod_bc: {e}"));
    (module, functions)
}
