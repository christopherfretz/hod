//! `spec/object.md` "Link-and-run tests", thread-local storage: objects
//! with `.tdata` and `.tbss` variables and hand-encoded functions that
//! compute their addresses through each TLS access model are linked by GNU
//! ld, lld and mold, into a program and into a shared object, and a C
//! driver compares what they return with the addresses its own compiler
//! computes, in the main thread and in a second one.
//!
//! The AArch64 tests exist on an AArch64 Linux host; the x86-64 tests run
//! anywhere the x86-64 toolchain and emulator are (`Toolchain::x86_64`). A
//! missing tool skips its part with a note, or fails under
//! `HOD_REQUIRE_TOOLS=1`.

#![cfg(target_os = "linux")]

mod common;

use std::fmt::Write as _;
use std::path::Path;

use common::linking::{Toolchain, exit_code, write};
use common::{define, function_bytes, undefined, words};
use hod_obj::{
    A64Reloc, Arch, Binding, Object, SectionId, SectionKind, SymbolId, SymbolKind, X64Reloc,
};
use hod_testutil::{Scratch, run, tool};

/// How a function finds a thread-local variable.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Model {
    /// The offset from the thread pointer is a link-time constant.
    LocalExec,
    /// The offset is loaded from the GOT.
    InitialExec,
    /// `__tls_get_addr` is called (x86-64 only here).
    GeneralDynamic,
    /// The variable's TLS descriptor is called.
    Descriptor,
}

impl Model {
    fn prefix(self) -> &'static str {
        match self {
            Self::LocalExec => "le",
            Self::InitialExec => "ie",
            Self::GeneralDynamic => "gd",
            Self::Descriptor => "desc",
        }
    }
}

/// The variables: `int tinit = 30` and a local `int tlocal = 7` in
/// `.tdata`, behind eight bytes of padding; `long tzero` in `.tbss`,
/// behind 8 KiB, so that its offset from the thread pointer does not fit
/// twelve bits.
struct Variables {
    tinit: SymbolId,
    tlocal: SymbolId,
    tzero: SymbolId,
}

fn variables(obj: &mut Object) -> Variables {
    let tdata = obj.add_section(b".tdata", SectionKind::Tls, 8).unwrap();
    let tbss = obj.add_section(b".tbss", SectionKind::TlsBss, 16).unwrap();
    obj.append(tdata, &[0x11; 8]);
    obj.append(tdata, &30u32.to_le_bytes());
    obj.append(tdata, &7u32.to_le_bytes());
    obj.append_zeros(tbss, 8192 + 8);
    let tls = SymbolKind::Tls;
    Variables {
        tinit: define(obj, "tinit", tls, Binding::Global, tdata, 8, 4),
        tlocal: define(obj, "tlocal", tls, Binding::Local, tdata, 12, 4),
        tzero: define(obj, "tzero", tls, Binding::Global, tbss, 8192, 8),
    }
}

/// Adds `void *name(void)`, which returns the address of `variable` in the
/// calling thread, found as `model` says.
fn accessor(obj: &mut Object, text: SectionId, name: &str, model: Model, variable: SymbolId) {
    match obj.arch() {
        Arch::AArch64 => accessor_a64(obj, text, name, model, variable),
        Arch::X86_64 => accessor_x64(obj, text, name, model, variable),
    }
}

fn accessor_a64(obj: &mut Object, text: SectionId, name: &str, model: Model, variable: SymbolId) {
    use common::{ADD_X0_LO12, ADD_X0_X1_X0, ADRP_X0, LDR_X0_LO12, MRS_X0_TP, MRS_X1_TP};
    use common::{POP_FRAME, PUSH_FRAME, RET, SET_FP};
    let (code, relocations): (&[u32], &[(u64, A64Reloc)]) = match model {
        Model::LocalExec => (
            // add x0, x0, #hi12, lsl #12; add x0, x0, #lo12
            &[MRS_X0_TP, 0x9140_0000, ADD_X0_LO12, RET],
            &[(4, A64Reloc::TlsleAddTprelHi12), (8, A64Reloc::TlsleAddTprelLo12Nc)],
        ),
        Model::InitialExec => (
            &[ADRP_X0, LDR_X0_LO12, MRS_X1_TP, ADD_X0_X1_X0, RET],
            &[(0, A64Reloc::TlsieAdrGottprelPage21), (4, A64Reloc::TlsieLd64GottprelLo12Nc)],
        ),
        Model::Descriptor => (
            // adrp x0, desc; ldr x1, [x0, #lo12]; add x0, x0, #lo12; blr x1
            &[
                PUSH_FRAME,
                SET_FP,
                ADRP_X0,
                0xf940_0001,
                ADD_X0_LO12,
                0xd63f_0020,
                MRS_X1_TP,
                ADD_X0_X1_X0,
                POP_FRAME,
                RET,
            ],
            &[
                (8, A64Reloc::TlsdescAdrPage21),
                (12, A64Reloc::TlsdescLd64Lo12),
                (16, A64Reloc::TlsdescAddLo12),
                (20, A64Reloc::TlsdescCall),
            ],
        ),
        Model::GeneralDynamic => unreachable!("AArch64 code uses descriptors"),
    };
    let function = function_bytes(obj, text, name, &words(code));
    let start = obj.symbol(function).value;
    for &(offset, kind) in relocations {
        obj.add_relocation(text, start + offset, kind, variable, 0);
    }
}

fn accessor_x64(obj: &mut Object, text: SectionId, name: &str, model: Model, variable: SymbolId) {
    #[rustfmt::skip]
    let (code, relocations): (&[u8], &[(u64, X64Reloc, i64)]) = match model {
        Model::LocalExec => (
            &[
                0x64, 0x48, 0x8b, 0x04, 0x25, 0, 0, 0, 0, // mov %fs:0, %rax
                0x48, 0x8d, 0x80, 0, 0, 0, 0,             // lea v@tpoff(%rax), %rax
                0xc3,                                     // ret
            ],
            &[(12, X64Reloc::TpOff32, 0)],
        ),
        Model::InitialExec => (
            &[
                0x48, 0x8b, 0x05, 0, 0, 0, 0,             // mov v@gottpoff(%rip), %rax
                0x64, 0x48, 0x03, 0x04, 0x25, 0, 0, 0, 0, // add %fs:0, %rax
                0xc3,                                     // ret
            ],
            &[(3, X64Reloc::GotTpOff, -4)],
        ),
        Model::GeneralDynamic => (
            &[
                0x55,                                     // push %rbp
                0x48, 0x89, 0xe5,                         // mov %rsp, %rbp
                0x66, 0x48, 0x8d, 0x3d, 0, 0, 0, 0,       // data16 lea v@tlsgd(%rip), %rdi
                0x66, 0x66, 0x48, 0xe8, 0, 0, 0, 0,       // data16 data16 rex.W call __tls_get_addr@PLT
                0x5d,                                     // pop %rbp
                0xc3,                                     // ret
            ],
            &[(8, X64Reloc::TlsGd, -4), (16, X64Reloc::Plt32, -4)],
        ),
        Model::Descriptor => (
            &[
                0x55,                                     // push %rbp
                0x48, 0x89, 0xe5,                         // mov %rsp, %rbp
                0x48, 0x8d, 0x05, 0, 0, 0, 0,             // lea v@tlsdesc(%rip), %rax
                0xff, 0x10,                               // call *v@tlscall(%rax)
                0x64, 0x48, 0x03, 0x04, 0x25, 0, 0, 0, 0, // add %fs:0, %rax
                0x5d,                                     // pop %rbp
                0xc3,                                     // ret
            ],
            &[(7, X64Reloc::GotPc32Tlsdesc, -4), (11, X64Reloc::TlsdescCall, 0)],
        ),
    };
    let function = function_bytes(obj, text, name, code);
    let start = obj.symbol(function).value;
    for &(offset, kind, addend) in relocations {
        // The second relocation of the general-dynamic pair is the call.
        let symbol =
            if kind == X64Reloc::Plt32 { undefined(obj, "__tls_get_addr") } else { variable };
        obj.add_relocation(text, start + offset, kind, symbol, addend);
    }
}

/// An object with the variables and one accessor per model and variable,
/// named `<model>_<variable>`, and the C program that checks them.
fn object_and_driver(arch: Arch, models: &[Model]) -> (Object, String) {
    let mut obj = Object::new(arch);
    let text = obj.add_section(b".text", SectionKind::Text, 16).unwrap();
    let vars = variables(&mut obj);
    let mut declarations = String::new();
    let mut checks = String::new();
    let mut local = "";
    for (index, &model) in models.iter().enumerate() {
        let prefix = model.prefix();
        for (variable, symbol) in
            [("tinit", vars.tinit), ("tzero", vars.tzero), ("tlocal", vars.tlocal)]
        {
            let name = format!("{prefix}_{variable}");
            accessor(&mut obj, text, &name, model, symbol);
            writeln!(declarations, "void *{name}(void);").unwrap();
            let expected = if variable == "tlocal" { "local" } else { &format!("&{variable}") };
            let code = 10 * (index + 1) + checks.lines().count() % 3;
            writeln!(checks, "    if ({name}() != (void *){expected}) return {code};").unwrap();
        }
        if local.is_empty() {
            local = prefix;
        }
    }
    let driver = format!(
        "#include <pthread.h>
extern __thread int tinit;
extern __thread long tzero;
{declarations}
/* Zero when this thread's variables have their initial values and every
   accessor agrees with the compiler; then changes this thread's copies. */
static long check(void) {{
    int *local = {local}_tlocal();
    if (tinit != 30 || tzero != 0 || *local != 7) return 1;
{checks}    tinit = 31;
    tzero = 5;
    *local = 8;
    return 0;
}}
static void *worker(void *unused) {{
    (void)unused;
    return (void *)check();
}}
int main(void) {{
    pthread_t thread;
    void *result;
    long failed = check();
    if (failed) return failed;
    if (pthread_create(&thread, 0, worker, 0) || pthread_join(thread, &result)) return 100;
    if (result) return 100 + (long)result;
    return tinit + tzero + *(int *){local}_tlocal() - 2;
}}
"
    );
    (obj, driver)
}

/// Every access model an executable may use, linked into a program both
/// position-independent and not. The linker relaxes the descriptor and
/// general-dynamic sequences to cheaper models.
fn tls_in_a_program(tools: &Toolchain, scratch: &Scratch, arch: Arch, models: &[Model]) {
    let (obj, driver) = object_and_driver(arch, models);
    let object = write(scratch, "tls.o", &obj);
    let driver = tools.compile_c(scratch, "driver", &driver, &["-fPIE"]);
    for linker in &tools.linkers {
        for mode in ["-pie", "-no-pie"] {
            let program = scratch.join(format!("tls{mode}-{}", linker.name));
            let ran =
                tools.link_and_run(linker, &[mode, "-pthread"], &[&object, &driver], &program);
            assert_eq!(exit_code(&ran), Some(42), "{} {mode}", linker.name);
        }
    }
}

/// Models a shared object may use, linked with `-shared`; the driver is a
/// program that loads the shared object. Nothing can be relaxed there:
/// the dynamic loader resolves the accesses, which the dynamic relocations
/// named in `dynamic` (the shared object must have each) confirm.
fn tls_in_a_shared_object(tools: &Toolchain, arch: Arch, models: &[Model], dynamic: &[&str]) {
    let Some(readelf) = tool("llvm-readelf") else { return };
    let scratch = Scratch::new("obj-tls-shared");
    let (obj, driver) = object_and_driver(arch, models);
    let object = write(&scratch, "tls.o", &obj);
    let driver = tools.compile_c(&scratch, "driver", &driver, &["-fPIE"]);
    for linker in &tools.linkers {
        let library = scratch.join(format!("libtls-{}.so", linker.name));
        tools.link_ok(linker, &["-shared"], &[&object], &library);
        let relocations = run(&readelf, [Path::new("-r"), &library]).stdout_text();
        for name in dynamic {
            assert!(relocations.contains(name), "{}: no {name} in:\n{relocations}", linker.name);
        }
        let program = scratch.join(format!("tls-shared-{}", linker.name));
        let ran = tools.link_and_run(linker, &["-pthread"], &[&driver, &library], &program);
        assert_eq!(exit_code(&ran), Some(42), "{}", linker.name);
    }
}

/// AArch64: local exec (`TLSLE_ADD_TPREL_HI12`, `TLSLE_ADD_TPREL_LO12_NC`),
/// initial exec (`TLSIE_ADR_GOTTPREL_PAGE21`,
/// `TLSIE_LD64_GOTTPREL_LO12_NC`) and descriptors (`TLSDESC_ADR_PAGE21`,
/// `TLSDESC_LD64_LO12`, `TLSDESC_ADD_LO12`, `TLSDESC_CALL`) in a program.
#[cfg(target_arch = "aarch64")]
#[test]
fn aarch64_tls_in_a_program() {
    let scratch = Scratch::new("obj-tls-a64");
    let Some(tools) = Toolchain::host(&scratch) else { return };
    let models = [Model::LocalExec, Model::InitialExec, Model::Descriptor];
    tls_in_a_program(&tools, &scratch, Arch::AArch64, &models);
}

/// AArch64: descriptors, and initial exec, in a shared object. They are
/// linked separately because GNU ld turns a descriptor access into initial
/// exec when the same variable is also accessed that way.
#[cfg(target_arch = "aarch64")]
#[test]
fn aarch64_tls_in_a_shared_object() {
    let scratch = Scratch::new("obj-tls-a64-so");
    let Some(tools) = Toolchain::host(&scratch) else { return };
    tls_in_a_shared_object(&tools, Arch::AArch64, &[Model::Descriptor], &["R_AARCH64_TLSDESC"]);
    let initial_exec = ["R_AARCH64_TLS_TPREL64"];
    tls_in_a_shared_object(&tools, Arch::AArch64, &[Model::InitialExec], &initial_exec);
}

/// x86-64: local exec (`TPOFF32`), initial exec (`GOTTPOFF`), general
/// dynamic (`TLSGD` with its `PLT32` call) and descriptors
/// (`GOTPC32_TLSDESC`, `TLSDESC_CALL`) in a program.
#[test]
fn x86_64_tls_in_a_program() {
    let scratch = Scratch::new("obj-tls-x64");
    let Some(tools) = Toolchain::x86_64(&scratch) else { return };
    let models = [Model::LocalExec, Model::InitialExec, Model::GeneralDynamic, Model::Descriptor];
    tls_in_a_program(&tools, &scratch, Arch::X86_64, &models);
}

/// x86-64: general dynamic and descriptors, and initial exec, in a shared
/// object.
#[test]
fn x86_64_tls_in_a_shared_object() {
    let scratch = Scratch::new("obj-tls-x64-so");
    let Some(tools) = Toolchain::x86_64(&scratch) else { return };
    let models = [Model::GeneralDynamic, Model::Descriptor];
    let dynamic = ["R_X86_64_DTPMOD64", "R_X86_64_TLSDESC"];
    tls_in_a_shared_object(&tools, Arch::X86_64, &models, &dynamic);
    tls_in_a_shared_object(&tools, Arch::X86_64, &[Model::InitialExec], &["R_X86_64_TPOFF64"]);
}
