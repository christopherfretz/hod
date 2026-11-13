//! Keyword codes (`spec/ll.md` §2.2): the lexer gives each word token
//! that is one of the words the parser looks for its [`Kw`], so that a
//! test of a word compares one byte and not a byte string.
//!
//! The code is found with a perfect hash of the word's bytes. A word of 1
//! to 16 bytes, zero-padded to 16, is a key that no other word has
//! ([`key`]); one multiplication of the key picks a slot of [`SLOTS`],
//! which holds the only keyword that can hash there, and one compare of
//! the two keys says whether the word is that keyword. The table is built
//! when the crate is compiled, and the build fails if two keywords share
//! a slot. The few keywords longer than 16 bytes are compared as byte
//! strings, and only for words that long.

use crate::attrs::padded_word;

/// The keywords, `Name = b"text"`, sorted by text.
macro_rules! keywords {
    ($($name:ident = $text:literal,)*) => {
        /// A word the parser looks for, or [`Kw::Other`].
        #[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, Default)]
        #[repr(u8)]
        #[allow(missing_docs)]
        pub enum Kw {
            /// Any other word, and every token that is not a word or an
            /// integer type.
            #[default]
            Other,
            $($name,)*
        }

        /// The keywords by code: `KWS[kw as usize] == kw`.
        const KWS: &[Kw] = &[Kw::Other, $(Kw::$name,)*];

        /// The text of each keyword, by code; empty for [`Kw::Other`].
        const TEXTS: &[&[u8]] = &[b"", $($text,)*];
    };
}

keywords! {
    AcqRel = b"acq_rel",
    Acquire = b"acquire",
    Add = b"add",
    Addrspace = b"addrspace",
    Addrspacecast = b"addrspacecast",
    Afn = b"afn",
    Alias = b"alias",
    Align = b"align",
    Alignstack = b"alignstack",
    Alloca = b"alloca",
    And = b"and",
    Any = b"any",
    Appending = b"appending",
    Arcp = b"arcp",
    Ashr = b"ashr",
    Asm = b"asm",
    Async = b"async",
    Atomic = b"atomic",
    Atomicrmw = b"atomicrmw",
    Attributes = b"attributes",
    AvailableExternally = b"available_externally",
    Bfloat = b"bfloat",
    Bitcast = b"bitcast",
    Blockaddress = b"blockaddress",
    Br = b"br",
    C = b"c",
    Call = b"call",
    Callbr = b"callbr",
    Caller = b"caller",
    Catch = b"catch",
    Catchpad = b"catchpad",
    Catchret = b"catchret",
    Catchswitch = b"catchswitch",
    Cc = b"cc",
    Cleanup = b"cleanup",
    Cleanuppad = b"cleanuppad",
    Cleanupret = b"cleanupret",
    Cmpxchg = b"cmpxchg",
    CodeModel = b"code_model",
    Comdat = b"comdat",
    Common = b"common",
    Constant = b"constant",
    Contract = b"contract",
    Datalayout = b"datalayout",
    Declare = b"declare",
    Default = b"default",
    Define = b"define",
    Dereferenceable = b"dereferenceable",
    Disjoint = b"disjoint",
    Distinct = b"distinct",
    Dllexport = b"dllexport",
    Dllimport = b"dllimport",
    Double = b"double",
    DsoLocal = b"dso_local",
    DsoLocalEquivalent = b"dso_local_equivalent",
    DsoPreemptable = b"dso_preemptable",
    Eq = b"eq",
    Exact = b"exact",
    Exactmatch = b"exactmatch",
    ExternWeak = b"extern_weak",
    External = b"external",
    ExternallyInitialized = b"externally_initialized",
    Extractelement = b"extractelement",
    Extractvalue = b"extractvalue",
    Fadd = b"fadd",
    False = b"false",
    Fast = b"fast",
    Fastcc = b"fastcc",
    Fcmp = b"fcmp",
    Fdiv = b"fdiv",
    Fence = b"fence",
    Filter = b"filter",
    Float = b"float",
    Fmul = b"fmul",
    Fneg = b"fneg",
    Fp128 = b"fp128",
    Fpext = b"fpext",
    Fptosi = b"fptosi",
    Fptoui = b"fptoui",
    Fptrunc = b"fptrunc",
    Freeze = b"freeze",
    Frem = b"frem",
    From = b"from",
    Fsub = b"fsub",
    Gc = b"gc",
    Getelementptr = b"getelementptr",
    Global = b"global",
    Half = b"half",
    Hidden = b"hidden",
    I1 = b"i1",
    I128 = b"i128",
    I16 = b"i16",
    I32 = b"i32",
    I64 = b"i64",
    I8 = b"i8",
    Icmp = b"icmp",
    Ifunc = b"ifunc",
    Inalloca = b"inalloca",
    Inbounds = b"inbounds",
    Indirectbr = b"indirectbr",
    Initialexec = b"initialexec",
    Inrange = b"inrange",
    Insertelement = b"insertelement",
    Insertvalue = b"insertvalue",
    Inteldialect = b"inteldialect",
    Internal = b"internal",
    Inttoptr = b"inttoptr",
    Invoke = b"invoke",
    Kernel = b"kernel",
    Label = b"label",
    Landingpad = b"landingpad",
    Large = b"large",
    Largest = b"largest",
    Linkonce = b"linkonce",
    LinkonceOdr = b"linkonce_odr",
    Load = b"load",
    LocalUnnamedAddr = b"local_unnamed_addr",
    Localdynamic = b"localdynamic",
    Localexec = b"localexec",
    Lshr = b"lshr",
    Medium = b"medium",
    Metadata = b"metadata",
    Module = b"module",
    Monotonic = b"monotonic",
    Mul = b"mul",
    Musttail = b"musttail",
    Ne = b"ne",
    Ninf = b"ninf",
    Nnan = b"nnan",
    Nneg = b"nneg",
    NoCfi = b"no_cfi",
    NoSanitizeAddress = b"no_sanitize_address",
    NoSanitizeHwaddress = b"no_sanitize_hwaddress",
    Nodeduplicate = b"nodeduplicate",
    None = b"none",
    Notail = b"notail",
    Nsw = b"nsw",
    Nsz = b"nsz",
    Null = b"null",
    Nusw = b"nusw",
    Nuw = b"nuw",
    Oeq = b"oeq",
    Oge = b"oge",
    Ogt = b"ogt",
    Ole = b"ole",
    Olt = b"olt",
    One = b"one",
    Opaque = b"opaque",
    Or = b"or",
    Ord = b"ord",
    Partition = b"partition",
    Personality = b"personality",
    Phi = b"phi",
    Poison = b"poison",
    PpcFp128 = b"ppc_fp128",
    Prefix = b"prefix",
    Private = b"private",
    Prologue = b"prologue",
    Protected = b"protected",
    Ptr = b"ptr",
    Ptrauth = b"ptrauth",
    Ptrtoaddr = b"ptrtoaddr",
    Ptrtoint = b"ptrtoint",
    Reassoc = b"reassoc",
    Release = b"release",
    Resume = b"resume",
    Ret = b"ret",
    RiscvVlsCc = b"riscv_vls_cc",
    Samesign = b"samesign",
    Samesize = b"samesize",
    SanitizeAddressDyninit = b"sanitize_address_dyninit",
    SanitizeMemtag = b"sanitize_memtag",
    Sdiv = b"sdiv",
    Section = b"section",
    Select = b"select",
    SeqCst = b"seq_cst",
    Sext = b"sext",
    Sge = b"sge",
    Sgt = b"sgt",
    Shl = b"shl",
    Shufflevector = b"shufflevector",
    Sideeffect = b"sideeffect",
    Singlethread = b"singlethread",
    Sitofp = b"sitofp",
    Sle = b"sle",
    Slt = b"slt",
    Small = b"small",
    SourceFilename = b"source_filename",
    Splat = b"splat",
    Srem = b"srem",
    Store = b"store",
    Sub = b"sub",
    Swifterror = b"swifterror",
    Switch = b"switch",
    Sync = b"sync",
    Syncscope = b"syncscope",
    Tail = b"tail",
    Target = b"target",
    ThreadLocal = b"thread_local",
    Tiny = b"tiny",
    To = b"to",
    Token = b"token",
    Triple = b"triple",
    True = b"true",
    Trunc = b"trunc",
    Type = b"type",
    Udiv = b"udiv",
    Ueq = b"ueq",
    Uge = b"uge",
    Ugt = b"ugt",
    Uitofp = b"uitofp",
    Ule = b"ule",
    Ult = b"ult",
    Undef = b"undef",
    Une = b"une",
    UnnamedAddr = b"unnamed_addr",
    Uno = b"uno",
    Unordered = b"unordered",
    Unreachable = b"unreachable",
    Unwind = b"unwind",
    Urem = b"urem",
    Uselistorder = b"uselistorder",
    UselistorderBb = b"uselistorder_bb",
    Uwtable = b"uwtable",
    VaArg = b"va_arg",
    Void = b"void",
    Volatile = b"volatile",
    Vscale = b"vscale",
    Weak = b"weak",
    WeakOdr = b"weak_odr",
    Within = b"within",
    X = b"x",
    X86Amx = b"x86_amx",
    X86Fp80 = b"x86_fp80",
    X86Mmx = b"x86_mmx",
    Xor = b"xor",
    Zeroinitializer = b"zeroinitializer",
    Zext = b"zext",
}

impl Kw {
    /// The keyword's text; empty for [`Kw::Other`].
    pub fn text(self) -> &'static [u8] {
        TEXTS[self as usize]
    }

    /// Whether it is the code of an integer type (`i32`), which only a
    /// [`Tok::IntType`](crate::Tok::IntType) token carries.
    pub fn is_int_type(self) -> bool {
        matches!(self, Kw::I1 | Kw::I8 | Kw::I16 | Kw::I32 | Kw::I64 | Kw::I128)
    }
}

/// The multiplier of the hash: found by a search (`find_a_multiplier`,
/// an ignored test) for one under which no two keywords share a slot.
const MULTIPLIER: u64 = 0x4c86_7062_2d9b_8ebf;

/// Slots of the table: `1 << SLOT_BITS`.
const SLOT_BITS: u32 = 12;

/// The longest keyword the table holds.
const MAX_HASHED: usize = 16;

/// The key of a word of 1 to 16 bytes, none of them zero: its bytes
/// zero-padded to 16, as two little-endian words. Two such words have the
/// same key only if they are the same word, and no key is `[0, 0]`.
#[inline]
fn key(word: &[u8]) -> Key {
    let len = word.len();
    debug_assert!((1..=MAX_HASHED).contains(&len) && !word.contains(&0));
    if len <= 8 {
        return [padded_word(word), 0];
    }
    let (first, rest) = word.split_at(8);
    [padded_word(first), padded_word(rest)]
}

/// A word's bytes, zero-padded to 16: the first eight and the next eight.
type Key = [u64; 2];

/// The slot of a key.
#[inline]
const fn slot(key: Key) -> usize {
    let mixed = key[0] ^ key[1].rotate_left(23);
    (mixed.wrapping_mul(MULTIPLIER) >> (64 - SLOT_BITS)) as usize
}

/// [`key`] at compile time.
const fn const_key(word: &[u8]) -> Key {
    let mut key = [0u64; 2];
    let mut at = 0;
    while at < word.len() {
        key[at / 8] |= (word[at] as u64) << (8 * (at % 8));
        at += 1;
    }
    key
}

/// The code of the keyword in each slot; 0 ([`Kw::Other`]) where there
/// is none.
static SLOTS: [u8; 1 << SLOT_BITS] = {
    let mut slots = [0u8; 1 << SLOT_BITS];
    let mut code = 1;
    while code < TEXTS.len() {
        let text = TEXTS[code];
        if text.len() <= MAX_HASHED {
            let at = slot(const_key(text));
            assert!(slots[at] == 0, "two keywords share a slot: find another MULTIPLIER");
            slots[at] = code as u8;
        }
        code += 1;
    }
    slots
};

/// The key of each keyword, by code; `[0, 0]` for [`Kw::Other`] and for
/// the keywords longer than [`MAX_HASHED`], which no word's key equals.
static KEYS: [Key; TEXTS.len()] = {
    let mut keys = [[0u64; 2]; TEXTS.len()];
    let mut code = 1;
    while code < TEXTS.len() {
        if TEXTS[code].len() <= MAX_HASHED {
            keys[code] = const_key(TEXTS[code]);
        }
        code += 1;
    }
    keys
};

/// The code of the word `src[start..end]`, a word token's or an integer
/// type's bytes (never empty, no zero byte).
#[allow(clippy::inline_always)] // Measured: a call per word otherwise (WP-168).
#[inline(always)]
pub(crate) fn keyword_at(src: &[u8], start: usize, end: usize) -> Kw {
    let len = end - start;
    // The common case: sixteen bytes from the word's start are in the
    // source, two loads, and the bytes past the word are masked off.
    if len <= MAX_HASHED
        && let Some(bytes) = src.get(start..).and_then(<[u8]>::first_chunk::<16>)
    {
        let (first, rest) = bytes.split_at(8);
        let lo = u64::from_le_bytes(first.try_into().unwrap_or_default());
        let hi = u64::from_le_bytes(rest.try_into().unwrap_or_default());
        let key = if len <= 8 {
            [lo & (u64::MAX >> (64 - 8 * len)), 0]
        } else {
            [lo, hi & (u64::MAX >> (128 - 8 * len))]
        };
        return by_key(key);
    }
    keyword(&src[start..end])
}

/// The code of `word` (never empty, no zero byte).
#[cold]
pub(crate) fn keyword(word: &[u8]) -> Kw {
    if word.len() > MAX_HASHED {
        return TEXTS.iter().position(|&text| text == word).map_or(Kw::Other, |code| KWS[code]);
    }
    by_key(key(word))
}

/// The code of the word whose key is `key`.
#[inline]
fn by_key(key: Key) -> Kw {
    let code = SLOTS[slot(key)] as usize;
    let known = KEYS[code];
    if (known[0] ^ key[0]) | (known[1] ^ key[1]) == 0 { KWS[code] } else { Kw::Other }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every keyword is found as itself, and in its own slot.
    #[test]
    fn every_keyword_maps_to_itself() {
        assert_eq!(KWS.len(), TEXTS.len());
        assert!(TEXTS.len() <= 256, "a code is one byte");
        for (code, &text) in TEXTS.iter().enumerate().skip(1) {
            assert_eq!(KWS[code] as usize, code);
            assert_eq!(keyword(text), KWS[code], "{}", String::from_utf8_lossy(text));
            assert_eq!(KWS[code].text(), text);
            if text.len() <= MAX_HASHED {
                assert_eq!(key(text), const_key(text));
                assert_eq!(SLOTS[slot(key(text))] as usize, code);
            }
        }
        let used = SLOTS.iter().filter(|&&code| code != 0).count();
        let hashed = TEXTS.iter().skip(1).filter(|text| text.len() <= MAX_HASHED).count();
        assert_eq!(used, hashed, "each hashed keyword has a slot of its own");
        assert!(TEXTS.windows(2).skip(1).all(|pair| pair[0] < pair[1]), "sorted, no repeats");
    }

    /// The one-load path and the general one agree, for a word with
    /// sixteen bytes after its start and for one at the end of the
    /// source.
    #[test]
    fn one_load_and_general_agree() {
        for &text in TEXTS.iter().skip(1).chain([&b"x86"[..], b"zeroinitializers"].iter()) {
            let mut src = b"  ".to_vec();
            src.extend_from_slice(text);
            let end = src.len();
            let expect = keyword(text);
            assert_eq!(keyword_at(&src, 2, end), expect, "at the end");
            src.extend_from_slice(b" ptr, align 8 x y z");
            assert_eq!(keyword_at(&src, 2, end), expect, "followed by more");
        }
    }

    /// A word that hashes to a keyword's slot, of the keyword's length,
    /// is not that keyword: the final compare of the keys rejects it.
    #[test]
    fn a_word_in_a_keywords_slot_is_rejected() {
        let mut found = 0;
        for &text in TEXTS.iter().skip(1).filter(|text| text.len() <= MAX_HASHED) {
            let target = slot(key(text));
            // Change the last byte, then the last two, until a word of
            // the same length lands in the same slot.
            let mut word = text.to_vec();
            let last = word.len() - 1;
            'search: for a in b'a'..=b'z' {
                for b in b'a'..=b'z' {
                    word[last] = b;
                    if last > 0 {
                        word[last - 1] = a;
                    }
                    if word != text && slot(key(&word)) == target {
                        assert_eq!(keyword(&word), Kw::Other, "{}", String::from_utf8_lossy(&word));
                        found += 1;
                        break 'search;
                    }
                }
            }
        }
        assert!(found > 0, "no word of a keyword's length shares its slot");
        // Words that are no keyword, of every length.
        for word in
            [&b"x86"[..], b"ptrs", b"calls", b"i33", b"zeroinitializers", b"externally_initialised"]
        {
            assert_eq!(keyword(word), Kw::Other, "{}", String::from_utf8_lossy(word));
        }
    }

    /// Finds a multiplier under which no two keywords share a slot, for
    /// when a keyword is added: `cargo test -p hod_ll find_a_multiplier
    /// -- --ignored --nocapture`.
    #[test]
    #[ignore = "a search, run by hand"]
    fn find_a_multiplier() {
        let keys: Vec<Key> =
            TEXTS.iter().skip(1).filter(|t| t.len() <= MAX_HASHED).map(|t| key(t)).collect();
        let mut state = 0x9E37_79B9_7F4A_7C15u64;
        for _ in 0..10_000_000 {
            // xorshift64
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            let multiplier = state | 1;
            let mut seen = vec![false; 1 << SLOT_BITS];
            let fits = keys.iter().all(|&key| {
                let mixed = key[0] ^ key[1].rotate_left(23);
                let at = (mixed.wrapping_mul(multiplier) >> (64 - SLOT_BITS)) as usize;
                !std::mem::replace(&mut seen[at], true)
            });
            if fits {
                println!("MULTIPLIER = {multiplier:#x}");
                return;
            }
        }
        panic!("no multiplier found");
    }
}
