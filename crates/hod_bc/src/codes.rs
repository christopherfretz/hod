//! Block ids and record codes of LLVM's bitcode (`LLVMBitCodes.h`), as
//! far as the module level reads them. Each layout used is in
//! `spec/bc.md` §4, checked against `llvm-bcanalyzer` output of LLVM 19
//! and 22.

/// Block ids.
pub mod block {
    pub const MODULE: u32 = 8;
    pub const PARAMATTR: u32 = 9;
    pub const PARAMATTR_GROUP: u32 = 10;
    pub const CONSTANTS: u32 = 11;
    pub const FUNCTION: u32 = 12;
    pub const IDENTIFICATION: u32 = 13;
    pub const VALUE_SYMTAB: u32 = 14;
    pub const METADATA: u32 = 15;
    pub const METADATA_ATTACHMENT: u32 = 16;
    pub const TYPE: u32 = 17;
    pub const USELIST: u32 = 18;
    pub const MODULE_STRTAB: u32 = 19;
    pub const GLOBALVAL_SUMMARY: u32 = 20;
    pub const OPERAND_BUNDLE_TAGS: u32 = 21;
    pub const METADATA_KIND: u32 = 22;
    pub const STRTAB: u32 = 23;
    pub const FULL_LTO_GLOBALVAL_SUMMARY: u32 = 24;
    pub const SYMTAB: u32 = 25;
    pub const SYNC_SCOPE_NAMES: u32 = 26;
}

/// `MODULE_CODE_*`.
pub mod module {
    pub const VERSION: u32 = 1;
    pub const TRIPLE: u32 = 2;
    pub const DATALAYOUT: u32 = 3;
    pub const ASM: u32 = 4;
    pub const SECTIONNAME: u32 = 5;
    pub const DEPLIB: u32 = 6;
    pub const GLOBALVAR: u32 = 7;
    pub const FUNCTION: u32 = 8;
    pub const ALIAS_OLD: u32 = 9;
    pub const GCNAME: u32 = 11;
    pub const COMDAT: u32 = 12;
    pub const VSTOFFSET: u32 = 13;
    pub const ALIAS: u32 = 14;
    pub const METADATA_VALUES_UNUSED: u32 = 15;
    pub const SOURCE_FILENAME: u32 = 16;
    pub const HASH: u32 = 17;
    pub const IFUNC: u32 = 18;
}

/// `IDENTIFICATION_CODE_*`.
pub mod ident {
    pub const STRING: u32 = 1;
    pub const EPOCH: u32 = 2;
}

/// `TYPE_CODE_*`.
pub mod ty {
    pub const NUMENTRY: u32 = 1;
    pub const VOID: u32 = 2;
    pub const FLOAT: u32 = 3;
    pub const DOUBLE: u32 = 4;
    pub const LABEL: u32 = 5;
    pub const OPAQUE: u32 = 6;
    pub const INTEGER: u32 = 7;
    pub const POINTER: u32 = 8;
    pub const FUNCTION_OLD: u32 = 9;
    pub const HALF: u32 = 10;
    pub const ARRAY: u32 = 11;
    pub const VECTOR: u32 = 12;
    pub const X86_FP80: u32 = 13;
    pub const FP128: u32 = 14;
    pub const PPC_FP128: u32 = 15;
    pub const METADATA: u32 = 16;
    pub const X86_MMX: u32 = 17;
    pub const STRUCT_ANON: u32 = 18;
    pub const STRUCT_NAME: u32 = 19;
    pub const STRUCT_NAMED: u32 = 20;
    pub const FUNCTION: u32 = 21;
    pub const TOKEN: u32 = 22;
    pub const BFLOAT: u32 = 23;
    pub const X86_AMX: u32 = 24;
    pub const OPAQUE_POINTER: u32 = 25;
    pub const TARGET_TYPE: u32 = 26;
}

/// `PARAMATTR_*`.
pub mod attr {
    pub const ENTRY_OLD: u32 = 1;
    pub const ENTRY: u32 = 2;
    pub const GRP_ENTRY: u32 = 3;
}

/// `CST_CODE_*`.
pub mod cst {
    pub const SETTYPE: u32 = 1;
    pub const NULL: u32 = 2;
    pub const UNDEF: u32 = 3;
    pub const INTEGER: u32 = 4;
    pub const WIDE_INTEGER: u32 = 5;
    pub const FLOAT: u32 = 6;
    pub const AGGREGATE: u32 = 7;
    pub const STRING: u32 = 8;
    pub const CSTRING: u32 = 9;
    pub const CE_BINOP: u32 = 10;
    pub const CE_CAST: u32 = 11;
    pub const CE_GEP_OLD: u32 = 12;
    pub const CE_SELECT: u32 = 13;
    pub const CE_EXTRACTELT: u32 = 14;
    pub const CE_INSERTELT: u32 = 15;
    pub const CE_SHUFFLEVEC: u32 = 16;
    pub const CE_CMP: u32 = 17;
    pub const INLINEASM_OLD: u32 = 18;
    pub const CE_SHUFVEC_EX: u32 = 19;
    pub const CE_INBOUNDS_GEP: u32 = 20;
    pub const BLOCKADDRESS: u32 = 21;
    pub const DATA: u32 = 22;
    pub const INLINEASM_OLD2: u32 = 23;
    pub const CE_GEP_WITH_INRANGE_INDEX_OLD: u32 = 24;
    pub const CE_UNOP: u32 = 25;
    pub const POISON: u32 = 26;
    pub const DSO_LOCAL_EQUIVALENT: u32 = 27;
    pub const INLINEASM_OLD3: u32 = 28;
    pub const NO_CFI_VALUE: u32 = 29;
    pub const INLINEASM: u32 = 30;
    pub const CE_GEP_WITH_INRANGE: u32 = 31;
    pub const CE_GEP: u32 = 32;
    pub const PTRAUTH: u32 = 33;
    pub const PTRAUTH2: u32 = 34;
}

/// `METADATA_*`.
pub mod md {
    pub const STRING_OLD: u32 = 1;
    pub const VALUE: u32 = 2;
    pub const NODE: u32 = 3;
    pub const NAME: u32 = 4;
    pub const DISTINCT_NODE: u32 = 5;
    pub const KIND: u32 = 6;
    pub const NAMED_NODE: u32 = 10;
    pub const ATTACHMENT: u32 = 11;
    pub const STRINGS: u32 = 35;
    pub const GLOBAL_DECL_ATTACHMENT: u32 = 36;
    pub const INDEX_OFFSET: u32 = 38;
    pub const INDEX: u32 = 39;
    pub const ARG_LIST: u32 = 46;

    /// Whether a record of `code` in a module's METADATA block defines
    /// the next metadata id: every node and value record of LLVM 19 to
    /// 22. `None` for a code this reader does not know.
    pub fn defines_id(code: u32) -> Option<bool> {
        match code {
            // STRING_OLD, VALUE, NODE, DISTINCT_NODE, LOCATION, OLD_NODE,
            // OLD_FN_NODE, and the debug nodes GENERIC_DEBUG (12) to
            // MACRO_FILE (34), GLOBAL_VAR_EXPR (37), LABEL (40),
            // STRING_TYPE (41), COMMON_BLOCK (44), GENERIC_SUBRANGE (45),
            // ASSIGN_ID (47), SUBRANGE_TYPE (48), FIXED_POINT_TYPE (49).
            1..=3 | 5 | 7..=9 | 12..=34 | 37 | 40 | 41 | 44 | 45 | 47..=49 => Some(true),
            // NAME, KIND, NAMED_NODE, ATTACHMENT, GLOBAL_DECL_ATTACHMENT,
            // INDEX_OFFSET, INDEX, ARG_LIST.
            4 | 6 | 10 | 11 | 36 | 38 | 39 | 46 => Some(false),
            _ => None,
        }
    }
}

/// The `SYNC_SCOPE_NAME` record.
/// `FUNCTION_BLOCK` records (`FunctionCodes`).
pub mod func {
    pub const DECLAREBLOCKS: u32 = 1;
    pub const BINOP: u32 = 2;
    pub const CAST: u32 = 3;
    pub const GEP_OLD: u32 = 4;
    pub const SELECT: u32 = 5;
    pub const EXTRACTELT: u32 = 6;
    pub const INSERTELT: u32 = 7;
    pub const SHUFFLEVEC: u32 = 8;
    pub const CMP: u32 = 9;
    pub const RET: u32 = 10;
    pub const BR: u32 = 11;
    pub const SWITCH: u32 = 12;
    pub const INVOKE: u32 = 13;
    pub const UNREACHABLE: u32 = 15;
    pub const PHI: u32 = 16;
    pub const ALLOCA: u32 = 19;
    pub const LOAD: u32 = 20;
    pub const VAARG: u32 = 23;
    pub const STORE_OLD: u32 = 24;
    pub const EXTRACTVAL: u32 = 26;
    pub const INSERTVAL: u32 = 27;
    pub const CMP2: u32 = 28;
    pub const VSELECT: u32 = 29;
    pub const INBOUNDS_GEP_OLD: u32 = 30;
    pub const INDIRECTBR: u32 = 31;
    pub const DEBUG_LOC_AGAIN: u32 = 33;
    pub const CALL: u32 = 34;
    pub const DEBUG_LOC: u32 = 35;
    pub const FENCE: u32 = 36;
    pub const CMPXCHG_OLD: u32 = 37;
    pub const ATOMICRMW_OLD: u32 = 38;
    pub const RESUME: u32 = 39;
    pub const LANDINGPAD_OLD: u32 = 40;
    pub const LOADATOMIC: u32 = 41;
    pub const STOREATOMIC_OLD: u32 = 42;
    pub const GEP: u32 = 43;
    pub const STORE: u32 = 44;
    pub const STOREATOMIC: u32 = 45;
    pub const CMPXCHG: u32 = 46;
    pub const LANDINGPAD: u32 = 47;
    pub const CLEANUPRET: u32 = 48;
    pub const CATCHRET: u32 = 49;
    pub const CATCHPAD: u32 = 50;
    pub const CLEANUPPAD: u32 = 51;
    pub const CATCHSWITCH: u32 = 52;
    pub const OPERAND_BUNDLE: u32 = 55;
    pub const UNOP: u32 = 56;
    pub const CALLBR: u32 = 57;
    pub const FREEZE: u32 = 58;
    pub const ATOMICRMW: u32 = 59;
    pub const BLOCKADDR_USERS: u32 = 60;
    /// `DEBUG_RECORD_VALUE` (61) to `DEBUG_RECORD_VALUE_SIMPLE` (65), and
    /// 66, which LLVM 22 adds: debug records, stepped over.
    pub const DEBUG_RECORD_FIRST: u32 = 61;
    pub const DEBUG_RECORD_LAST: u32 = 66;
}

pub const SYNC_SCOPE_NAME: u32 = 1;
/// The `OPERAND_BUNDLE_TAG` record.
pub const OPERAND_BUNDLE_TAG: u32 = 1;
/// The `STRTAB_BLOB` record.
pub const STRTAB_BLOB: u32 = 1;
