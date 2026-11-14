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
            // A constant promoted to static memory, never on the stack.
            #[allow(clippy::large_stack_arrays)]
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
    // `__builtin_setjmp`/`__builtin_longjmp` (WP-147).
    EhSjljSetjmp = "llvm.eh.sjlj.setjmp" [],
    EhSjljLongjmp = "llvm.eh.sjlj.longjmp" [],
    ThreadLocalAddress = "llvm.threadlocal.address" [Ret],
    ThreadPointer = "llvm.thread.pointer" [Ret],
    // Pointers, caches, counters.
    Prefetch = "llvm.prefetch" [P(0)],
    ClearCache = "llvm.clear_cache" [],
    ReadCycleCounter = "llvm.readcyclecounter" [],
    // Named registers: the first argument is a metadata operand (WP-064).
    ReadRegister = "llvm.read_register" [Ret],
    ReadVolatileRegister = "llvm.read_volatile_register" [Ret],
    WriteRegister = "llvm.write_register" [P(1)],
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
    // x86 spin-wait hint (WP-161): libstdc++'s `std::stop_token` yield.
    X86Sse2Pause = "llvm.x86.sse2.pause" [],
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
    // The census of `<arm_neon.h>` and `<arm_acle.h>` (WP-205): SM3/SM4,
    // the scalar FRINT32/64 and FJCVTZS of `<arm_acle.h>`.
    Aarch64CryptoSm3partw1 = "llvm.aarch64.crypto.sm3partw1" [],
    Aarch64CryptoSm3partw2 = "llvm.aarch64.crypto.sm3partw2" [],
    Aarch64CryptoSm3ss1 = "llvm.aarch64.crypto.sm3ss1" [],
    Aarch64CryptoSm3tt1a = "llvm.aarch64.crypto.sm3tt1a" [],
    Aarch64CryptoSm3tt1b = "llvm.aarch64.crypto.sm3tt1b" [],
    Aarch64CryptoSm3tt2a = "llvm.aarch64.crypto.sm3tt2a" [],
    Aarch64CryptoSm3tt2b = "llvm.aarch64.crypto.sm3tt2b" [],
    Aarch64CryptoSm4e = "llvm.aarch64.crypto.sm4e" [],
    Aarch64CryptoSm4ekey = "llvm.aarch64.crypto.sm4ekey" [],
    Aarch64Frint32x = "llvm.aarch64.frint32x" [Ret],
    Aarch64Frint32z = "llvm.aarch64.frint32z" [Ret],
    Aarch64Frint64x = "llvm.aarch64.frint64x" [Ret],
    Aarch64Frint64z = "llvm.aarch64.frint64z" [Ret],
    Aarch64Fjcvtzs = "llvm.aarch64.fjcvtzs" [],
    // x86 target intrinsics (WP-173): the names clang 19 and 22 emit at
    // `-O0` for the SSE, SSE2, SSE3 and SSSE3 headers that hod builds.
    X86SseCmpSs = "llvm.x86.sse.cmp.ss" [],
    X86SseComieqSs = "llvm.x86.sse.comieq.ss" [],
    X86SseComigeSs = "llvm.x86.sse.comige.ss" [],
    X86SseComigtSs = "llvm.x86.sse.comigt.ss" [],
    X86SseComileSs = "llvm.x86.sse.comile.ss" [],
    X86SseComiltSs = "llvm.x86.sse.comilt.ss" [],
    X86SseComineqSs = "llvm.x86.sse.comineq.ss" [],
    X86SseUcomieqSs = "llvm.x86.sse.ucomieq.ss" [],
    X86SseUcomigeSs = "llvm.x86.sse.ucomige.ss" [],
    X86SseUcomigtSs = "llvm.x86.sse.ucomigt.ss" [],
    X86SseUcomileSs = "llvm.x86.sse.ucomile.ss" [],
    X86SseUcomiltSs = "llvm.x86.sse.ucomilt.ss" [],
    X86SseUcomineqSs = "llvm.x86.sse.ucomineq.ss" [],
    X86SseCvtss2si = "llvm.x86.sse.cvtss2si" [],
    X86SseCvtss2si64 = "llvm.x86.sse.cvtss2si64" [],
    X86SseCvttss2si = "llvm.x86.sse.cvttss2si" [],
    X86SseCvttss2si64 = "llvm.x86.sse.cvttss2si64" [],
    X86SseMaxPs = "llvm.x86.sse.max.ps" [],
    X86SseMaxSs = "llvm.x86.sse.max.ss" [],
    X86SseMinPs = "llvm.x86.sse.min.ps" [],
    X86SseMinSs = "llvm.x86.sse.min.ss" [],
    X86SseMovmskPs = "llvm.x86.sse.movmsk.ps" [],
    X86SseSfence = "llvm.x86.sse.sfence" [],
    // WP-181: MXCSR and the approximations.
    X86SseLdmxcsr = "llvm.x86.sse.ldmxcsr" [],
    X86SseRcpPs = "llvm.x86.sse.rcp.ps" [],
    X86SseRcpSs = "llvm.x86.sse.rcp.ss" [],
    X86SseRsqrtPs = "llvm.x86.sse.rsqrt.ps" [],
    X86SseRsqrtSs = "llvm.x86.sse.rsqrt.ss" [],
    X86SseStmxcsr = "llvm.x86.sse.stmxcsr" [],
    X86Sse2CmpSd = "llvm.x86.sse2.cmp.sd" [],
    X86Sse2ComieqSd = "llvm.x86.sse2.comieq.sd" [],
    X86Sse2ComigeSd = "llvm.x86.sse2.comige.sd" [],
    X86Sse2ComigtSd = "llvm.x86.sse2.comigt.sd" [],
    X86Sse2ComileSd = "llvm.x86.sse2.comile.sd" [],
    X86Sse2ComiltSd = "llvm.x86.sse2.comilt.sd" [],
    X86Sse2ComineqSd = "llvm.x86.sse2.comineq.sd" [],
    X86Sse2UcomieqSd = "llvm.x86.sse2.ucomieq.sd" [],
    X86Sse2UcomigeSd = "llvm.x86.sse2.ucomige.sd" [],
    X86Sse2UcomigtSd = "llvm.x86.sse2.ucomigt.sd" [],
    X86Sse2UcomileSd = "llvm.x86.sse2.ucomile.sd" [],
    X86Sse2UcomiltSd = "llvm.x86.sse2.ucomilt.sd" [],
    X86Sse2UcomineqSd = "llvm.x86.sse2.ucomineq.sd" [],
    X86Sse2Cvtpd2dq = "llvm.x86.sse2.cvtpd2dq" [],
    X86Sse2Cvtpd2ps = "llvm.x86.sse2.cvtpd2ps" [],
    X86Sse2Cvtps2dq = "llvm.x86.sse2.cvtps2dq" [],
    X86Sse2Cvtsd2si = "llvm.x86.sse2.cvtsd2si" [],
    X86Sse2Cvtsd2si64 = "llvm.x86.sse2.cvtsd2si64" [],
    X86Sse2Cvtsd2ss = "llvm.x86.sse2.cvtsd2ss" [],
    X86Sse2Cvttpd2dq = "llvm.x86.sse2.cvttpd2dq" [],
    X86Sse2Cvttps2dq = "llvm.x86.sse2.cvttps2dq" [],
    X86Sse2Cvttsd2si = "llvm.x86.sse2.cvttsd2si" [],
    X86Sse2Cvttsd2si64 = "llvm.x86.sse2.cvttsd2si64" [],
    X86Sse2Lfence = "llvm.x86.sse2.lfence" [],
    X86Sse2Mfence = "llvm.x86.sse2.mfence" [],
    // WP-181.
    X86Sse2Clflush = "llvm.x86.sse2.clflush" [],
    X86Sse2MaskmovDqu = "llvm.x86.sse2.maskmov.dqu" [],
    X86Sse2MaxPd = "llvm.x86.sse2.max.pd" [],
    X86Sse2MaxSd = "llvm.x86.sse2.max.sd" [],
    X86Sse2MinPd = "llvm.x86.sse2.min.pd" [],
    X86Sse2MinSd = "llvm.x86.sse2.min.sd" [],
    X86Sse2MovmskPd = "llvm.x86.sse2.movmsk.pd" [],
    X86Sse2Packssdw = "llvm.x86.sse2.packssdw.128" [],
    X86Sse2Packsswb = "llvm.x86.sse2.packsswb.128" [],
    X86Sse2Packuswb = "llvm.x86.sse2.packuswb.128" [],
    X86Sse2PavgB = "llvm.x86.sse2.pavg.b" [],
    X86Sse2PavgW = "llvm.x86.sse2.pavg.w" [],
    X86Sse2PmaddWd = "llvm.x86.sse2.pmadd.wd" [],
    X86Sse2Pmovmskb = "llvm.x86.sse2.pmovmskb.128" [],
    X86Sse2PmulhW = "llvm.x86.sse2.pmulh.w" [],
    X86Sse2PmulhuW = "llvm.x86.sse2.pmulhu.w" [],
    X86Sse2PsadBw = "llvm.x86.sse2.psad.bw" [],
    X86Sse2PsllD = "llvm.x86.sse2.psll.d" [],
    X86Sse2PsllQ = "llvm.x86.sse2.psll.q" [],
    X86Sse2PsllW = "llvm.x86.sse2.psll.w" [],
    X86Sse2PslliD = "llvm.x86.sse2.pslli.d" [],
    X86Sse2PslliQ = "llvm.x86.sse2.pslli.q" [],
    X86Sse2PslliW = "llvm.x86.sse2.pslli.w" [],
    X86Sse2PsraD = "llvm.x86.sse2.psra.d" [],
    X86Sse2PsraW = "llvm.x86.sse2.psra.w" [],
    X86Sse2PsraiD = "llvm.x86.sse2.psrai.d" [],
    X86Sse2PsraiW = "llvm.x86.sse2.psrai.w" [],
    X86Sse2PsrlD = "llvm.x86.sse2.psrl.d" [],
    X86Sse2PsrlQ = "llvm.x86.sse2.psrl.q" [],
    X86Sse2PsrlW = "llvm.x86.sse2.psrl.w" [],
    X86Sse2PsrliD = "llvm.x86.sse2.psrli.d" [],
    X86Sse2PsrliQ = "llvm.x86.sse2.psrli.q" [],
    X86Sse2PsrliW = "llvm.x86.sse2.psrli.w" [],
    X86Sse3AddsubPd = "llvm.x86.sse3.addsub.pd" [],
    X86Sse3AddsubPs = "llvm.x86.sse3.addsub.ps" [],
    X86Sse3HaddPd = "llvm.x86.sse3.hadd.pd" [],
    X86Sse3HaddPs = "llvm.x86.sse3.hadd.ps" [],
    X86Sse3HsubPd = "llvm.x86.sse3.hsub.pd" [],
    X86Sse3HsubPs = "llvm.x86.sse3.hsub.ps" [],
    X86Sse3LduDq = "llvm.x86.sse3.ldu.dq" [],
    X86Ssse3PhaddD = "llvm.x86.ssse3.phadd.d.128" [],
    X86Ssse3PhaddSw = "llvm.x86.ssse3.phadd.sw.128" [],
    X86Ssse3PhaddW = "llvm.x86.ssse3.phadd.w.128" [],
    X86Ssse3PhsubD = "llvm.x86.ssse3.phsub.d.128" [],
    X86Ssse3PhsubSw = "llvm.x86.ssse3.phsub.sw.128" [],
    X86Ssse3PhsubW = "llvm.x86.ssse3.phsub.w.128" [],
    X86Ssse3PmaddUbSw = "llvm.x86.ssse3.pmadd.ub.sw.128" [],
    X86Ssse3PmulHrSw = "llvm.x86.ssse3.pmul.hr.sw.128" [],
    X86Ssse3PshufB = "llvm.x86.ssse3.pshuf.b.128" [],
    X86Ssse3PsignB = "llvm.x86.ssse3.psign.b.128" [],
    X86Ssse3PsignD = "llvm.x86.ssse3.psign.d.128" [],
    X86Ssse3PsignW = "llvm.x86.ssse3.psign.w.128" [],
    // x86 system and state intrinsics (WP-190): the names clang 19 and 22
    // emit at `-O0` for the non-vector headers that hod builds.
    X86FlagsReadU64 = "llvm.x86.flags.read.u64" [],
    X86FlagsWriteU64 = "llvm.x86.flags.write.u64" [],
    X86Fxsave = "llvm.x86.fxsave" [],
    X86Fxsave64 = "llvm.x86.fxsave64" [],
    X86Fxrstor = "llvm.x86.fxrstor" [],
    X86Fxrstor64 = "llvm.x86.fxrstor64" [],
    X86Rdtsc = "llvm.x86.rdtsc" [],
    X86Rdtscp = "llvm.x86.rdtscp" [],
    X86Rdpmc = "llvm.x86.rdpmc" [],
    X86Addcarry32 = "llvm.x86.addcarry.32" [],
    X86Addcarry64 = "llvm.x86.addcarry.64" [],
    X86Subborrow32 = "llvm.x86.subborrow.32" [],
    X86Subborrow64 = "llvm.x86.subborrow.64" [],
    X86Xgetbv = "llvm.x86.xgetbv" [],
    X86Xsetbv = "llvm.x86.xsetbv" [],
    X86Rdrand16 = "llvm.x86.rdrand.16" [],
    X86Rdrand32 = "llvm.x86.rdrand.32" [],
    X86Rdrand64 = "llvm.x86.rdrand.64" [],
    X86Rdseed16 = "llvm.x86.rdseed.16" [],
    X86Rdseed32 = "llvm.x86.rdseed.32" [],
    X86Rdseed64 = "llvm.x86.rdseed.64" [],
    X86Xsave = "llvm.x86.xsave" [],
    X86Xsave64 = "llvm.x86.xsave64" [],
    X86Xrstor = "llvm.x86.xrstor" [],
    X86Xrstor64 = "llvm.x86.xrstor64" [],
    X86Xsaveopt = "llvm.x86.xsaveopt" [],
    X86Xsaveopt64 = "llvm.x86.xsaveopt64" [],
    X86Xsavec = "llvm.x86.xsavec" [],
    X86Xsavec64 = "llvm.x86.xsavec64" [],
    X86Xsaves = "llvm.x86.xsaves" [],
    X86Xsaves64 = "llvm.x86.xsaves64" [],
    X86Xrstors = "llvm.x86.xrstors" [],
    X86Xrstors64 = "llvm.x86.xrstors64" [],
    X86Clflushopt = "llvm.x86.clflushopt" [],
    X86Clwb = "llvm.x86.clwb" [],
    X86BmiBextr32 = "llvm.x86.bmi.bextr.32" [],
    X86BmiBextr64 = "llvm.x86.bmi.bextr.64" [],
    X86BmiBzhi32 = "llvm.x86.bmi.bzhi.32" [],
    X86BmiBzhi64 = "llvm.x86.bmi.bzhi.64" [],
    X86BmiPdep32 = "llvm.x86.bmi.pdep.32" [],
    X86BmiPdep64 = "llvm.x86.bmi.pdep.64" [],
    X86BmiPext32 = "llvm.x86.bmi.pext.32" [],
    X86BmiPext64 = "llvm.x86.bmi.pext.64" [],
    X86Sse42Crc32x8 = "llvm.x86.sse42.crc32.32.8" [],
    X86Sse42Crc32x16 = "llvm.x86.sse42.crc32.32.16" [],
    X86Sse42Crc32x32 = "llvm.x86.sse42.crc32.32.32" [],
    X86Sse42Crc64x64 = "llvm.x86.sse42.crc32.64.64" [],
    X86Rdfsbase32 = "llvm.x86.rdfsbase.32" [],
    X86Rdfsbase64 = "llvm.x86.rdfsbase.64" [],
    X86Rdgsbase32 = "llvm.x86.rdgsbase.32" [],
    X86Rdgsbase64 = "llvm.x86.rdgsbase.64" [],
    X86Wrfsbase32 = "llvm.x86.wrfsbase.32" [],
    X86Wrfsbase64 = "llvm.x86.wrfsbase.64" [],
    X86Wrgsbase32 = "llvm.x86.wrgsbase.32" [],
    X86Wrgsbase64 = "llvm.x86.wrgsbase.64" [],
    X86Xbegin = "llvm.x86.xbegin" [],
    X86Xend = "llvm.x86.xend" [],
    X86Xabort = "llvm.x86.xabort" [],
    X86Xtest = "llvm.x86.xtest" [],
    X86Rdpid = "llvm.x86.rdpid" [],
    X86Rdpkru = "llvm.x86.rdpkru" [],
    X86Wrpkru = "llvm.x86.wrpkru" [],
    X86Serialize = "llvm.x86.serialize" [],
    X86Incsspd = "llvm.x86.incsspd" [],
    X86Incsspq = "llvm.x86.incsspq" [],
    X86Rdsspd = "llvm.x86.rdsspd" [],
    X86Rdsspq = "llvm.x86.rdsspq" [],
    // x86 vector intrinsics past SSSE3 (WP-202): the names clang 19 and 22
    // emit at `-O0` for the SSE4.1, SSE4.2, AES, PCLMUL, SHA, AVX, AVX2,
    // FMA, F16C and GFNI headers that hod builds.
    X86Sse41RoundPs = "llvm.x86.sse41.round.ps" [],
    X86Sse41RoundPd = "llvm.x86.sse41.round.pd" [],
    X86Sse41RoundSs = "llvm.x86.sse41.round.ss" [],
    X86Sse41RoundSd = "llvm.x86.sse41.round.sd" [],
    X86Sse41Blendvps = "llvm.x86.sse41.blendvps" [],
    X86Sse41Blendvpd = "llvm.x86.sse41.blendvpd" [],
    X86Sse41Pblendvb = "llvm.x86.sse41.pblendvb" [],
    X86Sse41Dpps = "llvm.x86.sse41.dpps" [],
    X86Sse41Dppd = "llvm.x86.sse41.dppd" [],
    X86Sse41Insertps = "llvm.x86.sse41.insertps" [],
    X86Sse41Mpsadbw = "llvm.x86.sse41.mpsadbw" [],
    X86Sse41Packusdw = "llvm.x86.sse41.packusdw" [],
    X86Sse41Phminposuw = "llvm.x86.sse41.phminposuw" [],
    X86Sse41Ptestz = "llvm.x86.sse41.ptestz" [],
    X86Sse41Ptestc = "llvm.x86.sse41.ptestc" [],
    X86Sse41Ptestnzc = "llvm.x86.sse41.ptestnzc" [],
    X86Sse42Pcmpistrm128 = "llvm.x86.sse42.pcmpistrm128" [],
    X86Sse42Pcmpistri128 = "llvm.x86.sse42.pcmpistri128" [],
    X86Sse42Pcmpistria128 = "llvm.x86.sse42.pcmpistria128" [],
    X86Sse42Pcmpistric128 = "llvm.x86.sse42.pcmpistric128" [],
    X86Sse42Pcmpistrio128 = "llvm.x86.sse42.pcmpistrio128" [],
    X86Sse42Pcmpistris128 = "llvm.x86.sse42.pcmpistris128" [],
    X86Sse42Pcmpistriz128 = "llvm.x86.sse42.pcmpistriz128" [],
    X86Sse42Pcmpestrm128 = "llvm.x86.sse42.pcmpestrm128" [],
    X86Sse42Pcmpestri128 = "llvm.x86.sse42.pcmpestri128" [],
    X86Sse42Pcmpestria128 = "llvm.x86.sse42.pcmpestria128" [],
    X86Sse42Pcmpestric128 = "llvm.x86.sse42.pcmpestric128" [],
    X86Sse42Pcmpestrio128 = "llvm.x86.sse42.pcmpestrio128" [],
    X86Sse42Pcmpestris128 = "llvm.x86.sse42.pcmpestris128" [],
    X86Sse42Pcmpestriz128 = "llvm.x86.sse42.pcmpestriz128" [],
    X86AesniAesenc = "llvm.x86.aesni.aesenc" [],
    X86AesniAesenclast = "llvm.x86.aesni.aesenclast" [],
    X86AesniAesdec = "llvm.x86.aesni.aesdec" [],
    X86AesniAesdeclast = "llvm.x86.aesni.aesdeclast" [],
    X86AesniAesimc = "llvm.x86.aesni.aesimc" [],
    X86AesniAeskeygenassist = "llvm.x86.aesni.aeskeygenassist" [],
    X86Pclmulqdq = "llvm.x86.pclmulqdq" [],
    X86Sha1rnds4 = "llvm.x86.sha1rnds4" [],
    X86Sha1nexte = "llvm.x86.sha1nexte" [],
    X86Sha1msg1 = "llvm.x86.sha1msg1" [],
    X86Sha1msg2 = "llvm.x86.sha1msg2" [],
    X86Sha256msg1 = "llvm.x86.sha256msg1" [],
    X86Sha256msg2 = "llvm.x86.sha256msg2" [],
    X86Sha256rnds2 = "llvm.x86.sha256rnds2" [],
    X86AvxAddsubPd256 = "llvm.x86.avx.addsub.pd.256" [],
    X86AvxAddsubPs256 = "llvm.x86.avx.addsub.ps.256" [],
    X86AvxMaxPd256 = "llvm.x86.avx.max.pd.256" [],
    X86AvxMaxPs256 = "llvm.x86.avx.max.ps.256" [],
    X86AvxMinPd256 = "llvm.x86.avx.min.pd.256" [],
    X86AvxMinPs256 = "llvm.x86.avx.min.ps.256" [],
    X86AvxHaddPd256 = "llvm.x86.avx.hadd.pd.256" [],
    X86AvxHaddPs256 = "llvm.x86.avx.hadd.ps.256" [],
    X86AvxHsubPd256 = "llvm.x86.avx.hsub.pd.256" [],
    X86AvxHsubPs256 = "llvm.x86.avx.hsub.ps.256" [],
    X86AvxBlendvPd256 = "llvm.x86.avx.blendv.pd.256" [],
    X86AvxBlendvPs256 = "llvm.x86.avx.blendv.ps.256" [],
    X86AvxDpPs256 = "llvm.x86.avx.dp.ps.256" [],
    X86AvxCmpPd256 = "llvm.x86.avx.cmp.pd.256" [],
    X86AvxCmpPs256 = "llvm.x86.avx.cmp.ps.256" [],
    X86AvxCvtPd2Ps256 = "llvm.x86.avx.cvt.pd2.ps.256" [],
    X86AvxCvtPs2dq256 = "llvm.x86.avx.cvt.ps2dq.256" [],
    X86AvxCvtPd2dq256 = "llvm.x86.avx.cvt.pd2dq.256" [],
    X86AvxCvttPd2dq256 = "llvm.x86.avx.cvtt.pd2dq.256" [],
    X86AvxCvttPs2dq256 = "llvm.x86.avx.cvtt.ps2dq.256" [],
    X86AvxRcpPs256 = "llvm.x86.avx.rcp.ps.256" [],
    X86AvxRsqrtPs256 = "llvm.x86.avx.rsqrt.ps.256" [],
    X86AvxRoundPd256 = "llvm.x86.avx.round.pd.256" [],
    X86AvxRoundPs256 = "llvm.x86.avx.round.ps.256" [],
    X86AvxVpermilvarPs = "llvm.x86.avx.vpermilvar.ps" [],
    X86AvxVpermilvarPd = "llvm.x86.avx.vpermilvar.pd" [],
    X86AvxVpermilvarPs256 = "llvm.x86.avx.vpermilvar.ps.256" [],
    X86AvxVpermilvarPd256 = "llvm.x86.avx.vpermilvar.pd.256" [],
    X86AvxVtestzPs = "llvm.x86.avx.vtestz.ps" [],
    X86AvxVtestzPd = "llvm.x86.avx.vtestz.pd" [],
    X86AvxVtestzPs256 = "llvm.x86.avx.vtestz.ps.256" [],
    X86AvxVtestzPd256 = "llvm.x86.avx.vtestz.pd.256" [],
    X86AvxVtestcPs = "llvm.x86.avx.vtestc.ps" [],
    X86AvxVtestcPd = "llvm.x86.avx.vtestc.pd" [],
    X86AvxVtestcPs256 = "llvm.x86.avx.vtestc.ps.256" [],
    X86AvxVtestcPd256 = "llvm.x86.avx.vtestc.pd.256" [],
    X86AvxVtestnzcPs = "llvm.x86.avx.vtestnzc.ps" [],
    X86AvxVtestnzcPd = "llvm.x86.avx.vtestnzc.pd" [],
    X86AvxVtestnzcPs256 = "llvm.x86.avx.vtestnzc.ps.256" [],
    X86AvxVtestnzcPd256 = "llvm.x86.avx.vtestnzc.pd.256" [],
    X86AvxPtestz256 = "llvm.x86.avx.ptestz.256" [],
    X86AvxPtestc256 = "llvm.x86.avx.ptestc.256" [],
    X86AvxPtestnzc256 = "llvm.x86.avx.ptestnzc.256" [],
    X86AvxMovmskPd256 = "llvm.x86.avx.movmsk.pd.256" [],
    X86AvxMovmskPs256 = "llvm.x86.avx.movmsk.ps.256" [],
    X86AvxLduDq256 = "llvm.x86.avx.ldu.dq.256" [],
    X86AvxMaskloadPs = "llvm.x86.avx.maskload.ps" [],
    X86AvxMaskstorePs = "llvm.x86.avx.maskstore.ps" [],
    X86AvxMaskloadPd = "llvm.x86.avx.maskload.pd" [],
    X86AvxMaskstorePd = "llvm.x86.avx.maskstore.pd" [],
    X86AvxMaskloadPs256 = "llvm.x86.avx.maskload.ps.256" [],
    X86AvxMaskstorePs256 = "llvm.x86.avx.maskstore.ps.256" [],
    X86AvxMaskloadPd256 = "llvm.x86.avx.maskload.pd.256" [],
    X86AvxMaskstorePd256 = "llvm.x86.avx.maskstore.pd.256" [],
    X86AvxVzeroall = "llvm.x86.avx.vzeroall" [],
    X86AvxVzeroupper = "llvm.x86.avx.vzeroupper" [],
    X86Avx2Packssdw = "llvm.x86.avx2.packssdw" [],
    X86Avx2Packsswb = "llvm.x86.avx2.packsswb" [],
    X86Avx2Packusdw = "llvm.x86.avx2.packusdw" [],
    X86Avx2Packuswb = "llvm.x86.avx2.packuswb" [],
    X86Avx2PavgB = "llvm.x86.avx2.pavg.b" [],
    X86Avx2PavgW = "llvm.x86.avx2.pavg.w" [],
    X86Avx2Pblendvb = "llvm.x86.avx2.pblendvb" [],
    X86Avx2PhaddD = "llvm.x86.avx2.phadd.d" [],
    X86Avx2PhaddW = "llvm.x86.avx2.phadd.w" [],
    X86Avx2PhaddSw = "llvm.x86.avx2.phadd.sw" [],
    X86Avx2PhsubD = "llvm.x86.avx2.phsub.d" [],
    X86Avx2PhsubW = "llvm.x86.avx2.phsub.w" [],
    X86Avx2PhsubSw = "llvm.x86.avx2.phsub.sw" [],
    X86Avx2PmaddUbSw = "llvm.x86.avx2.pmadd.ub.sw" [],
    X86Avx2PmaddWd = "llvm.x86.avx2.pmadd.wd" [],
    X86Avx2Pmovmskb = "llvm.x86.avx2.pmovmskb" [],
    X86Avx2PmulHrSw = "llvm.x86.avx2.pmul.hr.sw" [],
    X86Avx2PmulhW = "llvm.x86.avx2.pmulh.w" [],
    X86Avx2PmulhuW = "llvm.x86.avx2.pmulhu.w" [],
    X86Avx2PsadBw = "llvm.x86.avx2.psad.bw" [],
    X86Avx2PshufB = "llvm.x86.avx2.pshuf.b" [],
    X86Avx2PsignB = "llvm.x86.avx2.psign.b" [],
    X86Avx2PsignW = "llvm.x86.avx2.psign.w" [],
    X86Avx2PsignD = "llvm.x86.avx2.psign.d" [],
    X86Avx2PsllW = "llvm.x86.avx2.psll.w" [],
    X86Avx2PsllD = "llvm.x86.avx2.psll.d" [],
    X86Avx2PsllQ = "llvm.x86.avx2.psll.q" [],
    X86Avx2PslliW = "llvm.x86.avx2.pslli.w" [],
    X86Avx2PslliD = "llvm.x86.avx2.pslli.d" [],
    X86Avx2PslliQ = "llvm.x86.avx2.pslli.q" [],
    X86Avx2PsrlW = "llvm.x86.avx2.psrl.w" [],
    X86Avx2PsrlD = "llvm.x86.avx2.psrl.d" [],
    X86Avx2PsrlQ = "llvm.x86.avx2.psrl.q" [],
    X86Avx2PsrliW = "llvm.x86.avx2.psrli.w" [],
    X86Avx2PsrliD = "llvm.x86.avx2.psrli.d" [],
    X86Avx2PsrliQ = "llvm.x86.avx2.psrli.q" [],
    X86Avx2PsraW = "llvm.x86.avx2.psra.w" [],
    X86Avx2PsraD = "llvm.x86.avx2.psra.d" [],
    X86Avx2PsraiW = "llvm.x86.avx2.psrai.w" [],
    X86Avx2PsraiD = "llvm.x86.avx2.psrai.d" [],
    X86Avx2PsllvD = "llvm.x86.avx2.psllv.d" [],
    X86Avx2PsllvD256 = "llvm.x86.avx2.psllv.d.256" [],
    X86Avx2PsllvQ = "llvm.x86.avx2.psllv.q" [],
    X86Avx2PsllvQ256 = "llvm.x86.avx2.psllv.q.256" [],
    X86Avx2PsrlvD = "llvm.x86.avx2.psrlv.d" [],
    X86Avx2PsrlvD256 = "llvm.x86.avx2.psrlv.d.256" [],
    X86Avx2PsrlvQ = "llvm.x86.avx2.psrlv.q" [],
    X86Avx2PsrlvQ256 = "llvm.x86.avx2.psrlv.q.256" [],
    X86Avx2PsravD = "llvm.x86.avx2.psrav.d" [],
    X86Avx2PsravD256 = "llvm.x86.avx2.psrav.d.256" [],
    X86Avx2Permd = "llvm.x86.avx2.permd" [],
    X86Avx2Permps = "llvm.x86.avx2.permps" [],
    X86Avx2Mpsadbw = "llvm.x86.avx2.mpsadbw" [],
    X86Avx2MaskloadD = "llvm.x86.avx2.maskload.d" [],
    X86Avx2MaskstoreD = "llvm.x86.avx2.maskstore.d" [],
    X86Avx2MaskloadQ = "llvm.x86.avx2.maskload.q" [],
    X86Avx2MaskstoreQ = "llvm.x86.avx2.maskstore.q" [],
    X86Avx2MaskloadD256 = "llvm.x86.avx2.maskload.d.256" [],
    X86Avx2MaskstoreD256 = "llvm.x86.avx2.maskstore.d.256" [],
    X86Avx2MaskloadQ256 = "llvm.x86.avx2.maskload.q.256" [],
    X86Avx2MaskstoreQ256 = "llvm.x86.avx2.maskstore.q.256" [],
    X86FmaVfmaddsubPs = "llvm.x86.fma.vfmaddsub.ps" [],
    X86FmaVfmaddsubPd = "llvm.x86.fma.vfmaddsub.pd" [],
    X86FmaVfmaddsubPs256 = "llvm.x86.fma.vfmaddsub.ps.256" [],
    X86FmaVfmaddsubPd256 = "llvm.x86.fma.vfmaddsub.pd.256" [],
    X86Vcvtps2ph128 = "llvm.x86.vcvtps2ph.128" [],
    X86Vcvtps2ph256 = "llvm.x86.vcvtps2ph.256" [],
    X86Vgf2p8affineqb128 = "llvm.x86.vgf2p8affineqb.128" [],
    X86Vgf2p8affineqb256 = "llvm.x86.vgf2p8affineqb.256" [],
    X86Vgf2p8affineinvqb128 = "llvm.x86.vgf2p8affineinvqb.128" [],
    X86Vgf2p8affineinvqb256 = "llvm.x86.vgf2p8affineinvqb.256" [],
    X86Vgf2p8mulb128 = "llvm.x86.vgf2p8mulb.128" [],
    X86Vgf2p8mulb256 = "llvm.x86.vgf2p8mulb.256" [],
    // The AVX2 gathers (WP-207).
    X86Avx2GatherDD = "llvm.x86.avx2.gather.d.d" [],
    X86Avx2GatherDD256 = "llvm.x86.avx2.gather.d.d.256" [],
    X86Avx2GatherDQ = "llvm.x86.avx2.gather.d.q" [],
    X86Avx2GatherDQ256 = "llvm.x86.avx2.gather.d.q.256" [],
    X86Avx2GatherDPs = "llvm.x86.avx2.gather.d.ps" [],
    X86Avx2GatherDPs256 = "llvm.x86.avx2.gather.d.ps.256" [],
    X86Avx2GatherDPd = "llvm.x86.avx2.gather.d.pd" [],
    X86Avx2GatherDPd256 = "llvm.x86.avx2.gather.d.pd.256" [],
    X86Avx2GatherQD = "llvm.x86.avx2.gather.q.d" [],
    X86Avx2GatherQD256 = "llvm.x86.avx2.gather.q.d.256" [],
    X86Avx2GatherQQ = "llvm.x86.avx2.gather.q.q" [],
    X86Avx2GatherQQ256 = "llvm.x86.avx2.gather.q.q.256" [],
    X86Avx2GatherQPs = "llvm.x86.avx2.gather.q.ps" [],
    X86Avx2GatherQPs256 = "llvm.x86.avx2.gather.q.ps.256" [],
    X86Avx2GatherQPd = "llvm.x86.avx2.gather.q.pd" [],
    X86Avx2GatherQPd256 = "llvm.x86.avx2.gather.q.pd.256" [],
    // The masked memory intrinsics (WP-208): generic, but what clang 19
    // and 22 emit for the AVX-512 masked loads, stores, expand loads and
    // compress stores. LLVM 19 passes the alignment of `load`/`store` as
    // an `i32` operand, LLVM 22 as the pointer's `align` attribute.
    MaskedLoad = "llvm.masked.load" [Ret, P(0)],
    MaskedStore = "llvm.masked.store" [P(0), P(1)],
    MaskedExpandLoad = "llvm.masked.expandload" [Ret],
    MaskedCompressStore = "llvm.masked.compressstore" [P(0)],
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
            I::FrameAddress | I::ReturnAddress | I::UbsanTrap | I::X86Xabort => &[0],
            I::Ctlz
            | I::Cttz
            | I::Abs
            | I::IsFpClass
            | I::VectorExtract
            | I::X86Sse41RoundPs
            | I::X86Sse41RoundPd
            | I::X86AesniAeskeygenassist
            | I::X86AvxRoundPd256
            | I::X86AvxRoundPs256
            | I::X86Vcvtps2ph128
            | I::X86Vcvtps2ph256 => &[1],
            I::Prefetch | I::ObjectSize => &[1, 2, 3],
            I::VectorInsert
            | I::X86SseCmpSs
            | I::X86Sse2CmpSd
            | I::X86Sse41RoundSs
            | I::X86Sse41RoundSd
            | I::X86Sse41Dpps
            | I::X86Sse41Dppd
            | I::X86Sse41Insertps
            | I::X86Sse41Mpsadbw
            | I::X86Sse42Pcmpistrm128
            | I::X86Sse42Pcmpistri128
            | I::X86Sse42Pcmpistria128
            | I::X86Sse42Pcmpistric128
            | I::X86Sse42Pcmpistrio128
            | I::X86Sse42Pcmpistris128
            | I::X86Sse42Pcmpistriz128
            | I::X86Pclmulqdq
            | I::X86Sha1rnds4
            | I::X86AvxDpPs256
            | I::X86AvxCmpPd256
            | I::X86AvxCmpPs256
            | I::X86Avx2Mpsadbw
            | I::X86Vgf2p8affineqb128
            | I::X86Vgf2p8affineqb256
            | I::X86Vgf2p8affineinvqb128
            | I::X86Vgf2p8affineinvqb256 => &[2],
            I::X86Sse42Pcmpestrm128
            | I::X86Sse42Pcmpestri128
            | I::X86Sse42Pcmpestria128
            | I::X86Sse42Pcmpestric128
            | I::X86Sse42Pcmpestrio128
            | I::X86Sse42Pcmpestris128
            | I::X86Sse42Pcmpestriz128
            | I::X86Avx2GatherDD
            | I::X86Avx2GatherDD256
            | I::X86Avx2GatherDQ
            | I::X86Avx2GatherDQ256
            | I::X86Avx2GatherDPs
            | I::X86Avx2GatherDPs256
            | I::X86Avx2GatherDPd
            | I::X86Avx2GatherDPd256
            | I::X86Avx2GatherQD
            | I::X86Avx2GatherQD256
            | I::X86Avx2GatherQQ
            | I::X86Avx2GatherQQ256
            | I::X86Avx2GatherQPs
            | I::X86Avx2GatherQPs256
            | I::X86Avx2GatherQPd
            | I::X86Avx2GatherQPd256 => &[4],
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
/// of old IR). Spellings LLVM 19 and 22 still read are accepted:
/// `llvm.thread.pointer` without a suffix, which was not overloaded
/// before LLVM 21, and those of [`upgraded_va_spelling`].
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
    if !exact && !unsuffixed_thread_pointer && !upgraded_va_spelling(intrinsic, name, &overloads) {
        return other;
    }
    IntrinsicRef { intrinsic, overloads, len: len as u8 }
}

/// The spellings of the variable-argument intrinsics that LLVM 19 and 22
/// read and rename to `<base>.p0` (WP-147): the unsuffixed name of
/// `llvm.va_start`, `llvm.va_end`, `llvm.va_copy` (not overloaded before
/// LLVM 19), and `llvm.va_copy.p0.p0` (one suffix per pointer operand),
/// each of a pointer in address space 0.
fn upgraded_va_spelling(intrinsic: Intrinsic<'_>, name: &str, overloads: &[TypeId]) -> bool {
    use Intrinsic::{VaCopy, VaEnd, VaStart};
    if overloads[0] != TypeId::PTR || !matches!(intrinsic, VaStart | VaEnd | VaCopy) {
        return false;
    }
    name == intrinsic.name() || (intrinsic == VaCopy && name == "llvm.va_copy.p0.p0")
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

/// Whether `name` is `llvm.lifetime.start` (`Some(true)`) or
/// `llvm.lifetime.end` (`Some(false)`): calls [`no_code_intrinsic`] drops,
/// which a producer records as a lifetime marker on their pointer
/// argument (`FunctionBuilder::lifetime_marker`, §5.2).
pub fn lifetime_intrinsic(name: &str) -> Option<bool> {
    if has_dotted_prefix(name, "llvm.lifetime.start") {
        Some(true)
    } else if has_dotted_prefix(name, "llvm.lifetime.end") {
        Some(false)
    } else {
        None
    }
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

        // WP-147: the variable-argument spellings LLVM 19 and 22 rename
        // to `.p0`.
        let one = t.function(TypeId::VOID, &[ptr], false).unwrap();
        let two = t.function(TypeId::VOID, &[ptr, ptr], false).unwrap();
        for (name, ty, want) in [
            ("llvm.va_copy.p0.p0", two, Intrinsic::VaCopy),
            ("llvm.va_copy", two, Intrinsic::VaCopy),
            ("llvm.va_start", one, Intrinsic::VaStart),
            ("llvm.va_end", one, Intrinsic::VaEnd),
        ] {
            let r = resolve_intrinsic(t, name, ty);
            assert_eq!((r.intrinsic, r.overloads()), (want, &[ptr][..]), "{name}");
        }
        assert!(matches!(
            resolve_intrinsic(t, "llvm.va_end.p0.p0", one).intrinsic,
            Intrinsic::Other(_)
        ));
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
            ("llvm.x86.sse3.monitor", memcpy),
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
    fn lifetime_names() {
        assert_eq!(lifetime_intrinsic("llvm.lifetime.start.p0"), Some(true));
        assert_eq!(lifetime_intrinsic("llvm.lifetime.end.p0"), Some(false));
        assert_eq!(lifetime_intrinsic("llvm.lifetime.end"), Some(false));
        assert_eq!(lifetime_intrinsic("llvm.lifetime.startx"), None);
        assert_eq!(lifetime_intrinsic("llvm.invariant.start.p0"), None);
    }

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
