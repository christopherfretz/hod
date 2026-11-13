//! Intrinsics (`spec/ir.md` §5.5): the closed list hod knows, the one
//! mapping from an LLVM name and function type to a member of it, and the
//! rule for the intrinsics that generate no code.
//!
//! The list is the non-target-specific intrinsics that kiln's lowering can
//! name and that `clang++` 22 emits at `-O0` over a spread of C++ (the
//! survey is recorded in `spec/landed/ir/WP-004.md`). Being on the list
//! says only that the name is recognised: whether a target lowers it is
//! `classify`'s decision.

use crate::ids::TypeId;
use crate::intern::Layering;
use crate::types::{TypeKind, TypeTable};

/// Where an overload type of an intrinsic is read from its function type.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Ov {
    /// The return type.
    Ret,
    /// Parameter `n`.
    P(u8),
    /// Field `n` of the returned struct.
    RetField(u8),
}

use Ov::{P, Ret, RetField};

macro_rules! intrinsics {
    ($($variant:ident = $name:literal [$($ov:expr),*],)+) => {
        /// The callee of an intrinsic call (§5.5).
        ///
        /// Each member but the last is one LLVM intrinsic, whatever its
        /// overload types; those are separate operands of the call
        /// ([`crate::CallSite::overloads`]). `Other` is any `llvm.*` name not
        /// on the list, kept so that `classify` can reject it by name.
        #[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
        pub enum Intrinsic<'a> {
            $(#[doc = concat!("`", $name, "`.")] $variant,)+
            /// An intrinsic hod does not know, by its full name.
            Other(&'a str),
        }

        #[repr(u16)]
        enum Known {
            $($variant,)+
        }

        /// Base name and overload sources, parallel to [`Intrinsic::ALL`].
        const TABLE: &[(&str, &[Ov])] = &[$(($name, &[$($ov),*]),)+];

        impl Intrinsic<'_> {
            /// Every known intrinsic.
            pub const ALL: &'static [Intrinsic<'static>] = &[$(Intrinsic::$variant,)+];

            /// The position in [`Intrinsic::ALL`]; `None` for `Other`.
            fn index(self) -> Option<usize> {
                match self {
                    $(Intrinsic::$variant => Some(Known::$variant as usize),)+
                    Intrinsic::Other(_) => None,
                }
            }
        }
    };
}

intrinsics! {
    // Memory.
    Memcpy = "llvm.memcpy" [P(0), P(1), P(2)],
    MemcpyInline = "llvm.memcpy.inline" [P(0), P(1), P(2)],
    Memmove = "llvm.memmove" [P(0), P(1), P(2)],
    Memset = "llvm.memset" [P(0), P(2)],
    MemsetInline = "llvm.memset.inline" [P(0), P(2)],
    // Variable arguments.
    VaStart = "llvm.va_start" [P(0)],
    VaEnd = "llvm.va_end" [P(0)],
    VaCopy = "llvm.va_copy" [P(0)],
    // The stack and the frame.
    StackSave = "llvm.stacksave" [Ret],
    StackRestore = "llvm.stackrestore" [P(0)],
    FrameAddress = "llvm.frameaddress" [Ret],
    ReturnAddress = "llvm.returnaddress" [],
    // Traps.
    Trap = "llvm.trap" [],
    DebugTrap = "llvm.debugtrap" [],
    UbsanTrap = "llvm.ubsantrap" [],
    // Integer bit operations.
    Bswap = "llvm.bswap" [Ret],
    BitReverse = "llvm.bitreverse" [Ret],
    Ctpop = "llvm.ctpop" [Ret],
    Ctlz = "llvm.ctlz" [Ret],
    Cttz = "llvm.cttz" [Ret],
    Fshl = "llvm.fshl" [Ret],
    Fshr = "llvm.fshr" [Ret],
    Abs = "llvm.abs" [Ret],
    Smax = "llvm.smax" [Ret],
    Smin = "llvm.smin" [Ret],
    Umax = "llvm.umax" [Ret],
    Umin = "llvm.umin" [Ret],
    Scmp = "llvm.scmp" [Ret, P(0)],
    Ucmp = "llvm.ucmp" [Ret, P(0)],
    Clmul = "llvm.clmul" [Ret],
    // Checked and saturating arithmetic.
    SaddWithOverflow = "llvm.sadd.with.overflow" [P(0)],
    UaddWithOverflow = "llvm.uadd.with.overflow" [P(0)],
    SsubWithOverflow = "llvm.ssub.with.overflow" [P(0)],
    UsubWithOverflow = "llvm.usub.with.overflow" [P(0)],
    SmulWithOverflow = "llvm.smul.with.overflow" [P(0)],
    UmulWithOverflow = "llvm.umul.with.overflow" [P(0)],
    SaddSat = "llvm.sadd.sat" [Ret],
    UaddSat = "llvm.uadd.sat" [Ret],
    SsubSat = "llvm.ssub.sat" [Ret],
    UsubSat = "llvm.usub.sat" [Ret],
    // Floating point.
    Fabs = "llvm.fabs" [Ret],
    Sqrt = "llvm.sqrt" [Ret],
    Floor = "llvm.floor" [Ret],
    Ceil = "llvm.ceil" [Ret],
    Trunc = "llvm.trunc" [Ret],
    Round = "llvm.round" [Ret],
    RoundEven = "llvm.roundeven" [Ret],
    NearbyInt = "llvm.nearbyint" [Ret],
    Rint = "llvm.rint" [Ret],
    CopySign = "llvm.copysign" [Ret],
    Fma = "llvm.fma" [Ret],
    FmulAdd = "llvm.fmuladd" [Ret],
    MinNum = "llvm.minnum" [Ret],
    MaxNum = "llvm.maxnum" [Ret],
    Minimum = "llvm.minimum" [Ret],
    Maximum = "llvm.maximum" [Ret],
    MinimumNum = "llvm.minimumnum" [Ret],
    MaximumNum = "llvm.maximumnum" [Ret],
    Canonicalize = "llvm.canonicalize" [Ret],
    ArithmeticFence = "llvm.arithmetic.fence" [Ret],
    Pow = "llvm.pow" [Ret],
    Powi = "llvm.powi" [Ret, P(1)],
    Ldexp = "llvm.ldexp" [Ret, P(1)],
    Frexp = "llvm.frexp" [RetField(0), RetField(1)],
    Sin = "llvm.sin" [Ret],
    Cos = "llvm.cos" [Ret],
    Tan = "llvm.tan" [Ret],
    Asin = "llvm.asin" [Ret],
    Acos = "llvm.acos" [Ret],
    Atan = "llvm.atan" [Ret],
    Atan2 = "llvm.atan2" [Ret],
    Sinh = "llvm.sinh" [Ret],
    Cosh = "llvm.cosh" [Ret],
    Tanh = "llvm.tanh" [Ret],
    Exp = "llvm.exp" [Ret],
    Exp2 = "llvm.exp2" [Ret],
    Exp10 = "llvm.exp10" [Ret],
    Log = "llvm.log" [Ret],
    Log2 = "llvm.log2" [Ret],
    Log10 = "llvm.log10" [Ret],
    Lround = "llvm.lround" [Ret, P(0)],
    Llround = "llvm.llround" [Ret, P(0)],
    Lrint = "llvm.lrint" [Ret, P(0)],
    Llrint = "llvm.llrint" [Ret, P(0)],
    FptosiSat = "llvm.fptosi.sat" [Ret, P(0)],
    FptouiSat = "llvm.fptoui.sat" [Ret, P(0)],
    IsFpClass = "llvm.is.fpclass" [P(0)],
    // Exceptions and thread-local storage.
    EhTypeidFor = "llvm.eh.typeid.for" [P(0)],
    ThreadLocalAddress = "llvm.threadlocal.address" [Ret],
    ThreadPointer = "llvm.thread.pointer" [Ret],
    // Pointers, caches, counters.
    Prefetch = "llvm.prefetch" [P(0)],
    ClearCache = "llvm.clear_cache" [],
    ReadCycleCounter = "llvm.readcyclecounter" [],
    IsConstant = "llvm.is.constant" [P(0)],
    ObjectSize = "llvm.objectsize" [Ret, P(0)],
    Ptrmask = "llvm.ptrmask" [Ret, P(1)],
    LoadRelative = "llvm.load.relative" [P(1)],
    InstrprofIncrement = "llvm.instrprof.increment" [],
    InstrprofIncrementStep = "llvm.instrprof.increment.step" [],
    // Vectors.
    VectorReduceAdd = "llvm.vector.reduce.add" [P(0)],
    VectorReduceMul = "llvm.vector.reduce.mul" [P(0)],
    VectorReduceAnd = "llvm.vector.reduce.and" [P(0)],
    VectorReduceOr = "llvm.vector.reduce.or" [P(0)],
    VectorReduceXor = "llvm.vector.reduce.xor" [P(0)],
    VectorReduceSmax = "llvm.vector.reduce.smax" [P(0)],
    VectorReduceSmin = "llvm.vector.reduce.smin" [P(0)],
    VectorReduceUmax = "llvm.vector.reduce.umax" [P(0)],
    VectorReduceUmin = "llvm.vector.reduce.umin" [P(0)],
    VectorReduceFmax = "llvm.vector.reduce.fmax" [P(0)],
    VectorReduceFmin = "llvm.vector.reduce.fmin" [P(0)],
    VectorReduceFadd = "llvm.vector.reduce.fadd" [P(1)],
    VectorReduceFmul = "llvm.vector.reduce.fmul" [P(1)],
    VectorExtract = "llvm.vector.extract" [Ret, P(0)],
    VectorInsert = "llvm.vector.insert" [Ret, P(1)],
    // Coroutines (lowered by `hod_xform`, never by a target).
    CoroId = "llvm.coro.id" [],
    CoroAlloc = "llvm.coro.alloc" [],
    CoroBegin = "llvm.coro.begin" [],
    CoroEnd = "llvm.coro.end" [],
    CoroFree = "llvm.coro.free" [],
    CoroSize = "llvm.coro.size" [Ret],
    CoroAlign = "llvm.coro.align" [Ret],
    CoroSave = "llvm.coro.save" [],
    CoroSuspend = "llvm.coro.suspend" [],
    CoroResume = "llvm.coro.resume" [],
    CoroDestroy = "llvm.coro.destroy" [],
    CoroDone = "llvm.coro.done" [],
    CoroPromise = "llvm.coro.promise" [],
    CoroFrame = "llvm.coro.frame" [],
    CoroNoop = "llvm.coro.noop" [],
    CoroIsInRamp = "llvm.coro.is_in_ramp" [],
    CoroAwaitSuspendVoid = "llvm.coro.await.suspend.void" [],
    CoroAwaitSuspendBool = "llvm.coro.await.suspend.bool" [],
    CoroAwaitSuspendHandle = "llvm.coro.await.suspend.handle" [],
    // Saturating shifts (WP-084).
    SshlSat = "llvm.sshl.sat" [Ret],
    UshlSat = "llvm.ushl.sat" [Ret],
    // AArch64 CRC32 (FEAT_CRC32, WP-084): not overloaded.
    Aarch64Crc32b = "llvm.aarch64.crc32b" [],
    Aarch64Crc32h = "llvm.aarch64.crc32h" [],
    Aarch64Crc32w = "llvm.aarch64.crc32w" [],
    Aarch64Crc32x = "llvm.aarch64.crc32x" [],
    Aarch64Crc32cb = "llvm.aarch64.crc32cb" [],
    Aarch64Crc32ch = "llvm.aarch64.crc32ch" [],
    Aarch64Crc32cw = "llvm.aarch64.crc32cw" [],
    Aarch64Crc32cx = "llvm.aarch64.crc32cx" [],
    // AArch64 target intrinsics (WP-083): the NEON and crypto names
    // `<arm_neon.h>` of clang 19 and 22 and kiln emit, the scalar `cls`,
    // and the barriers and hints of `<arm_acle.h>`.
    Aarch64Cls = "llvm.aarch64.cls" [],
    Aarch64Cls64 = "llvm.aarch64.cls64" [],
    Aarch64CryptoAesd = "llvm.aarch64.crypto.aesd" [],
    Aarch64CryptoAese = "llvm.aarch64.crypto.aese" [],
    Aarch64CryptoAesimc = "llvm.aarch64.crypto.aesimc" [],
    Aarch64CryptoAesmc = "llvm.aarch64.crypto.aesmc" [],
    Aarch64CryptoBcaxs = "llvm.aarch64.crypto.bcaxs" [Ret],
    Aarch64CryptoBcaxu = "llvm.aarch64.crypto.bcaxu" [Ret],
    Aarch64CryptoEor3s = "llvm.aarch64.crypto.eor3s" [Ret],
    Aarch64CryptoEor3u = "llvm.aarch64.crypto.eor3u" [Ret],
    Aarch64CryptoRax1 = "llvm.aarch64.crypto.rax1" [],
    Aarch64CryptoSha1c = "llvm.aarch64.crypto.sha1c" [],
    Aarch64CryptoSha1h = "llvm.aarch64.crypto.sha1h" [],
    Aarch64CryptoSha1m = "llvm.aarch64.crypto.sha1m" [],
    Aarch64CryptoSha1p = "llvm.aarch64.crypto.sha1p" [],
    Aarch64CryptoSha1su0 = "llvm.aarch64.crypto.sha1su0" [],
    Aarch64CryptoSha1su1 = "llvm.aarch64.crypto.sha1su1" [],
    Aarch64CryptoSha256h = "llvm.aarch64.crypto.sha256h" [],
    Aarch64CryptoSha256h2 = "llvm.aarch64.crypto.sha256h2" [],
    Aarch64CryptoSha256su0 = "llvm.aarch64.crypto.sha256su0" [],
    Aarch64CryptoSha256su1 = "llvm.aarch64.crypto.sha256su1" [],
    Aarch64CryptoSha512h = "llvm.aarch64.crypto.sha512h" [],
    Aarch64CryptoSha512h2 = "llvm.aarch64.crypto.sha512h2" [],
    Aarch64CryptoSha512su0 = "llvm.aarch64.crypto.sha512su0" [],
    Aarch64CryptoSha512su1 = "llvm.aarch64.crypto.sha512su1" [],
    Aarch64CryptoXar = "llvm.aarch64.crypto.xar" [],
    Aarch64Dmb = "llvm.aarch64.dmb" [],
    Aarch64Dsb = "llvm.aarch64.dsb" [],
    Aarch64Hint = "llvm.aarch64.hint" [],
    Aarch64Isb = "llvm.aarch64.isb" [],
    Aarch64NeonAbs = "llvm.aarch64.neon.abs" [Ret],
    Aarch64NeonAddp = "llvm.aarch64.neon.addp" [Ret],
    Aarch64NeonBfcvt = "llvm.aarch64.neon.bfcvt" [],
    Aarch64NeonBfcvtn = "llvm.aarch64.neon.bfcvtn" [],
    Aarch64NeonBfcvtn2 = "llvm.aarch64.neon.bfcvtn2" [],
    Aarch64NeonBfdot = "llvm.aarch64.neon.bfdot" [Ret, P(1)],
    Aarch64NeonBfmlalb = "llvm.aarch64.neon.bfmlalb" [],
    Aarch64NeonBfmlalt = "llvm.aarch64.neon.bfmlalt" [],
    Aarch64NeonBfmmla = "llvm.aarch64.neon.bfmmla" [],
    Aarch64NeonCls = "llvm.aarch64.neon.cls" [Ret],
    Aarch64NeonFabd = "llvm.aarch64.neon.fabd" [Ret],
    Aarch64NeonFacge = "llvm.aarch64.neon.facge" [Ret, P(0)],
    Aarch64NeonFacgt = "llvm.aarch64.neon.facgt" [Ret, P(0)],
    Aarch64NeonFaddp = "llvm.aarch64.neon.faddp" [Ret],
    Aarch64NeonFaddv = "llvm.aarch64.neon.faddv" [Ret, P(0)],
    Aarch64NeonFcvtas = "llvm.aarch64.neon.fcvtas" [Ret, P(0)],
    Aarch64NeonFcvtau = "llvm.aarch64.neon.fcvtau" [Ret, P(0)],
    Aarch64NeonFcvtms = "llvm.aarch64.neon.fcvtms" [Ret, P(0)],
    Aarch64NeonFcvtmu = "llvm.aarch64.neon.fcvtmu" [Ret, P(0)],
    Aarch64NeonFcvtns = "llvm.aarch64.neon.fcvtns" [Ret, P(0)],
    Aarch64NeonFcvtnu = "llvm.aarch64.neon.fcvtnu" [Ret, P(0)],
    Aarch64NeonFcvtps = "llvm.aarch64.neon.fcvtps" [Ret, P(0)],
    Aarch64NeonFcvtpu = "llvm.aarch64.neon.fcvtpu" [Ret, P(0)],
    Aarch64NeonFcvtxn = "llvm.aarch64.neon.fcvtxn" [Ret, P(0)],
    Aarch64NeonFcvtzs = "llvm.aarch64.neon.fcvtzs" [Ret, P(0)],
    Aarch64NeonFcvtzu = "llvm.aarch64.neon.fcvtzu" [Ret, P(0)],
    Aarch64NeonFmax = "llvm.aarch64.neon.fmax" [Ret],
    Aarch64NeonFmaxnm = "llvm.aarch64.neon.fmaxnm" [Ret],
    Aarch64NeonFmaxnmp = "llvm.aarch64.neon.fmaxnmp" [Ret],
    Aarch64NeonFmaxnmv = "llvm.aarch64.neon.fmaxnmv" [Ret, P(0)],
    Aarch64NeonFmaxp = "llvm.aarch64.neon.fmaxp" [Ret],
    Aarch64NeonFmaxv = "llvm.aarch64.neon.fmaxv" [Ret, P(0)],
    Aarch64NeonFmin = "llvm.aarch64.neon.fmin" [Ret],
    Aarch64NeonFminnm = "llvm.aarch64.neon.fminnm" [Ret],
    Aarch64NeonFminnmp = "llvm.aarch64.neon.fminnmp" [Ret],
    Aarch64NeonFminnmv = "llvm.aarch64.neon.fminnmv" [Ret, P(0)],
    Aarch64NeonFminp = "llvm.aarch64.neon.fminp" [Ret],
    Aarch64NeonFminv = "llvm.aarch64.neon.fminv" [Ret, P(0)],
    Aarch64NeonFmlal = "llvm.aarch64.neon.fmlal" [Ret, P(1)],
    Aarch64NeonFmlal2 = "llvm.aarch64.neon.fmlal2" [Ret, P(1)],
    Aarch64NeonFmlsl = "llvm.aarch64.neon.fmlsl" [Ret, P(1)],
    Aarch64NeonFmlsl2 = "llvm.aarch64.neon.fmlsl2" [Ret, P(1)],
    Aarch64NeonFmulx = "llvm.aarch64.neon.fmulx" [Ret],
    Aarch64NeonFrecpe = "llvm.aarch64.neon.frecpe" [Ret],
    Aarch64NeonFrecps = "llvm.aarch64.neon.frecps" [Ret],
    Aarch64NeonFrecpx = "llvm.aarch64.neon.frecpx" [Ret],
    Aarch64NeonFrint32x = "llvm.aarch64.neon.frint32x" [Ret],
    Aarch64NeonFrint32z = "llvm.aarch64.neon.frint32z" [Ret],
    Aarch64NeonFrint64x = "llvm.aarch64.neon.frint64x" [Ret],
    Aarch64NeonFrint64z = "llvm.aarch64.neon.frint64z" [Ret],
    Aarch64NeonFrsqrte = "llvm.aarch64.neon.frsqrte" [Ret],
    Aarch64NeonFrsqrts = "llvm.aarch64.neon.frsqrts" [Ret],
    Aarch64NeonLd1x2 = "llvm.aarch64.neon.ld1x2" [RetField(0), P(0)],
    Aarch64NeonLd1x3 = "llvm.aarch64.neon.ld1x3" [RetField(0), P(0)],
    Aarch64NeonLd1x4 = "llvm.aarch64.neon.ld1x4" [RetField(0), P(0)],
    Aarch64NeonLd2 = "llvm.aarch64.neon.ld2" [RetField(0), P(0)],
    Aarch64NeonLd2lane = "llvm.aarch64.neon.ld2lane" [RetField(0), P(3)],
    Aarch64NeonLd2r = "llvm.aarch64.neon.ld2r" [RetField(0), P(0)],
    Aarch64NeonLd3 = "llvm.aarch64.neon.ld3" [RetField(0), P(0)],
    Aarch64NeonLd3lane = "llvm.aarch64.neon.ld3lane" [RetField(0), P(4)],
    Aarch64NeonLd3r = "llvm.aarch64.neon.ld3r" [RetField(0), P(0)],
    Aarch64NeonLd4 = "llvm.aarch64.neon.ld4" [RetField(0), P(0)],
    Aarch64NeonLd4lane = "llvm.aarch64.neon.ld4lane" [RetField(0), P(5)],
    Aarch64NeonLd4r = "llvm.aarch64.neon.ld4r" [RetField(0), P(0)],
    Aarch64NeonPmul = "llvm.aarch64.neon.pmul" [Ret],
    Aarch64NeonPmull = "llvm.aarch64.neon.pmull" [Ret],
    Aarch64NeonPmull64 = "llvm.aarch64.neon.pmull64" [],
    Aarch64NeonRaddhn = "llvm.aarch64.neon.raddhn" [Ret],
    Aarch64NeonRshrn = "llvm.aarch64.neon.rshrn" [Ret],
    Aarch64NeonRsubhn = "llvm.aarch64.neon.rsubhn" [Ret],
    Aarch64NeonSabd = "llvm.aarch64.neon.sabd" [Ret],
    Aarch64NeonSaddlp = "llvm.aarch64.neon.saddlp" [Ret, P(0)],
    Aarch64NeonSaddlv = "llvm.aarch64.neon.saddlv" [Ret, P(0)],
    Aarch64NeonSaddv = "llvm.aarch64.neon.saddv" [Ret, P(0)],
    Aarch64NeonScalarSqxtn = "llvm.aarch64.neon.scalar.sqxtn" [Ret, P(0)],
    Aarch64NeonScalarSqxtun = "llvm.aarch64.neon.scalar.sqxtun" [Ret, P(0)],
    Aarch64NeonScalarUqxtn = "llvm.aarch64.neon.scalar.uqxtn" [Ret, P(0)],
    Aarch64NeonSdot = "llvm.aarch64.neon.sdot" [Ret, P(1)],
    Aarch64NeonShadd = "llvm.aarch64.neon.shadd" [Ret],
    Aarch64NeonShsub = "llvm.aarch64.neon.shsub" [Ret],
    Aarch64NeonSmax = "llvm.aarch64.neon.smax" [Ret],
    Aarch64NeonSmaxp = "llvm.aarch64.neon.smaxp" [Ret],
    Aarch64NeonSmaxv = "llvm.aarch64.neon.smaxv" [Ret, P(0)],
    Aarch64NeonSmin = "llvm.aarch64.neon.smin" [Ret],
    Aarch64NeonSminp = "llvm.aarch64.neon.sminp" [Ret],
    Aarch64NeonSminv = "llvm.aarch64.neon.sminv" [Ret, P(0)],
    Aarch64NeonSmmla = "llvm.aarch64.neon.smmla" [Ret, P(1)],
    Aarch64NeonSmull = "llvm.aarch64.neon.smull" [Ret],
    Aarch64NeonSqabs = "llvm.aarch64.neon.sqabs" [Ret],
    Aarch64NeonSqadd = "llvm.aarch64.neon.sqadd" [Ret],
    Aarch64NeonSqdmulh = "llvm.aarch64.neon.sqdmulh" [Ret],
    Aarch64NeonSqdmulhLane = "llvm.aarch64.neon.sqdmulh.lane" [Ret, P(1)],
    Aarch64NeonSqdmulhLaneq = "llvm.aarch64.neon.sqdmulh.laneq" [Ret, P(1)],
    Aarch64NeonSqdmull = "llvm.aarch64.neon.sqdmull" [Ret],
    Aarch64NeonSqdmullsScalar = "llvm.aarch64.neon.sqdmulls.scalar" [],
    Aarch64NeonSqneg = "llvm.aarch64.neon.sqneg" [Ret],
    Aarch64NeonSqrdmlah = "llvm.aarch64.neon.sqrdmlah" [Ret],
    Aarch64NeonSqrdmlsh = "llvm.aarch64.neon.sqrdmlsh" [Ret],
    Aarch64NeonSqrdmulh = "llvm.aarch64.neon.sqrdmulh" [Ret],
    Aarch64NeonSqrdmulhLane = "llvm.aarch64.neon.sqrdmulh.lane" [Ret, P(1)],
    Aarch64NeonSqrdmulhLaneq = "llvm.aarch64.neon.sqrdmulh.laneq" [Ret, P(1)],
    Aarch64NeonSqrshl = "llvm.aarch64.neon.sqrshl" [Ret],
    Aarch64NeonSqrshrn = "llvm.aarch64.neon.sqrshrn" [Ret],
    Aarch64NeonSqrshrun = "llvm.aarch64.neon.sqrshrun" [Ret],
    Aarch64NeonSqshl = "llvm.aarch64.neon.sqshl" [Ret],
    Aarch64NeonSqshlu = "llvm.aarch64.neon.sqshlu" [Ret],
    Aarch64NeonSqshrn = "llvm.aarch64.neon.sqshrn" [Ret],
    Aarch64NeonSqshrun = "llvm.aarch64.neon.sqshrun" [Ret],
    Aarch64NeonSqsub = "llvm.aarch64.neon.sqsub" [Ret],
    Aarch64NeonSqxtn = "llvm.aarch64.neon.sqxtn" [Ret],
    Aarch64NeonSqxtun = "llvm.aarch64.neon.sqxtun" [Ret],
    Aarch64NeonSrhadd = "llvm.aarch64.neon.srhadd" [Ret],
    Aarch64NeonSrshl = "llvm.aarch64.neon.srshl" [Ret],
    Aarch64NeonSshl = "llvm.aarch64.neon.sshl" [Ret],
    Aarch64NeonSt1x2 = "llvm.aarch64.neon.st1x2" [P(0), P(2)],
    Aarch64NeonSt1x3 = "llvm.aarch64.neon.st1x3" [P(0), P(3)],
    Aarch64NeonSt1x4 = "llvm.aarch64.neon.st1x4" [P(0), P(4)],
    Aarch64NeonSt2 = "llvm.aarch64.neon.st2" [P(0), P(2)],
    Aarch64NeonSt2lane = "llvm.aarch64.neon.st2lane" [P(0), P(3)],
    Aarch64NeonSt3 = "llvm.aarch64.neon.st3" [P(0), P(3)],
    Aarch64NeonSt3lane = "llvm.aarch64.neon.st3lane" [P(0), P(4)],
    Aarch64NeonSt4 = "llvm.aarch64.neon.st4" [P(0), P(4)],
    Aarch64NeonSt4lane = "llvm.aarch64.neon.st4lane" [P(0), P(5)],
    Aarch64NeonSuqadd = "llvm.aarch64.neon.suqadd" [Ret],
    Aarch64NeonTbl1 = "llvm.aarch64.neon.tbl1" [Ret],
    Aarch64NeonTbl2 = "llvm.aarch64.neon.tbl2" [Ret],
    Aarch64NeonTbl3 = "llvm.aarch64.neon.tbl3" [Ret],
    Aarch64NeonTbl4 = "llvm.aarch64.neon.tbl4" [Ret],
    Aarch64NeonTbx1 = "llvm.aarch64.neon.tbx1" [Ret],
    Aarch64NeonTbx2 = "llvm.aarch64.neon.tbx2" [Ret],
    Aarch64NeonTbx3 = "llvm.aarch64.neon.tbx3" [Ret],
    Aarch64NeonTbx4 = "llvm.aarch64.neon.tbx4" [Ret],
    Aarch64NeonUabd = "llvm.aarch64.neon.uabd" [Ret],
    Aarch64NeonUaddlp = "llvm.aarch64.neon.uaddlp" [Ret, P(0)],
    Aarch64NeonUaddlv = "llvm.aarch64.neon.uaddlv" [Ret, P(0)],
    Aarch64NeonUaddv = "llvm.aarch64.neon.uaddv" [Ret, P(0)],
    Aarch64NeonUdot = "llvm.aarch64.neon.udot" [Ret, P(1)],
    Aarch64NeonUhadd = "llvm.aarch64.neon.uhadd" [Ret],
    Aarch64NeonUhsub = "llvm.aarch64.neon.uhsub" [Ret],
    Aarch64NeonUmax = "llvm.aarch64.neon.umax" [Ret],
    Aarch64NeonUmaxp = "llvm.aarch64.neon.umaxp" [Ret],
    Aarch64NeonUmaxv = "llvm.aarch64.neon.umaxv" [Ret, P(0)],
    Aarch64NeonUmin = "llvm.aarch64.neon.umin" [Ret],
    Aarch64NeonUminp = "llvm.aarch64.neon.uminp" [Ret],
    Aarch64NeonUminv = "llvm.aarch64.neon.uminv" [Ret, P(0)],
    Aarch64NeonUmmla = "llvm.aarch64.neon.ummla" [Ret, P(1)],
    Aarch64NeonUmull = "llvm.aarch64.neon.umull" [Ret],
    Aarch64NeonUqadd = "llvm.aarch64.neon.uqadd" [Ret],
    Aarch64NeonUqrshl = "llvm.aarch64.neon.uqrshl" [Ret],
    Aarch64NeonUqrshrn = "llvm.aarch64.neon.uqrshrn" [Ret],
    Aarch64NeonUqshl = "llvm.aarch64.neon.uqshl" [Ret],
    Aarch64NeonUqshrn = "llvm.aarch64.neon.uqshrn" [Ret],
    Aarch64NeonUqsub = "llvm.aarch64.neon.uqsub" [Ret],
    Aarch64NeonUqxtn = "llvm.aarch64.neon.uqxtn" [Ret],
    Aarch64NeonUrecpe = "llvm.aarch64.neon.urecpe" [Ret],
    Aarch64NeonUrhadd = "llvm.aarch64.neon.urhadd" [Ret],
    Aarch64NeonUrshl = "llvm.aarch64.neon.urshl" [Ret],
    Aarch64NeonUrsqrte = "llvm.aarch64.neon.ursqrte" [Ret],
    Aarch64NeonUsdot = "llvm.aarch64.neon.usdot" [Ret, P(1)],
    Aarch64NeonUshl = "llvm.aarch64.neon.ushl" [Ret],
    Aarch64NeonUsmmla = "llvm.aarch64.neon.usmmla" [Ret, P(1)],
    Aarch64NeonUsqadd = "llvm.aarch64.neon.usqadd" [Ret],
    Aarch64NeonVcaddRot270 = "llvm.aarch64.neon.vcadd.rot270" [Ret],
    Aarch64NeonVcaddRot90 = "llvm.aarch64.neon.vcadd.rot90" [Ret],
    Aarch64NeonVcmlaRot0 = "llvm.aarch64.neon.vcmla.rot0" [Ret],
    Aarch64NeonVcmlaRot180 = "llvm.aarch64.neon.vcmla.rot180" [Ret],
    Aarch64NeonVcmlaRot270 = "llvm.aarch64.neon.vcmla.rot270" [Ret],
    Aarch64NeonVcmlaRot90 = "llvm.aarch64.neon.vcmla.rot90" [Ret],
    Aarch64NeonVcvtfp2fxs = "llvm.aarch64.neon.vcvtfp2fxs" [Ret, P(0)],
    Aarch64NeonVcvtfp2fxu = "llvm.aarch64.neon.vcvtfp2fxu" [Ret, P(0)],
    Aarch64NeonVcvtfp2hf = "llvm.aarch64.neon.vcvtfp2hf" [],
    Aarch64NeonVcvtfxs2fp = "llvm.aarch64.neon.vcvtfxs2fp" [Ret, P(0)],
    Aarch64NeonVcvtfxu2fp = "llvm.aarch64.neon.vcvtfxu2fp" [Ret, P(0)],
    Aarch64NeonVcvthf2fp = "llvm.aarch64.neon.vcvthf2fp" [],
    Aarch64NeonVsli = "llvm.aarch64.neon.vsli" [Ret],
    Aarch64NeonVsri = "llvm.aarch64.neon.vsri" [Ret],
    Aarch64SisdFabd = "llvm.aarch64.sisd.fabd" [Ret],
    Aarch64SisdFcvtxn = "llvm.aarch64.sisd.fcvtxn" [],
}

/// The most overload types an intrinsic on the list has.
const MAX_OVERLOADS: usize = 3;

/// The code a call record stores for `Other`.
pub(crate) const CODE_OTHER: u16 = u16::MAX;

impl<'a> Intrinsic<'a> {
    /// The name without overload suffixes (`llvm.memcpy`); for `Other`, the
    /// whole name.
    pub fn name(self) -> &'a str {
        match self.index() {
            Some(index) => TABLE[index].0,
            None => match self {
                Intrinsic::Other(name) => name,
                _ => "",
            },
        }
    }

    /// The arguments LLVM marks `immarg`, by index: each must be a
    /// constant integer or float, never `undef` or `poison` (§5.8). The
    /// same in LLVM 19 and LLVM 22.
    pub fn immediate_args(self) -> &'static [usize] {
        use Intrinsic as I;
        match self {
            I::Memcpy | I::MemcpyInline | I::Memmove | I::Memset | I::MemsetInline => &[3],
            I::FrameAddress | I::ReturnAddress | I::UbsanTrap => &[0],
            I::Ctlz | I::Cttz | I::Abs | I::IsFpClass | I::VectorExtract => &[1],
            I::Prefetch | I::ObjectSize => &[1, 2, 3],
            I::VectorInsert => &[2],
            _ => &[],
        }
    }

    /// Whether LLVM allows `invoke` of this intrinsic (§5.8): the
    /// coroutine resumers and `await.suspend`, and of the names hod does
    /// not know, `llvm.donothing`, the SEH scope markers, the patchpoints
    /// and statepoints, and (LLVM 22, not LLVM 19, which the printer
    /// refuses them for, §9.6) `llvm.wasm.throw`, `llvm.wasm.rethrow` and
    /// `llvm.fake.use`.
    pub fn is_invokable(self) -> bool {
        use Intrinsic as I;
        match self {
            I::CoroResume
            | I::CoroDestroy
            | I::CoroAwaitSuspendVoid
            | I::CoroAwaitSuspendBool
            | I::CoroAwaitSuspendHandle => true,
            I::Other(name) => {
                matches!(
                    name,
                    "llvm.donothing"
                        | "llvm.seh.try.begin"
                        | "llvm.seh.try.end"
                        | "llvm.seh.scope.begin"
                        | "llvm.seh.scope.end"
                        | "llvm.wasm.throw"
                        | "llvm.wasm.rethrow"
                        | "llvm.fake.use"
                ) || name.starts_with("llvm.experimental.patchpoint.")
                    || name.starts_with("llvm.experimental.gc.statepoint.")
            }
            _ => false,
        }
    }

    /// How many overload types the intrinsic has; 0 for `Other`.
    pub fn overload_count(self) -> usize {
        self.index().map_or(0, |index| TABLE[index].1.len())
    }

    /// The full LLVM name for `overloads` (`llvm.memcpy.p0.p0.i64`), or
    /// `None` if a type has no mangling here. For `Other`, the name itself.
    pub fn mangled_name<L: Layering>(
        self,
        types: &TypeTable<L>,
        overloads: &[TypeId],
    ) -> Option<String> {
        let mut name = self.name().to_string();
        if self.index().is_some() {
            for &ty in overloads {
                name.push('.');
                if !mangle(types, ty, &mut name) {
                    return None;
                }
            }
        }
        Some(name)
    }

    /// The overload types of a call of this intrinsic made with function
    /// type `fn_ty`: LLVM derives them from the signature. `None` if
    /// `fn_ty` is not a function type or lacks a parameter or return field
    /// the intrinsic reads one from. Entries past
    /// [`Intrinsic::overload_count`] are `void`.
    pub(crate) fn overloads_from<L: Layering>(
        self,
        types: &TypeTable<L>,
        fn_ty: TypeId,
    ) -> Option<[TypeId; MAX_OVERLOADS]> {
        let sources = self.index().map_or(&[][..], |index| TABLE[index].1);
        let TypeKind::Function { ret, params, .. } = types.kind(fn_ty) else { return None };
        let mut overloads = [TypeId::VOID; MAX_OVERLOADS];
        for (slot, source) in overloads.iter_mut().zip(sources) {
            *slot = match *source {
                Ret => ret,
                P(n) => *params.get(usize::from(n))?,
                RetField(n) => *types.struct_fields(ret)?.get(usize::from(n))?,
            };
        }
        Some(overloads)
    }

    /// The code a call record stores: 0 is "not an intrinsic".
    pub(crate) fn code(self) -> u16 {
        self.index().map_or(CODE_OTHER, |index| index as u16 + 1)
    }

    /// The known intrinsic stored as `code`.
    pub(crate) fn from_code(code: u16) -> Option<Intrinsic<'static>> {
        Intrinsic::ALL.get(usize::from(code).wrapping_sub(1)).copied()
    }
}

/// An intrinsic and its overload types, as [`resolve_intrinsic`] finds them.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct IntrinsicRef<'a> {
    /// Which intrinsic.
    pub intrinsic: Intrinsic<'a>,
    overloads: [TypeId; MAX_OVERLOADS],
    len: u8,
}

impl IntrinsicRef<'_> {
    /// The overload types, in the order of the name's suffixes.
    pub fn overloads(&self) -> &[TypeId] {
        &self.overloads[..usize::from(self.len)]
    }
}

/// Maps the declaration of an `llvm.*` function to hod's intrinsic (§5.5).
/// Every producer calls this, so that all of them map names the same way.
///
/// The overload types are read from `fn_ty` (as LLVM does), and the name
/// must be exactly the one LLVM gives that intrinsic with those types.
/// Anything else is `Intrinsic::Other(name)` with no overloads: an unknown
/// name, a known base name with a signature that does not fit it, or a
/// spelling LLVM would only accept by renaming (the typed-pointer suffixes
/// of old IR). One spelling without a suffix is accepted:
/// `llvm.thread.pointer`, which was not overloaded before LLVM 21.
///
/// A producer first asks [`no_code_intrinsic`]; names it claims never get
/// here.
pub fn resolve_intrinsic<'a, L: Layering>(
    types: &TypeTable<L>,
    name: &'a str,
    fn_ty: TypeId,
) -> IntrinsicRef<'a> {
    let other = IntrinsicRef {
        intrinsic: Intrinsic::Other(name),
        overloads: [TypeId::VOID; MAX_OVERLOADS],
        len: 0,
    };
    // The longest base name that is `name` or a dotted prefix of it.
    let found = TABLE
        .iter()
        .enumerate()
        .filter(|(_, (base, _))| has_dotted_prefix(name, base))
        .max_by_key(|(_, (base, _))| base.len());
    let Some((index, &(base, sources))) = found else { return other };
    let intrinsic = Intrinsic::ALL[index];
    let Some(overloads) = intrinsic.overloads_from(types, fn_ty) else { return other };
    let len = sources.len();
    let exact = intrinsic.mangled_name(types, &overloads[..len]).is_some_and(|m| m == name);
    let unsuffixed_thread_pointer =
        intrinsic == Intrinsic::ThreadPointer && name == base && overloads[0] == TypeId::PTR;
    if !exact && !unsuffixed_thread_pointer {
        return other;
    }
    IntrinsicRef { intrinsic, overloads, len: len as u8 }
}

/// Whether `name` is `prefix` or starts with `prefix` and a dot.
fn has_dotted_prefix(name: &str, prefix: &str) -> bool {
    name.strip_prefix(prefix).is_some_and(|rest| rest.is_empty() || rest.starts_with('.'))
}

/// Appends LLVM's mangling of `ty` as an intrinsic name suffix
/// (`getMangledTypeStr`). False for a type whose mangling hod cannot
/// reproduce: named structs (names are not kept, §3), opaque structs,
/// target extension types, and the non-value types.
fn mangle<L: Layering>(types: &TypeTable<L>, ty: TypeId, out: &mut String) -> bool {
    use std::fmt::Write as _;
    // Writing to a `String` cannot fail.
    let _ = match types.kind(ty) {
        TypeKind::Int(bits) => write!(out, "i{bits}"),
        TypeKind::Half => write!(out, "f16"),
        TypeKind::BFloat => write!(out, "bf16"),
        TypeKind::Float => write!(out, "f32"),
        TypeKind::Double => write!(out, "f64"),
        TypeKind::Fp128 => write!(out, "f128"),
        TypeKind::X86Fp80 => write!(out, "f80"),
        TypeKind::Ptr { addr_space } => write!(out, "p{addr_space}"),
        TypeKind::Vector { elem, len } => {
            let _ = write!(out, "v{len}");
            return mangle(types, elem, out);
        }
        TypeKind::ScalableVector { elem, min_len } => {
            let _ = write!(out, "nxv{min_len}");
            return mangle(types, elem, out);
        }
        TypeKind::Array { elem, len } => {
            let _ = write!(out, "a{len}");
            return mangle(types, elem, out);
        }
        TypeKind::Struct { fields, .. } => {
            out.push_str("sl_");
            if !fields.iter().all(|&field| mangle(types, field, out)) {
                return false;
            }
            out.push('s');
            return true;
        }
        TypeKind::Function { ret, params, variadic } => {
            out.push_str("f_");
            if !mangle(types, ret, out) || !params.iter().all(|&param| mangle(types, param, out)) {
                return false;
            }
            if variadic {
                out.push_str("vararg");
            }
            out.push('f');
            return true;
        }
        _ => return false,
    };
    true
}

/// What a producer does with a call to an intrinsic that generates no code
/// (§5.5). Such a call has no representation in a body.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum NoCode {
    /// Leave the call out. If it has a result (`llvm.invariant.start`), the
    /// result stands for `poison`: only other dropped calls may use it.
    Drop,
    /// Leave the call out and use its first argument wherever its result
    /// is used.
    FirstArg,
    /// Not code: a debug record (§6), or nothing when the producer was not
    /// asked for debug information.
    DebugRecord,
}

/// Base names and what to do with them; a name matches a base when it is
/// the base or continues it with a dot.
const NO_CODE: &[(&str, NoCode)] = &[
    ("llvm.lifetime.start", NoCode::Drop),
    ("llvm.lifetime.end", NoCode::Drop),
    ("llvm.invariant.start", NoCode::Drop),
    ("llvm.invariant.end", NoCode::Drop),
    ("llvm.assume", NoCode::Drop),
    ("llvm.donothing", NoCode::Drop),
    ("llvm.var.annotation", NoCode::Drop),
    ("llvm.experimental.noalias.scope.decl", NoCode::Drop),
    ("llvm.expect", NoCode::FirstArg),
    ("llvm.annotation", NoCode::FirstArg),
    ("llvm.ptr.annotation", NoCode::FirstArg),
    ("llvm.launder.invariant.group", NoCode::FirstArg),
    ("llvm.strip.invariant.group", NoCode::FirstArg),
    ("llvm.dbg", NoCode::DebugRecord),
];

/// The base names [`no_code_intrinsic`] claims, for documentation and
/// tests.
pub fn no_code_names() -> impl Iterator<Item = (&'static str, NoCode)> {
    NO_CODE.iter().copied()
}

/// The producer-side rule for intrinsics that generate no code (§5.5):
/// `Some` says the call named `name` is not to be built, and what replaces
/// it. Every producer asks this before [`resolve_intrinsic`].
pub fn no_code_intrinsic(name: &str) -> Option<NoCode> {
    NO_CODE.iter().find(|(base, _)| has_dotted_prefix(name, base)).map(|&(_, rule)| rule)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::module::Module;
    use crate::target::TargetSpec;

    /// §5.5: names map to a member and its overload types, read from the
    /// function type.
    #[test]
    fn names_resolve_with_overloads() {
        let mut m = Module::new(TargetSpec::aarch64_linux_gnu());
        let t = &mut m.types;
        let (ptr, i1, i32, i64) = (TypeId::PTR, TypeId::I1, TypeId::I32, TypeId::I64);
        let memcpy = t.function(TypeId::VOID, &[ptr, ptr, i64, i1], false).unwrap();
        let r = resolve_intrinsic(t, "llvm.memcpy.p0.p0.i64", memcpy);
        assert_eq!(r.intrinsic, Intrinsic::Memcpy);
        assert_eq!(r.overloads(), [ptr, ptr, i64]);
        // The longest base wins: `llvm.memcpy.inline`, not `llvm.memcpy`.
        let r = resolve_intrinsic(t, "llvm.memcpy.inline.p0.p0.i64", memcpy);
        assert_eq!(r.intrinsic, Intrinsic::MemcpyInline);
        let memset = t.function(TypeId::VOID, &[ptr, TypeId::I8, i32, i1], false).unwrap();
        let r = resolve_intrinsic(t, "llvm.memset.p0.i32", memset);
        assert_eq!((r.intrinsic, r.overloads()), (Intrinsic::Memset, &[ptr, i32][..]));

        let pair = t.struct_type(&[i32, i1], false).unwrap();
        let checked = t.function(pair, &[i32, i32], false).unwrap();
        let r = resolve_intrinsic(t, "llvm.sadd.with.overflow.i32", checked);
        assert_eq!((r.intrinsic, r.overloads()), (Intrinsic::SaddWithOverflow, &[i32][..]));

        let frexp_ret = t.struct_type(&[TypeId::DOUBLE, i32], false).unwrap();
        let frexp = t.function(frexp_ret, &[TypeId::DOUBLE], false).unwrap();
        let r = resolve_intrinsic(t, "llvm.frexp.f64.i32", frexp);
        assert_eq!((r.intrinsic, r.overloads()), (Intrinsic::Frexp, &[TypeId::DOUBLE, i32][..]));

        let v4 = t.vector(i32, 4).unwrap();
        let reduce = t.function(i32, &[v4], false).unwrap();
        let r = resolve_intrinsic(t, "llvm.vector.reduce.add.v4i32", reduce);
        assert_eq!((r.intrinsic, r.overloads()), (Intrinsic::VectorReduceAdd, &[v4][..]));

        let trap = t.function(TypeId::VOID, &[], false).unwrap();
        let r = resolve_intrinsic(t, "llvm.trap", trap);
        assert_eq!((r.intrinsic, r.overloads().len()), (Intrinsic::Trap, 0));

        let tp = t.function(ptr, &[], false).unwrap();
        assert_eq!(
            resolve_intrinsic(t, "llvm.thread.pointer.p0", tp).intrinsic,
            Intrinsic::ThreadPointer
        );
        assert_eq!(
            resolve_intrinsic(t, "llvm.thread.pointer", tp).intrinsic,
            Intrinsic::ThreadPointer
        );
    }

    /// §5.5: anything that is not exactly a known intrinsic is `Other`
    /// with its name.
    #[test]
    fn unknown_or_misnamed_is_other() {
        let mut m = Module::new(TargetSpec::aarch64_linux_gnu());
        let t = &mut m.types;
        let (ptr, i1, i64) = (TypeId::PTR, TypeId::I1, TypeId::I64);
        let memcpy = t.function(TypeId::VOID, &[ptr, ptr, i64, i1], false).unwrap();
        let cases = [
            // Unknown.
            ("llvm.x86.sse2.pause", memcpy),
            // A typed-pointer spelling LLVM would rename.
            ("llvm.memcpy.p0i8.p0i8.i64", memcpy),
            // The wrong suffix for the signature.
            ("llvm.memcpy.p0.p0.i32", memcpy),
            // No suffix where one is required.
            ("llvm.memcpy", memcpy),
            // A base name that only shares a prefix.
            ("llvm.memcpyfoo", memcpy),
            // Not a function type.
            ("llvm.trap", i64),
            // A suffix on an intrinsic that has no overloads.
            ("llvm.trap.i64", memcpy),
        ];
        for (name, ty) in cases {
            let r = resolve_intrinsic(t, name, ty);
            assert_eq!(r.intrinsic, Intrinsic::Other(name));
            assert!(r.overloads().is_empty());
            assert_eq!(r.intrinsic.name(), name);
            assert_eq!(r.intrinsic.code(), CODE_OTHER);
        }
        // Too few parameters for the overload sources.
        let short = t.function(TypeId::VOID, &[ptr], false).unwrap();
        assert!(matches!(
            resolve_intrinsic(t, "llvm.memcpy.p0", short).intrinsic,
            Intrinsic::Other(_)
        ));
    }

    /// §5.5: the list is closed, codes round-trip, base names are unique.
    #[test]
    fn table_is_consistent() {
        assert_eq!(Intrinsic::ALL.len(), TABLE.len());
        assert_eq!(Intrinsic::from_code(0), None);
        assert_eq!(Intrinsic::from_code(CODE_OTHER), None);
        for (index, &intrinsic) in Intrinsic::ALL.iter().enumerate() {
            assert_eq!(intrinsic.index(), Some(index));
            assert_eq!(Intrinsic::from_code(intrinsic.code()), Some(intrinsic));
            assert!(intrinsic.name().starts_with("llvm."));
            assert!(intrinsic.overload_count() <= MAX_OVERLOADS);
            assert_eq!(no_code_intrinsic(intrinsic.name()), None, "{intrinsic:?}");
            for &other in &Intrinsic::ALL[..index] {
                assert_ne!(other.name(), intrinsic.name());
            }
        }
        assert_eq!(Intrinsic::Other("llvm.x").overload_count(), 0);
    }

    /// §5.5: type manglings, including the ones no listed intrinsic uses
    /// today.
    #[test]
    fn type_mangling() {
        let mut m = Module::new(TargetSpec::aarch64_linux_gnu());
        let t = &mut m.types;
        let v = t.vector(TypeId::FLOAT, 4).unwrap();
        let sv = t.scalable_vector(TypeId::I8, 16).unwrap();
        let arr = t.array(TypeId::I16, 3).unwrap();
        let st = t.struct_type(&[TypeId::I32, TypeId::PTR], false).unwrap();
        let f = t.function(TypeId::I32, &[TypeId::PTR], true).unwrap();
        let p1 = t.ptr(1).unwrap();
        let opaque = t.opaque_struct().unwrap();
        let cases = [
            (TypeId::I1, "i1"),
            (TypeId::I128, "i128"),
            (TypeId::HALF, "f16"),
            (TypeId::BFLOAT, "bf16"),
            (TypeId::FLOAT, "f32"),
            (TypeId::DOUBLE, "f64"),
            (TypeId::FP128, "f128"),
            (TypeId::X86_FP80, "f80"),
            (TypeId::PTR, "p0"),
            (p1, "p1"),
            (v, "v4f32"),
            (sv, "nxv16i8"),
            (arr, "a3i16"),
            (st, "sl_i32p0s"),
            (f, "f_i32p0varargf"),
        ];
        for (ty, expected) in cases {
            let mut out = String::new();
            assert!(mangle(t, ty, &mut out));
            assert_eq!(out, expected);
        }
        for ty in [TypeId::VOID, TypeId::TOKEN, TypeId::METADATA, opaque] {
            assert!(!mangle(t, ty, &mut String::new()));
        }
        assert_eq!(
            Intrinsic::Ctlz.mangled_name(t, &[TypeId::I64]).as_deref(),
            Some("llvm.ctlz.i64")
        );
        assert_eq!(Intrinsic::Ctlz.mangled_name(t, &[opaque]), None);
        assert_eq!(Intrinsic::Other("llvm.q").mangled_name(t, &[v]).as_deref(), Some("llvm.q"));
    }

    /// §5.5: the intrinsics that generate no code, and what replaces them.
    #[test]
    fn no_code_rule() {
        let cases = [
            ("llvm.lifetime.start.p0", Some(NoCode::Drop)),
            ("llvm.lifetime.end.p0", Some(NoCode::Drop)),
            ("llvm.invariant.start.p0", Some(NoCode::Drop)),
            ("llvm.invariant.end.p0", Some(NoCode::Drop)),
            ("llvm.assume", Some(NoCode::Drop)),
            ("llvm.donothing", Some(NoCode::Drop)),
            ("llvm.var.annotation.p0.p0", Some(NoCode::Drop)),
            ("llvm.expect.i64", Some(NoCode::FirstArg)),
            ("llvm.expect.with.probability.i64", Some(NoCode::FirstArg)),
            ("llvm.annotation.i32.p0", Some(NoCode::FirstArg)),
            ("llvm.ptr.annotation.p0.p0", Some(NoCode::FirstArg)),
            ("llvm.launder.invariant.group.p0", Some(NoCode::FirstArg)),
            ("llvm.strip.invariant.group.p0", Some(NoCode::FirstArg)),
            ("llvm.dbg.declare", Some(NoCode::DebugRecord)),
            ("llvm.dbg.value", Some(NoCode::DebugRecord)),
            ("llvm.dbg.label", Some(NoCode::DebugRecord)),
            ("llvm.experimental.noalias.scope.decl", Some(NoCode::Drop)),
            ("llvm.expectation", None),
            ("llvm.memcpy.p0.p0.i64", None),
            ("llvm.threadlocal.address.p0", None),
            ("memcpy", None),
        ];
        for (name, expected) in cases {
            assert_eq!(no_code_intrinsic(name), expected, "{name}");
        }
        assert_eq!(no_code_names().count(), NO_CODE.len());
    }
}
