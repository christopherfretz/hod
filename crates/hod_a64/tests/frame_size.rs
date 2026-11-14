//! Frame size against `llc -O0 -global-isel=0 -fast-isel=0` on AArch64
//! (WP-198, `spec/codegen.md` §5.1 "Value slots"): the tracked number,
//! over the seeds of `hod_x64`'s `tests/data/frame_size.ll` (see
//! `hod_x64`'s `tests/common/frame_size.rs`). At landing the seeds' total
//! was 288 bytes against `llc`'s 288 (1.61 before WP-198), the largest
//! function 1.14 times `llc`'s.

#[path = "../../hod_x64/tests/common/frame_size.rs"]
mod frame_size;

#[path = "../../hod_obj/tests/common/linking.rs"]
#[allow(dead_code, reason = "a shared helper: not every function is used here")]
mod linking;

use hod_cg::{Options, RelocModel, Stress};
use hod_testutil::Scratch;
use linking::{Toolchain, exit_code};

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

/// §5.1 "Value slots" 3: no block slot is shared across a `returns_twice`
/// call; the program of `frame_size::SETJMP_PROGRAM` exits 0, plainly and
/// under `Stress::ALL`, with every linker.
#[test]
fn no_block_slot_across_setjmp() {
    let dir = Scratch::new("frame_size_setjmp");
    let Some(toolchain) = Toolchain::aarch64(&dir) else { return };
    let text = format!(
        "target datalayout = \"{}\"\ntarget triple = \"aarch64-unknown-linux-gnu\"\n{}",
        hod_ir::AARCH64_LINUX_DATA_LAYOUT,
        frame_size::SETJMP_PROGRAM
    );
    let (module, bodies) = frame_size::parse(&text);
    for (name, stress) in [("plain", Stress::default()), ("stress", Stress::ALL)] {
        let options = Options { reloc_model: RelocModel::Pic, stress, ..Options::default() };
        let bytes = hod_a64::compile_to_object(&module, &bodies, &options)
            .unwrap_or_else(|e| panic!("hod: {e}"));
        let object = dir.write(&format!("{name}.o"), bytes);
        for linker in &toolchain.linkers {
            let exe = dir.join(format!("{name}-{}", linker.name));
            let ran = toolchain.link_and_run(linker, &[], &[&object], &exe);
            assert_eq!(exit_code(&ran), Some(0), "{name}, {}: {}", linker.name, ran.stderr_text());
        }
    }
}
