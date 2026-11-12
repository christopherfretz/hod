//! Support shared by the module-level tests (`spec/ir.md` §2, §5.7): the
//! LLVM tools of one major version, by path.
//!
//! Each test file uses part of this module.
#![allow(dead_code)]

use std::path::PathBuf;

use hod_testutil::Scratch;

/// The LLVM majors the module-level rules are checked against.
pub const MAJORS: [u32; 2] = [22, 19];

/// `/usr/lib/llvm-<major>/bin/<name>`.
///
/// `None`, after a note, when it is missing and tools are optional.
///
/// # Panics
/// Panics when it is missing and `HOD_REQUIRE_TOOLS=1`.
pub fn llvm_tool(major: u32, name: &str) -> Option<PathBuf> {
    let path = PathBuf::from(format!("/usr/lib/llvm-{major}/bin/{name}"));
    if path.is_file() {
        return Some(path);
    }
    assert!(
        !hod_testutil::tools_required(),
        "{} not found and HOD_REQUIRE_TOOLS=1",
        path.display()
    );
    eprintln!("note: skipping: {} not found", path.display());
    None
}

/// `llvm-as` of one major version.
pub struct Assembler {
    pub major: u32,
    path: PathBuf,
}

impl Assembler {
    /// Finds `llvm-as` of `major`; see [`llvm_tool`].
    pub fn new(major: u32) -> Option<Assembler> {
        llvm_tool(major, "llvm-as").map(|path| Assembler { major, path })
    }

    /// Assembles `ir`, with the verifier on or off. `Err` holds the
    /// diagnostics.
    pub fn assemble(
        &self,
        scratch: &Scratch,
        name: &str,
        ir: &str,
        verify: bool,
    ) -> Result<(), String> {
        let input = scratch.write(&format!("{name}.ll"), ir);
        let output = scratch.join(format!("{name}.bc"));
        let mut args: Vec<&std::ffi::OsStr> = Vec::new();
        if !verify {
            args.push("-disable-verify".as_ref());
        }
        args.extend(["-o".as_ref(), output.as_os_str(), input.as_os_str()]);
        let outcome = hod_testutil::run(&self.path, args);
        assert!(!outcome.timed_out(), "llvm-as timed out on {name}");
        if outcome.success() { Ok(()) } else { Err(outcome.stderr_text()) }
    }
}
