//! Frame size against `llc -O0` on x86-64 (WP-198, `spec/codegen.md`
//! §5.1 "Value slots"): the tracked number, over the seeds of
//! `data/frame_size.ll` (see `common/frame_size.rs`).
//!
//! The pushes of the save set count: `llc -O0` keeps no value in a
//! callee-saved register, hod's pool is the callee-saved set (§4.4), and
//! those pushes are most of what hod has over `llc` here. At landing the
//! seeds' total was 328 bytes against `llc`'s 216 (1.52; 2.33 before
//! WP-198), the largest function 1.73 times `llc`'s.

#[path = "common/frame_size.rs"]
mod frame_size;

use hod_cg::{Options, RelocModel};

#[test]
fn frame_bytes_against_llc_o0() {
    let options = Options { reloc_model: RelocModel::Pic, ..Options::default() };
    let compile = |module: &hod_ir::Module, bodies: &[hod_ir::Body]| {
        hod_x64::compile_to_object(module, bodies, &options).unwrap_or_else(|e| panic!("hod: {e}"))
    };
    frame_size::check(&frame_size::Target {
        header: format!(
            "target datalayout = \"{}\"\ntarget triple = \"x86_64-unknown-linux-gnu\"\n",
            hod_ir::X86_64_LINUX_DATA_LAYOUT
        ),
        llc_args: &["-mtriple=x86_64-unknown-linux-gnu", "-relocation-model=pic", "-filetype=obj"],
        compile: &compile,
        max_total: 1.6,
        max_function: 1.85,
    });
}
