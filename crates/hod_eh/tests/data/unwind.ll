; ModuleID = 'unwind.cpp'
source_filename = "unwind.cpp"
target datalayout = "e-m:e-i8:8:32-i16:16:32-i64:64-i128:128-n32:64-S128-Fn32"
target triple = "aarch64-unknown-linux-gnu"

%struct.Error = type { i32 }
%struct.Guard = type { ptr }

$_ZN5GuardD2Ev = comdat any

$_Z9templatedIiET_S0_ = comdat any

$__clang_call_terminate = comdat any

$_ZTS5Error = comdat any

$_ZTI5Error = comdat any

@_ZTVN10__cxxabiv117__class_type_infoE = external global [0 x ptr]
@_ZTS5Error = linkonce_odr dso_local constant [7 x i8] c"5Error\00", comdat, align 1
@_ZTI5Error = linkonce_odr dso_local constant { ptr, ptr } { ptr getelementptr inbounds (ptr, ptr @_ZTVN10__cxxabiv117__class_type_infoE, i64 2), ptr @_ZTS5Error }, comdat, align 8
@.str = private unnamed_addr constant [6 x i8] c"large\00", align 1
@.str.1 = private unnamed_addr constant [15 x i8] c"rethrowing %d\0A\00", align 1
@.str.2 = private unnamed_addr constant [13 x i8] c"not reached\0A\00", align 1
@.str.3 = private unnamed_addr constant [11 x i8] c"caught %d\0A\00", align 1
@.str.4 = private unnamed_addr constant [10 x i8] c"no throw\0A\00", align 1
@.str.5 = private unnamed_addr constant [12 x i8] c"cleanup %s\0A\00", align 1
@.str.6 = private unnamed_addr constant [9 x i8] c"template\00", align 1

; Function Attrs: mustprogress noinline optnone uwtable
define dso_local void @_Z7throweri(i32 noundef %x) #0 {
entry:
  %x.addr = alloca i32, align 4
  store i32 %x, ptr %x.addr, align 4
  %0 = load i32, ptr %x.addr, align 4
  %cmp = icmp sgt i32 %0, 0
  br i1 %cmp, label %if.then, label %if.end

if.then:                                          ; preds = %entry
  %exception = call ptr @__cxa_allocate_exception(i64 4) #7
  %value = getelementptr inbounds %struct.Error, ptr %exception, i32 0, i32 0
  %1 = load i32, ptr %x.addr, align 4
  store i32 %1, ptr %value, align 16
  call void @__cxa_throw(ptr %exception, ptr @_ZTI5Error, ptr null) #8
  unreachable

if.end:                                           ; preds = %entry
  ret void
}

declare ptr @__cxa_allocate_exception(i64)

declare void @__cxa_throw(ptr, ptr, ptr)

; Function Attrs: mustprogress noinline optnone uwtable
define dso_local noundef i32 @_Z11large_framei(i32 noundef %x) #0 personality ptr @__gxx_personality_v0 {
entry:
  %x.addr = alloca i32, align 4
  %buffer = alloca [70000 x i8], align 1
  %g = alloca %struct.Guard, align 8
  %exn.slot = alloca ptr, align 8
  %ehselector.slot = alloca i32, align 4
  store i32 %x, ptr %x.addr, align 4
  %0 = load i32, ptr %x.addr, align 4
  %conv = trunc i32 %0 to i8
  %1 = load i32, ptr %x.addr, align 4
  %idxprom = sext i32 %1 to i64
  %arrayidx = getelementptr inbounds [70000 x i8], ptr %buffer, i64 0, i64 %idxprom
  store volatile i8 %conv, ptr %arrayidx, align 1
  %name = getelementptr inbounds %struct.Guard, ptr %g, i32 0, i32 0
  store ptr @.str, ptr %name, align 8
  %2 = load i32, ptr %x.addr, align 4
  invoke void @_Z7throweri(i32 noundef %2)
          to label %invoke.cont unwind label %lpad

invoke.cont:                                      ; preds = %entry
  %arrayidx1 = getelementptr inbounds [70000 x i8], ptr %buffer, i64 0, i64 0
  %3 = load volatile i8, ptr %arrayidx1, align 1
  %conv2 = zext i8 %3 to i32
  call void @_ZN5GuardD2Ev(ptr noundef nonnull align 8 dereferenceable(8) %g) #7
  ret i32 %conv2

lpad:                                             ; preds = %entry
  %4 = landingpad { ptr, i32 }
          cleanup
  %5 = extractvalue { ptr, i32 } %4, 0
  store ptr %5, ptr %exn.slot, align 8
  %6 = extractvalue { ptr, i32 } %4, 1
  store i32 %6, ptr %ehselector.slot, align 4
  call void @_ZN5GuardD2Ev(ptr noundef nonnull align 8 dereferenceable(8) %g) #7
  br label %eh.resume

eh.resume:                                        ; preds = %lpad
  %exn = load ptr, ptr %exn.slot, align 8
  %sel = load i32, ptr %ehselector.slot, align 4
  %lpad.val = insertvalue { ptr, i32 } poison, ptr %exn, 0
  %lpad.val3 = insertvalue { ptr, i32 } %lpad.val, i32 %sel, 1
  resume { ptr, i32 } %lpad.val3
}

declare i32 @__gxx_personality_v0(...)

; Function Attrs: mustprogress noinline nounwind optnone uwtable
define linkonce_odr dso_local void @_ZN5GuardD2Ev(ptr noundef nonnull align 8 dereferenceable(8) %this) unnamed_addr #1 comdat personality ptr @__gxx_personality_v0 {
entry:
  %this.addr = alloca ptr, align 8
  store ptr %this, ptr %this.addr, align 8
  %this1 = load ptr, ptr %this.addr, align 8
  %name = getelementptr inbounds %struct.Guard, ptr %this1, i32 0, i32 0
  %0 = load ptr, ptr %name, align 8
  %call = invoke i32 (ptr, ...) @printf(ptr noundef @.str.5, ptr noundef %0)
          to label %invoke.cont unwind label %terminate.lpad

invoke.cont:                                      ; preds = %entry
  ret void

terminate.lpad:                                   ; preds = %entry
  %1 = landingpad { ptr, i32 }
          catch ptr null
  %2 = extractvalue { ptr, i32 } %1, 0
  call void @__clang_call_terminate(ptr %2) #9
  unreachable
}

; Function Attrs: mustprogress noinline optnone uwtable
define dso_local noundef i32 @_Z13dynamic_framei(i32 noundef %n) #0 {
entry:
  %n.addr = alloca i32, align 4
  %p = alloca ptr, align 8
  %r = alloca i32, align 4
  store i32 %n, ptr %n.addr, align 4
  %0 = load i32, ptr %n.addr, align 4
  %mul = mul nsw i32 %0, 16
  %conv = sext i32 %mul to i64
  %1 = alloca i8, i64 %conv, align 16
  store ptr %1, ptr %p, align 8
  %2 = load ptr, ptr %p, align 8
  %arrayidx = getelementptr inbounds i8, ptr %2, i64 0
  store i8 1, ptr %arrayidx, align 1
  %3 = load i32, ptr %n.addr, align 4
  %call = call noundef i32 @_Z11large_framei(i32 noundef %3)
  store i32 %call, ptr %r, align 4
  %4 = load i32, ptr %r, align 4
  %5 = load ptr, ptr %p, align 8
  %arrayidx1 = getelementptr inbounds i8, ptr %5, i64 0
  %6 = load i8, ptr %arrayidx1, align 1
  %conv2 = zext i8 %6 to i32
  %add = add nsw i32 %4, %conv2
  ret i32 %add
}

; Function Attrs: mustprogress noinline optnone uwtable
define dso_local noundef i32 @_Z9rethroweri(i32 noundef %n) #0 personality ptr @__gxx_personality_v0 {
entry:
  %n.addr = alloca i32, align 4
  %exn.slot = alloca ptr, align 8
  %ehselector.slot = alloca i32, align 4
  %e = alloca ptr, align 8
  store i32 %n, ptr %n.addr, align 4
  %0 = load i32, ptr %n.addr, align 4
  %call = invoke noundef i32 @_Z9templatedIiET_S0_(i32 noundef %0)
          to label %invoke.cont unwind label %lpad

invoke.cont:                                      ; preds = %entry
  ret i32 %call

lpad:                                             ; preds = %entry
  %1 = landingpad { ptr, i32 }
          catch ptr @_ZTI5Error
  %2 = extractvalue { ptr, i32 } %1, 0
  store ptr %2, ptr %exn.slot, align 8
  %3 = extractvalue { ptr, i32 } %1, 1
  store i32 %3, ptr %ehselector.slot, align 4
  br label %catch.dispatch

catch.dispatch:                                   ; preds = %lpad
  %sel = load i32, ptr %ehselector.slot, align 4
  %4 = call i32 @llvm.eh.typeid.for.p0(ptr @_ZTI5Error) #7
  %matches = icmp eq i32 %sel, %4
  br i1 %matches, label %catch, label %eh.resume

catch:                                            ; preds = %catch.dispatch
  %exn = load ptr, ptr %exn.slot, align 8
  %5 = call ptr @__cxa_begin_catch(ptr %exn) #7
  store ptr %5, ptr %e, align 8
  %6 = load ptr, ptr %e, align 8
  %value = getelementptr inbounds %struct.Error, ptr %6, i32 0, i32 0
  %7 = load i32, ptr %value, align 4
  %call3 = invoke i32 (ptr, ...) @printf(ptr noundef @.str.1, i32 noundef %7)
          to label %invoke.cont2 unwind label %lpad1

invoke.cont2:                                     ; preds = %catch
  %8 = load ptr, ptr %e, align 8
  %value4 = getelementptr inbounds %struct.Error, ptr %8, i32 0, i32 0
  %9 = load i32, ptr %value4, align 4
  %add = add nsw i32 %9, 100
  store i32 %add, ptr %value4, align 4
  invoke void @__cxa_rethrow() #8
          to label %unreachable unwind label %lpad1

lpad1:                                            ; preds = %invoke.cont2, %catch
  %10 = landingpad { ptr, i32 }
          cleanup
  %11 = extractvalue { ptr, i32 } %10, 0
  store ptr %11, ptr %exn.slot, align 8
  %12 = extractvalue { ptr, i32 } %10, 1
  store i32 %12, ptr %ehselector.slot, align 4
  invoke void @__cxa_end_catch()
          to label %invoke.cont5 unwind label %terminate.lpad

invoke.cont5:                                     ; preds = %lpad1
  br label %eh.resume

try.cont:                                         ; No predecessors!
  call void @llvm.trap()
  unreachable

eh.resume:                                        ; preds = %invoke.cont5, %catch.dispatch
  %exn6 = load ptr, ptr %exn.slot, align 8
  %sel7 = load i32, ptr %ehselector.slot, align 4
  %lpad.val = insertvalue { ptr, i32 } poison, ptr %exn6, 0
  %lpad.val8 = insertvalue { ptr, i32 } %lpad.val, i32 %sel7, 1
  resume { ptr, i32 } %lpad.val8

terminate.lpad:                                   ; preds = %lpad1
  %13 = landingpad { ptr, i32 }
          catch ptr null
  %14 = extractvalue { ptr, i32 } %13, 0
  call void @__clang_call_terminate(ptr %14) #9
  unreachable

unreachable:                                      ; preds = %invoke.cont2
  unreachable
}

; Function Attrs: mustprogress noinline optnone uwtable
define linkonce_odr dso_local noundef i32 @_Z9templatedIiET_S0_(i32 noundef %n) #0 comdat personality ptr @__gxx_personality_v0 {
entry:
  %n.addr = alloca i32, align 4
  %g = alloca %struct.Guard, align 8
  %exn.slot = alloca ptr, align 8
  %ehselector.slot = alloca i32, align 4
  store i32 %n, ptr %n.addr, align 4
  %name = getelementptr inbounds %struct.Guard, ptr %g, i32 0, i32 0
  store ptr @.str.6, ptr %name, align 8
  %0 = load i32, ptr %n.addr, align 4
  %call = invoke noundef i32 @_Z13dynamic_framei(i32 noundef %0)
          to label %invoke.cont unwind label %lpad

invoke.cont:                                      ; preds = %entry
  call void @_ZN5GuardD2Ev(ptr noundef nonnull align 8 dereferenceable(8) %g) #7
  ret i32 %call

lpad:                                             ; preds = %entry
  %1 = landingpad { ptr, i32 }
          cleanup
  %2 = extractvalue { ptr, i32 } %1, 0
  store ptr %2, ptr %exn.slot, align 8
  %3 = extractvalue { ptr, i32 } %1, 1
  store i32 %3, ptr %ehselector.slot, align 4
  call void @_ZN5GuardD2Ev(ptr noundef nonnull align 8 dereferenceable(8) %g) #7
  br label %eh.resume

eh.resume:                                        ; preds = %lpad
  %exn = load ptr, ptr %exn.slot, align 8
  %sel = load i32, ptr %ehselector.slot, align 4
  %lpad.val = insertvalue { ptr, i32 } poison, ptr %exn, 0
  %lpad.val1 = insertvalue { ptr, i32 } %lpad.val, i32 %sel, 1
  resume { ptr, i32 } %lpad.val1
}

; Function Attrs: nounwind memory(none)
declare i32 @llvm.eh.typeid.for.p0(ptr) #2

declare ptr @__cxa_begin_catch(ptr)

declare i32 @printf(ptr noundef, ...) #3

declare void @__cxa_rethrow()

declare void @__cxa_end_catch()

; Function Attrs: noinline noreturn nounwind uwtable
define linkonce_odr hidden void @__clang_call_terminate(ptr noundef %0) #4 comdat {
  %2 = call ptr @__cxa_begin_catch(ptr %0) #7
  call void @_ZSt9terminatev() #9
  unreachable
}

declare void @_ZSt9terminatev()

; Function Attrs: cold noreturn nounwind memory(inaccessiblemem: write)
declare void @llvm.trap() #5

; Function Attrs: mustprogress noinline norecurse optnone uwtable
define dso_local noundef i32 @main() #6 personality ptr @__gxx_personality_v0 {
entry:
  %retval = alloca i32, align 4
  %i = alloca i32, align 4
  %exn.slot = alloca ptr, align 8
  %ehselector.slot = alloca i32, align 4
  %e = alloca ptr, align 8
  store i32 0, ptr %retval, align 4
  store i32 1, ptr %i, align 4
  br label %for.cond

for.cond:                                         ; preds = %for.inc, %entry
  %0 = load i32, ptr %i, align 4
  %cmp = icmp sle i32 %0, 3
  br i1 %cmp, label %for.body, label %for.end

for.body:                                         ; preds = %for.cond
  %1 = load i32, ptr %i, align 4
  %call = invoke noundef i32 @_Z9rethroweri(i32 noundef %1)
          to label %invoke.cont unwind label %lpad

invoke.cont:                                      ; preds = %for.body
  %call2 = invoke i32 (ptr, ...) @printf(ptr noundef @.str.2)
          to label %invoke.cont1 unwind label %lpad

invoke.cont1:                                     ; preds = %invoke.cont
  br label %try.cont

lpad:                                             ; preds = %invoke.cont, %for.body
  %2 = landingpad { ptr, i32 }
          catch ptr @_ZTI5Error
  %3 = extractvalue { ptr, i32 } %2, 0
  store ptr %3, ptr %exn.slot, align 8
  %4 = extractvalue { ptr, i32 } %2, 1
  store i32 %4, ptr %ehselector.slot, align 4
  br label %catch.dispatch

catch.dispatch:                                   ; preds = %lpad
  %sel = load i32, ptr %ehselector.slot, align 4
  %5 = call i32 @llvm.eh.typeid.for.p0(ptr @_ZTI5Error) #7
  %matches = icmp eq i32 %sel, %5
  br i1 %matches, label %catch, label %eh.resume

catch:                                            ; preds = %catch.dispatch
  %exn = load ptr, ptr %exn.slot, align 8
  %6 = call ptr @__cxa_begin_catch(ptr %exn) #7
  store ptr %6, ptr %e, align 8
  %7 = load ptr, ptr %e, align 8
  %value = getelementptr inbounds %struct.Error, ptr %7, i32 0, i32 0
  %8 = load i32, ptr %value, align 4
  %call5 = invoke i32 (ptr, ...) @printf(ptr noundef @.str.3, i32 noundef %8)
          to label %invoke.cont4 unwind label %lpad3

invoke.cont4:                                     ; preds = %catch
  call void @__cxa_end_catch()
  br label %try.cont

try.cont:                                         ; preds = %invoke.cont4, %invoke.cont1
  br label %for.inc

for.inc:                                          ; preds = %try.cont
  %9 = load i32, ptr %i, align 4
  %inc = add nsw i32 %9, 1
  store i32 %inc, ptr %i, align 4
  br label %for.cond, !llvm.loop !6

lpad3:                                            ; preds = %catch
  %10 = landingpad { ptr, i32 }
          cleanup
  %11 = extractvalue { ptr, i32 } %10, 0
  store ptr %11, ptr %exn.slot, align 8
  %12 = extractvalue { ptr, i32 } %10, 1
  store i32 %12, ptr %ehselector.slot, align 4
  invoke void @__cxa_end_catch()
          to label %invoke.cont6 unwind label %terminate.lpad

invoke.cont6:                                     ; preds = %lpad3
  br label %eh.resume

for.end:                                          ; preds = %for.cond
  invoke void @_Z7throweri(i32 noundef 0)
          to label %invoke.cont8 unwind label %lpad7

invoke.cont8:                                     ; preds = %for.end
  %call10 = invoke i32 (ptr, ...) @printf(ptr noundef @.str.4)
          to label %invoke.cont9 unwind label %lpad7

invoke.cont9:                                     ; preds = %invoke.cont8
  br label %try.cont17

lpad7:                                            ; preds = %invoke.cont8, %for.end
  %13 = landingpad { ptr, i32 }
          catch ptr null
  %14 = extractvalue { ptr, i32 } %13, 0
  store ptr %14, ptr %exn.slot, align 8
  %15 = extractvalue { ptr, i32 } %13, 1
  store i32 %15, ptr %ehselector.slot, align 4
  br label %catch11

catch11:                                          ; preds = %lpad7
  %exn12 = load ptr, ptr %exn.slot, align 8
  %16 = call ptr @__cxa_begin_catch(ptr %exn12) #7
  %call15 = invoke i32 (ptr, ...) @printf(ptr noundef @.str.2)
          to label %invoke.cont14 unwind label %lpad13

invoke.cont14:                                    ; preds = %catch11
  call void @__cxa_end_catch()
  br label %try.cont17

try.cont17:                                       ; preds = %invoke.cont14, %invoke.cont9
  ret i32 0

lpad13:                                           ; preds = %catch11
  %17 = landingpad { ptr, i32 }
          cleanup
  %18 = extractvalue { ptr, i32 } %17, 0
  store ptr %18, ptr %exn.slot, align 8
  %19 = extractvalue { ptr, i32 } %17, 1
  store i32 %19, ptr %ehselector.slot, align 4
  invoke void @__cxa_end_catch()
          to label %invoke.cont16 unwind label %terminate.lpad

invoke.cont16:                                    ; preds = %lpad13
  br label %eh.resume

eh.resume:                                        ; preds = %invoke.cont16, %invoke.cont6, %catch.dispatch
  %exn18 = load ptr, ptr %exn.slot, align 8
  %sel19 = load i32, ptr %ehselector.slot, align 4
  %lpad.val = insertvalue { ptr, i32 } poison, ptr %exn18, 0
  %lpad.val20 = insertvalue { ptr, i32 } %lpad.val, i32 %sel19, 1
  resume { ptr, i32 } %lpad.val20

terminate.lpad:                                   ; preds = %lpad13, %lpad3
  %20 = landingpad { ptr, i32 }
          catch ptr null
  %21 = extractvalue { ptr, i32 } %20, 0
  call void @__clang_call_terminate(ptr %21) #9
  unreachable
}

attributes #0 = { mustprogress noinline optnone uwtable "frame-pointer"="non-leaf" "no-trapping-math"="true" "stack-protector-buffer-size"="8" "target-cpu"="generic" "target-features"="+fp-armv8,+neon,+outline-atomics,+v8a,-fmv" }
attributes #1 = { mustprogress noinline nounwind optnone uwtable "frame-pointer"="non-leaf" "no-trapping-math"="true" "stack-protector-buffer-size"="8" "target-cpu"="generic" "target-features"="+fp-armv8,+neon,+outline-atomics,+v8a,-fmv" }
attributes #2 = { nounwind memory(none) }
attributes #3 = { "frame-pointer"="non-leaf" "no-trapping-math"="true" "stack-protector-buffer-size"="8" "target-cpu"="generic" "target-features"="+fp-armv8,+neon,+outline-atomics,+v8a,-fmv" }
attributes #4 = { noinline noreturn nounwind uwtable "frame-pointer"="non-leaf" "no-trapping-math"="true" "stack-protector-buffer-size"="8" "target-cpu"="generic" "target-features"="+fp-armv8,+neon,+outline-atomics,+v8a,-fmv" }
attributes #5 = { cold noreturn nounwind memory(inaccessiblemem: write) }
attributes #6 = { mustprogress noinline norecurse optnone uwtable "frame-pointer"="non-leaf" "no-trapping-math"="true" "stack-protector-buffer-size"="8" "target-cpu"="generic" "target-features"="+fp-armv8,+neon,+outline-atomics,+v8a,-fmv" }
attributes #7 = { nounwind }
attributes #8 = { noreturn }
attributes #9 = { noreturn nounwind }

!llvm.module.flags = !{!0, !1, !2, !3, !4}
!llvm.ident = !{!5}

!0 = !{i32 1, !"wchar_size", i32 4}
!1 = !{i32 8, !"PIC Level", i32 2}
!2 = !{i32 7, !"PIE Level", i32 2}
!3 = !{i32 7, !"uwtable", i32 2}
!4 = !{i32 7, !"frame-pointer", i32 1}
!5 = !{!"Debian clang version 19.1.7 (3+b1)"}
!6 = distinct !{!6, !7}
!7 = !{!"llvm.loop.mustprogress"}
