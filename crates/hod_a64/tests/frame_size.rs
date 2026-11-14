//! Frame size against `llc -O0 -global-isel=0 -fast-isel=0` on AArch64
//! (WP-198, `spec/codegen.md` §5.1 "Value slots"): the tracked number,
//! over the seeds of `hod_x64`'s `tests/data/frame_size.ll` (see
//! `hod_x64`'s `tests/common/frame_size.rs`). At landing the seeds' total
//! was 288 bytes against `llc`'s 288 (1.61 before WP-198), the largest
//! function 1.14 times `llc`'s.

#[path = "../../hod_x64/tests/common/frame_size.rs"]
mod frame_size;

use hod_cg::{Options, RelocModel};

#[test]
fn frame_bytes_against_llc_o0() {
    let options = Options { reloc_model: RelocModel::Pic, ..Options::default() };
    let compile = |module: &hod_ir::Module, bodies: &[hod_ir::Body]| {
        hod_a64::compile_to_object(module, bodies, &options).unwrap_or_else(|e| panic!("hod: {e}"))
    };
    frame_size::check(&frame_size::Target {
        header: format!(
            "target datalayout = \"{}\"\ntarget triple = \"aarch64-unknown-linux-gnu\"\n",
            hod_ir::AARCH64_LINUX_DATA_LAYOUT
        ),
        llc_args: &[
            "-mtriple=aarch64-unknown-linux-gnu",
            "-global-isel=0",
            "-fast-isel=0",
            "-relocation-model=pic",
            "-filetype=obj",
        ],
        compile: &compile,
        max_total: 1.1,
        max_function: 1.25,
    });
}
