; The casts: trunc, zext, sext, fptrunc, fpext, fptoui, fptosi, uitofp,
; sitofp, ptrtoint, inttoptr and bitcast, each with an operand that
; arrives as an argument. No conversion here is out of range.
;@ covers: spec/ir.md §5.4 cast
;@ unsupported(aarch64): global:function-definition
;@ unsupported(x86_64): global:variable-definition
;@ stdout: trunc: i32=-1698898192 i16=-8464/57072 i8=-16/240 i1=0
;@ stdout: trunc: i32=-1 i16=-1/65535 i8=-1/255 i1=1
;@ stdout: ext: sext=-2 zext=4294967294 sext8=-128 zext8=128 sext1=-1
;@ stdout: ext: sext=2147483647 zext=2147483647 sext8=127 zext8=127 sext1=0
;@ stdout: fp: fptrunc=0.10000000149011612 fptosi=-2 fptoui=3 wide=1000000000000000 uwide=18000000000000000000
;@ stdout: itof: sitofp=-7.0 uitofp=4294967289.0 s64=-1.0 u64=18446744073709551616.0 f32=16777216.0
;@ stdout: bits: double=3ff0000000000000 float=c0000000 back=3.141593
;@ stdout: ptr: element=30 distance=12 same=1

@.trunc = private unnamed_addr constant [40 x i8] c"trunc: i32=%d i16=%d/%u i8=%d/%u i1=%d\0A\00"
@.ext = private unnamed_addr constant [53 x i8] c"ext: sext=%lld zext=%lld sext8=%d zext8=%d sext1=%d\0A\00"
@.fp = private unnamed_addr constant [60 x i8] c"fp: fptrunc=%.17g fptosi=%d fptoui=%u wide=%lld uwide=%llu\0A\00"
@.itof = private unnamed_addr constant [58 x i8] c"itof: sitofp=%.1f uitofp=%.1f s64=%.1f u64=%.1f f32=%.1f\0A\00"
@.bits = private unnamed_addr constant [38 x i8] c"bits: double=%llx float=%x back=%.6f\0A\00"
@.ptr = private unnamed_addr constant [39 x i8] c"ptr: element=%d distance=%lld same=%d\0A\00"
@table = global [4 x i32] [i32 10, i32 20, i32 30, i32 40]

declare i32 @printf(ptr, ...)

define void @truncs(i64 %x) {
  %t32 = trunc i64 %x to i32
  %t16 = trunc i64 %x to i16
  %t8 = trunc i64 %x to i8
  %t1 = trunc i64 %x to i1
  %s16 = sext i16 %t16 to i32
  %z16 = zext i16 %t16 to i32
  %s8 = sext i8 %t8 to i32
  %z8 = zext i8 %t8 to i32
  %z1 = zext i1 %t1 to i32
  call i32 (ptr, ...) @printf(ptr @.trunc, i32 %t32, i32 %s16, i32 %z16, i32 %s8, i32 %z8, i32 %z1)
  ret void
}

define void @exts(i32 %x, i8 %b, i1 %bit) {
  %s = sext i32 %x to i64
  %z = zext i32 %x to i64
  %s8 = sext i8 %b to i32
  %z8 = zext i8 %b to i32
  %s1 = sext i1 %bit to i32
  call i32 (ptr, ...) @printf(ptr @.ext, i64 %s, i64 %z, i32 %s8, i32 %z8, i32 %s1)
  ret void
}

define void @fps(double %tenth, double %neg, double %pos, double %big, double %huge) {
  %f = fptrunc double %tenth to float
  %back = fpext float %f to double
  %si = fptosi double %neg to i32
  %ui = fptoui double %pos to i32
  %wide = fptosi double %big to i64
  %uwide = fptoui double %huge to i64
  call i32 (ptr, ...) @printf(ptr @.fp, double %back, i32 %si, i32 %ui, i64 %wide, i64 %uwide)
  ret void
}

define void @itofs(i32 %x, i64 %y, i64 %odd) {
  %s = sitofp i32 %x to double
  %u = uitofp i32 %x to double
  %s64 = sitofp i64 %y to double
  %u64 = uitofp i64 %y to double
  %f = uitofp i64 %odd to float
  %f.d = fpext float %f to double
  call i32 (ptr, ...) @printf(ptr @.itof, double %s, double %u, double %s64, double %u64, double %f.d)
  ret void
}

define void @bits(double %d, float %f, i32 %pi) {
  %db = bitcast double %d to i64
  %fb = bitcast float %f to i32
  %back = bitcast i32 %pi to float
  %back.d = fpext float %back to double
  call i32 (ptr, ...) @printf(ptr @.bits, i64 %db, i32 %fb, double %back.d)
  ret void
}

define void @ptrs(ptr %base, ptr %third) {
  %b = ptrtoint ptr %base to i64
  %t = ptrtoint ptr %third to i64
  %distance = sub i64 %t, %b
  %addr = add i64 %b, 8
  %p = inttoptr i64 %addr to ptr
  %element = load i32, ptr %p
  %again = inttoptr i64 %b to ptr
  %same = icmp eq ptr %again, %base
  %same.i = zext i1 %same to i32
  call i32 (ptr, ...) @printf(ptr @.ptr, i32 %element, i64 %distance, i32 %same.i)
  ret void
}

define i32 @main() {
  call void @truncs(i64 1311768467463790320)
  call void @truncs(i64 -1)
  call void @exts(i32 -2, i8 -128, i1 true)
  call void @exts(i32 2147483647, i8 127, i1 false)
  call void @fps(double 0x3FB999999999999A, double -2.75, double 3.99, double 1.0e15, double 1.8e19)
  call void @itofs(i32 -7, i64 -1, i64 16777217)
  call void @bits(double 1.0, float -2.0, i32 1078530011)
  call void @ptrs(ptr @table, ptr getelementptr inbounds ([4 x i32], ptr @table, i64 0, i64 3))
  ret i32 0
}
