//! Options of code generation: `spec/codegen.md` §7.4, §8.1, §9.7, §12.

use hod_ir::CodeModel;

/// What a function compile and module emission are asked for besides the
/// module and the body: `llc`'s options that change the object.
#[derive(Clone, Copy, PartialEq, Eq, Default, Debug)]
pub struct Options {
    /// `-relocation-model=` (§7.4). The default is `llc`'s on both ELF
    /// targets: static.
    pub reloc_model: RelocModel,
    /// `-code-model=`; `None` is the module's (`Code Model`), else small.
    pub code_model: Option<CodeModel>,
    /// `-function-sections`: each function in its own section (§9.7).
    pub function_sections: bool,
    /// `-data-sections`: each variable in its own section (§9.7).
    pub data_sections: bool,
    /// The stress options of §12. All off in a product build.
    pub stress: Stress,
    /// `--compress-debug-sections=` (WP-061, `spec/object.md`
    /// "Compressed sections"): how the `.debug_*` sections are written.
    /// `llc` has no such option; `clang -gz=zlib` does this.
    pub debug_compression: hod_obj::Compression,
    /// `-split-dwarf-file=` and `-split-dwarf-output=` (WP-062,
    /// `spec/dwarf.md` §6.14): a skeleton unit in the object and a split
    /// unit in `.dwo` sections, in the object or in a companion object
    /// (`hod_obj::Object::dwo`). The `.dwo` name is the unit's
    /// `splitDebugFilename`; the caller checks it is the option's.
    pub split_dwarf: hod_dwarf::SplitDwarf,
}

/// A `.debug_*` section of 4 GiB or more with compression asked for: the
/// compressor's limit (`hod_obj::Limit::CompressedBytes`).
pub const LIMIT_COMPRESSED_SECTION: &str = "limit:compressed-section";

impl Options {
    /// The code model in force: the option, else the module's, else
    /// small.
    pub fn code_model(&self, module: &hod_ir::Module) -> CodeModel {
        self.code_model.or(module.props.code_model).unwrap_or(CodeModel::Small)
    }
}

/// `llc`'s `-relocation-model=` for ELF (§7.4). The ROPI and RWPI models
/// are ARM-only and not represented.
#[derive(Clone, Copy, PartialEq, Eq, Default, Debug)]
pub enum RelocModel {
    /// `static`: the output is linked at a fixed address.
    #[default]
    Static,
    /// `pic`: position-independent. Whether the output is a program
    /// (PIE) or a shared object is the module's `PIE Level`.
    Pic,
    /// `dynamic-no-pic`: on ELF, as `static`.
    DynamicNoPic,
}

impl RelocModel {
    /// Whether the code is position-independent.
    pub const fn is_pic(self) -> bool {
        matches!(self, RelocModel::Pic)
    }
}

/// The stress options of §12, for tests only. A program must behave the
/// same under all of them.
#[derive(Clone, Copy, PartialEq, Eq, Default, Debug)]
pub struct Stress {
    /// Restrict the pool to its minimum (§2.1): the four lowest general
    /// and the two lowest vector registers of the target's pool.
    pub min_pool: bool,
    /// Compile every function in far-branch mode from the start (§6.3).
    pub far: bool,
    /// Displace every register value to a spill slot after each
    /// instruction (§4.3).
    pub displace: bool,
}

impl Stress {
    /// Every stress option at once.
    pub const ALL: Stress = Stress { min_pool: true, far: true, displace: true };
}
