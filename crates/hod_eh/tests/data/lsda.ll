; ModuleID = 'lsda.cpp'
source_filename = "lsda.cpp"
target datalayout = "e-m:e-i8:8:32-i16:16:32-i64:64-i128:128-n32:64-S128-Fn32"
target triple = "aarch64-unknown-linux-gnu"

%struct.Base = type { i32 }
%struct.Guard = type { ptr }

$_ZN5ErrorC2Ei = comdat any

$__clang_call_terminate = comdat any

$_ZN5GuardD2Ev = comdat any

$_ZTS5Error = comdat any

$_ZTS4Base = comdat any

$_ZTI4Base = comdat any

$_ZTI5Error = comdat any

@_ZTIi = external constant ptr
@_ZTVN10__cxxabiv120__si_class_type_infoE = external global [0 x ptr]
@_ZTS5Error = linkonce_odr dso_local constant [7 x i8] c"5Error\00", comdat, align 1
@_ZTVN10__cxxabiv117__class_type_infoE = external global [0 x ptr]
@_ZTS4Base = linkonce_odr dso_local constant [6 x i8] c"4Base\00", comdat, align 1
@_ZTI4Base = linkonce_odr dso_local constant { ptr, ptr } { ptr getelementptr inbounds (ptr, ptr @_ZTVN10__cxxabiv117__class_type_infoE, i64 2), ptr @_ZTS4Base }, comdat, align 8
@_ZTI5Error = linkonce_odr dso_local constant { ptr, ptr, ptr } { ptr getelementptr inbounds (ptr, ptr @_ZTVN10__cxxabiv120__si_class_type_infoE, i64 2), ptr @_ZTS5Error, ptr @_ZTI4Base }, comdat, align 8
@_ZTIl = external constant ptr
@.str = private unnamed_addr constant [13 x i8] c"wrong: Base\0A\00", align 1
@.str.1 = private unnamed_addr constant [15 x i8] c"caught int %d\0A\00", align 1
@.str.2 = private unnamed_addr constant [13 x i8] c"wrong: long\0A\00", align 1
@.str.3 = private unnamed_addr constant [6 x i8] c"inner\00", align 1
@_ZTId = external constant ptr
@.str.4 = private unnamed_addr constant [15 x i8] c"wrong: double\0A\00", align 1
@.str.5 = private unnamed_addr constant [12 x i8] c"wrong: int\0A\00", align 1
@.str.6 = private unnamed_addr constant [16 x i8] c"caught Base %d\0A\00", align 1
@.str.7 = private unnamed_addr constant [8 x i8] c"handler\00", align 1
@.str.8 = private unnamed_addr constant [15 x i8] c"rethrowing %d\0A\00", align 1
@.str.9 = private unnamed_addr constant [12 x i8] c"caught all\0A\00", align 1
@.str.10 = private unnamed_addr constant [8 x i8] c"allowed\00", align 1
@.str.11 = private unnamed_addr constant [34 x i8] c"caught Error %d through a filter\0A\00", align 1
@.str.12 = private unnamed_addr constant [27 x i8] c"caught int %d in two_pads\0A\00", align 1
@.str.13 = private unnamed_addr constant [29 x i8] c"caught Error %d in two_pads\0A\00", align 1
@.str.14 = private unnamed_addr constant [7 x i8] c"filter\00", align 1
@.str.15 = private unnamed_addr constant [20 x i8] c"violating a filter\0A\00", align 1
@stdout = external global ptr, align 8
@.str.16 = private unnamed_addr constant [9 x i8] c"noexcept\00", align 1
@.str.17 = private unnamed_addr constant [26 x i8] c"throwing out of noexcept\0A\00", align 1
@.str.18 = private unnamed_addr constant [6 x i8] c"done\0A\00", align 1
@.str.19 = private unnamed_addr constant [12 x i8] c"cleanup %s\0A\00", align 1

; Function Attrs: mustprogress noinline optnone uwtable
define dso_local void @_Z9throw_inti(i32 noundef %x) #0 {
entry:
  %x.addr = alloca i32, align 4
  store i32 %x, ptr %x.addr, align 4
  %0 = load i32, ptr %x.addr, align 4
  %cmp = icmp sgt i32 %0, 0
  br i1 %cmp, label %if.then, label %if.end

if.then:                                          ; preds = %entry
  %exception = call ptr @__cxa_allocate_exception(i64 4) #7
  %1 = load i32, ptr %x.addr, align 4
  store i32 %1, ptr %exception, align 16
  call void @__cxa_throw(ptr %exception, ptr @_ZTIi, ptr null) #8
  unreachable

if.end:                                           ; preds = %entry
  ret void
}

declare ptr @__cxa_allocate_exception(i64)

declare void @__cxa_throw(ptr, ptr, ptr)

; Function Attrs: mustprogress noinline optnone uwtable
define dso_local void @_Z11throw_errori(i32 noundef %x) #0 personality ptr @__gxx_personality_v0 {
entry:
  %x.addr = alloca i32, align 4
  %exn.slot = alloca ptr, align 8
  %ehselector.slot = alloca i32, align 4
  store i32 %x, ptr %x.addr, align 4
  %0 = load i32, ptr %x.addr, align 4
  %cmp = icmp sgt i32 %0, 0
  br i1 %cmp, label %if.then, label %if.end

if.then:                                          ; preds = %entry
  %exception = call ptr @__cxa_allocate_exception(i64 4) #7
  %1 = load i32, ptr %x.addr, align 4
  invoke void @_ZN5ErrorC2Ei(ptr noundef nonnull align 4 dereferenceable(4) %exception, i32 noundef %1)
          to label %invoke.cont unwind label %lpad

invoke.cont:                                      ; preds = %if.then
  call void @__cxa_throw(ptr %exception, ptr @_ZTI5Error, ptr null) #8
  unreachable

lpad:                                             ; preds = %if.then
  %2 = landingpad { ptr, i32 }
          cleanup
  %3 = extractvalue { ptr, i32 } %2, 0
  store ptr %3, ptr %exn.slot, align 8
  %4 = extractvalue { ptr, i32 } %2, 1
  store i32 %4, ptr %ehselector.slot, align 4
  call void @__cxa_free_exception(ptr %exception) #7
  br label %eh.resume

if.end:                                           ; preds = %entry
  ret void

eh.resume:                                        ; preds = %lpad
  %exn = load ptr, ptr %exn.slot, align 8
  %sel = load i32, ptr %ehselector.slot, align 4
  %lpad.val = insertvalue { ptr, i32 } poison, ptr %exn, 0
  %lpad.val1 = insertvalue { ptr, i32 } %lpad.val, i32 %sel, 1
  resume { ptr, i32 } %lpad.val1
}

; Function Attrs: mustprogress noinline nounwind optnone uwtable
define linkonce_odr dso_local void @_ZN5ErrorC2Ei(ptr noundef nonnull align 4 dereferenceable(4) %this, i32 noundef %v) unnamed_addr #1 comdat {
entry:
  %this.addr = alloca ptr, align 8
  %v.addr = alloca i32, align 4
  store ptr %this, ptr %this.addr, align 8
  store i32 %v, ptr %v.addr, align 4
  %this1 = load ptr, ptr %this.addr, align 8
  %0 = load i32, ptr %v.addr, align 4
  %value = getelementptr inbounds %struct.Base, ptr %this1, i32 0, i32 0
  store i32 %0, ptr %value, align 4
  ret void
}

declare i32 @__gxx_personality_v0(...)

declare void @__cxa_free_exception(ptr)

; Function Attrs: mustprogress noinline nounwind optnone uwtable
define dso_local noundef i32 @_Z5quieti(i32 noundef %x) #1 {
entry:
  %x.addr = alloca i32, align 4
  store i32 %x, ptr %x.addr, align 4
  %0 = load i32, ptr %x.addr, align 4
  %add = add nsw i32 %0, 1
  ret i32 %add
}

; Function Attrs: mustprogress noinline optnone uwtable
define dso_local void @_Z7by_typei(i32 noundef %x) #0 personality ptr @__gxx_personality_v0 {
entry:
  %x.addr = alloca i32, align 4
  %exn.slot = alloca ptr, align 8
  %ehselector.slot = alloca i32, align 4
  %0 = alloca ptr, align 8
  %v = alloca i32, align 4
  %1 = alloca i64, align 8
  store i32 %x, ptr %x.addr, align 4
  %2 = load i32, ptr %x.addr, align 4
  invoke void @_Z9throw_inti(i32 noundef %2)
          to label %invoke.cont unwind label %lpad

invoke.cont:                                      ; preds = %entry
  br label %try.cont

lpad:                                             ; preds = %entry
  %3 = landingpad { ptr, i32 }
          catch ptr @_ZTIl
          catch ptr @_ZTIi
          catch ptr @_ZTI4Base
  %4 = extractvalue { ptr, i32 } %3, 0
  store ptr %4, ptr %exn.slot, align 8
  %5 = extractvalue { ptr, i32 } %3, 1
  store i32 %5, ptr %ehselector.slot, align 4
  br label %catch.dispatch

catch.dispatch:                                   ; preds = %lpad
  %sel = load i32, ptr %ehselector.slot, align 4
  %6 = call i32 @llvm.eh.typeid.for.p0(ptr @_ZTIl) #7
  %matches = icmp eq i32 %sel, %6
  br i1 %matches, label %catch12, label %catch.fallthrough

catch12:                                          ; preds = %catch.dispatch
  %exn13 = load ptr, ptr %exn.slot, align 8
  %7 = call ptr @__cxa_begin_catch(ptr %exn13) #7
  %8 = load i64, ptr %7, align 8
  store i64 %8, ptr %1, align 8
  %call16 = invoke i32 (ptr, ...) @printf(ptr noundef @.str.2)
          to label %invoke.cont15 unwind label %lpad14

invoke.cont15:                                    ; preds = %catch12
  call void @__cxa_end_catch() #7
  br label %try.cont

try.cont:                                         ; preds = %invoke.cont15, %invoke.cont10, %invoke.cont5, %invoke.cont
  ret void

catch.fallthrough:                                ; preds = %catch.dispatch
  %9 = call i32 @llvm.eh.typeid.for.p0(ptr @_ZTIi) #7
  %matches1 = icmp eq i32 %sel, %9
  br i1 %matches1, label %catch7, label %catch.fallthrough2

catch7:                                           ; preds = %catch.fallthrough
  %exn8 = load ptr, ptr %exn.slot, align 8
  %10 = call ptr @__cxa_begin_catch(ptr %exn8) #7
  %11 = load i32, ptr %10, align 4
  store i32 %11, ptr %v, align 4
  %12 = load i32, ptr %v, align 4
  %call11 = invoke i32 (ptr, ...) @printf(ptr noundef @.str.1, i32 noundef %12)
          to label %invoke.cont10 unwind label %lpad9

invoke.cont10:                                    ; preds = %catch7
  call void @__cxa_end_catch() #7
  br label %try.cont

catch.fallthrough2:                               ; preds = %catch.fallthrough
  %13 = call i32 @llvm.eh.typeid.for.p0(ptr @_ZTI4Base) #7
  %matches3 = icmp eq i32 %sel, %13
  br i1 %matches3, label %catch, label %eh.resume

catch:                                            ; preds = %catch.fallthrough2
  %exn = load ptr, ptr %exn.slot, align 8
  %14 = call ptr @__cxa_begin_catch(ptr %exn) #7
  store ptr %14, ptr %0, align 8
  %call = invoke i32 (ptr, ...) @printf(ptr noundef @.str)
          to label %invoke.cont5 unwind label %lpad4

invoke.cont5:                                     ; preds = %catch
  call void @__cxa_end_catch()
  br label %try.cont

lpad4:                                            ; preds = %catch
  %15 = landingpad { ptr, i32 }
          cleanup
  %16 = extractvalue { ptr, i32 } %15, 0
  store ptr %16, ptr %exn.slot, align 8
  %17 = extractvalue { ptr, i32 } %15, 1
  store i32 %17, ptr %ehselector.slot, align 4
  invoke void @__cxa_end_catch()
          to label %invoke.cont6 unwind label %terminate.lpad

invoke.cont6:                                     ; preds = %lpad4
  br label %eh.resume

lpad9:                                            ; preds = %catch7
  %18 = landingpad { ptr, i32 }
          cleanup
  %19 = extractvalue { ptr, i32 } %18, 0
  store ptr %19, ptr %exn.slot, align 8
  %20 = extractvalue { ptr, i32 } %18, 1
  store i32 %20, ptr %ehselector.slot, align 4
  call void @__cxa_end_catch() #7
  br label %eh.resume

lpad14:                                           ; preds = %catch12
  %21 = landingpad { ptr, i32 }
          cleanup
  %22 = extractvalue { ptr, i32 } %21, 0
  store ptr %22, ptr %exn.slot, align 8
  %23 = extractvalue { ptr, i32 } %21, 1
  store i32 %23, ptr %ehselector.slot, align 4
  call void @__cxa_end_catch() #7
  br label %eh.resume

eh.resume:                                        ; preds = %lpad14, %lpad9, %invoke.cont6, %catch.fallthrough2
  %exn17 = load ptr, ptr %exn.slot, align 8
  %sel18 = load i32, ptr %ehselector.slot, align 4
  %lpad.val = insertvalue { ptr, i32 } poison, ptr %exn17, 0
  %lpad.val19 = insertvalue { ptr, i32 } %lpad.val, i32 %sel18, 1
  resume { ptr, i32 } %lpad.val19

terminate.lpad:                                   ; preds = %lpad4
  %24 = landingpad { ptr, i32 }
          catch ptr null
  %25 = extractvalue { ptr, i32 } %24, 0
  call void @__clang_call_terminate(ptr %25) #9
  unreachable
}

; Function Attrs: nounwind memory(none)
declare i32 @llvm.eh.typeid.for.p0(ptr) #2

declare ptr @__cxa_begin_catch(ptr)

declare i32 @printf(ptr noundef, ...) #3

declare void @__cxa_end_catch()

; Function Attrs: noinline noreturn nounwind uwtable
define linkonce_odr hidden void @__clang_call_terminate(ptr noundef %0) #4 comdat {
  %2 = call ptr @__cxa_begin_catch(ptr %0) #7
  call void @_ZSt9terminatev() #9
  unreachable
}

declare void @_ZSt9terminatev()

; Function Attrs: mustprogress noinline optnone uwtable
define dso_local void @_Z5inneri(i32 noundef %x) #0 personality ptr @__gxx_personality_v0 {
entry:
  %x.addr = alloca i32, align 4
  %g = alloca %struct.Guard, align 8
  %exn.slot = alloca ptr, align 8
  %ehselector.slot = alloca i32, align 4
  %0 = alloca double, align 8
  %1 = alloca i32, align 4
  store i32 %x, ptr %x.addr, align 4
  %name = getelementptr inbounds %struct.Guard, ptr %g, i32 0, i32 0
  store ptr @.str.3, ptr %name, align 8
  %2 = load i32, ptr %x.addr, align 4
  %call = call noundef i32 @_Z5quieti(i32 noundef %2) #7
  %3 = load i32, ptr %x.addr, align 4
  invoke void @_Z11throw_errori(i32 noundef %3)
          to label %invoke.cont unwind label %lpad

invoke.cont:                                      ; preds = %entry
  br label %try.cont

lpad:                                             ; preds = %entry
  %4 = landingpad { ptr, i32 }
          cleanup
          catch ptr @_ZTIi
          catch ptr @_ZTId
  %5 = extractvalue { ptr, i32 } %4, 0
  store ptr %5, ptr %exn.slot, align 8
  %6 = extractvalue { ptr, i32 } %4, 1
  store i32 %6, ptr %ehselector.slot, align 4
  br label %catch.dispatch

catch.dispatch:                                   ; preds = %lpad
  %sel = load i32, ptr %ehselector.slot, align 4
  %7 = call i32 @llvm.eh.typeid.for.p0(ptr @_ZTIi) #7
  %matches = icmp eq i32 %sel, %7
  br i1 %matches, label %catch5, label %catch.fallthrough

catch5:                                           ; preds = %catch.dispatch
  %exn6 = load ptr, ptr %exn.slot, align 8
  %8 = call ptr @__cxa_begin_catch(ptr %exn6) #7
  %9 = load i32, ptr %8, align 4
  store i32 %9, ptr %1, align 4
  %call9 = invoke i32 (ptr, ...) @printf(ptr noundef @.str.5)
          to label %invoke.cont8 unwind label %lpad7

invoke.cont8:                                     ; preds = %catch5
  call void @__cxa_end_catch() #7
  br label %try.cont

try.cont:                                         ; preds = %invoke.cont8, %invoke.cont3, %invoke.cont
  call void @_ZN5GuardD2Ev(ptr noundef nonnull align 8 dereferenceable(8) %g) #7
  ret void

catch.fallthrough:                                ; preds = %catch.dispatch
  %10 = call i32 @llvm.eh.typeid.for.p0(ptr @_ZTId) #7
  %matches1 = icmp eq i32 %sel, %10
  br i1 %matches1, label %catch, label %ehcleanup

catch:                                            ; preds = %catch.fallthrough
  %exn = load ptr, ptr %exn.slot, align 8
  %11 = call ptr @__cxa_begin_catch(ptr %exn) #7
  %12 = load double, ptr %11, align 8
  store double %12, ptr %0, align 8
  %call4 = invoke i32 (ptr, ...) @printf(ptr noundef @.str.4)
          to label %invoke.cont3 unwind label %lpad2

invoke.cont3:                                     ; preds = %catch
  call void @__cxa_end_catch() #7
  br label %try.cont

lpad2:                                            ; preds = %catch
  %13 = landingpad { ptr, i32 }
          cleanup
  %14 = extractvalue { ptr, i32 } %13, 0
  store ptr %14, ptr %exn.slot, align 8
  %15 = extractvalue { ptr, i32 } %13, 1
  store i32 %15, ptr %ehselector.slot, align 4
  call void @__cxa_end_catch() #7
  br label %ehcleanup

lpad7:                                            ; preds = %catch5
  %16 = landingpad { ptr, i32 }
          cleanup
  %17 = extractvalue { ptr, i32 } %16, 0
  store ptr %17, ptr %exn.slot, align 8
  %18 = extractvalue { ptr, i32 } %16, 1
  store i32 %18, ptr %ehselector.slot, align 4
  call void @__cxa_end_catch() #7
  br label %ehcleanup

ehcleanup:                                        ; preds = %lpad7, %lpad2, %catch.fallthrough
  call void @_ZN5GuardD2Ev(ptr noundef nonnull align 8 dereferenceable(8) %g) #7
  br label %eh.resume

eh.resume:                                        ; preds = %ehcleanup
  %exn10 = load ptr, ptr %exn.slot, align 8
  %sel11 = load i32, ptr %ehselector.slot, align 4
  %lpad.val = insertvalue { ptr, i32 } poison, ptr %exn10, 0
  %lpad.val12 = insertvalue { ptr, i32 } %lpad.val, i32 %sel11, 1
  resume { ptr, i32 } %lpad.val12
}

; Function Attrs: mustprogress noinline nounwind optnone uwtable
define linkonce_odr dso_local void @_ZN5GuardD2Ev(ptr noundef nonnull align 8 dereferenceable(8) %this) unnamed_addr #1 comdat personality ptr @__gxx_personality_v0 {
entry:
  %this.addr = alloca ptr, align 8
  store ptr %this, ptr %this.addr, align 8
  %this1 = load ptr, ptr %this.addr, align 8
  %name = getelementptr inbounds %struct.Guard, ptr %this1, i32 0, i32 0
  %0 = load ptr, ptr %name, align 8
  %call = invoke i32 (ptr, ...) @printf(ptr noundef @.str.19, ptr noundef %0)
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
define dso_local void @_Z6nestedi(i32 noundef %x) #0 personality ptr @__gxx_personality_v0 {
entry:
  %x.addr = alloca i32, align 4
  %exn.slot = alloca ptr, align 8
  %ehselector.slot = alloca i32, align 4
  %0 = alloca i64, align 8
  %b = alloca ptr, align 8
  store i32 %x, ptr %x.addr, align 4
  %1 = load i32, ptr %x.addr, align 4
  invoke void @_Z5inneri(i32 noundef %1)
          to label %invoke.cont unwind label %lpad

invoke.cont:                                      ; preds = %entry
  br label %try.cont

lpad:                                             ; preds = %entry
  %2 = landingpad { ptr, i32 }
          catch ptr @_ZTIl
          catch ptr @_ZTI4Base
  %3 = extractvalue { ptr, i32 } %2, 0
  store ptr %3, ptr %exn.slot, align 8
  %4 = extractvalue { ptr, i32 } %2, 1
  store i32 %4, ptr %ehselector.slot, align 4
  br label %catch.dispatch

catch.dispatch:                                   ; preds = %lpad
  %sel = load i32, ptr %ehselector.slot, align 4
  %5 = call i32 @llvm.eh.typeid.for.p0(ptr @_ZTIl) #7
  %matches = icmp eq i32 %sel, %5
  br i1 %matches, label %catch, label %catch.dispatch3

catch:                                            ; preds = %catch.dispatch
  %exn = load ptr, ptr %exn.slot, align 8
  %6 = call ptr @__cxa_begin_catch(ptr %exn) #7
  %7 = load i64, ptr %6, align 8
  store i64 %7, ptr %0, align 8
  %call = invoke i32 (ptr, ...) @printf(ptr noundef @.str.2)
          to label %invoke.cont2 unwind label %lpad1

invoke.cont2:                                     ; preds = %catch
  call void @__cxa_end_catch() #7
  br label %try.cont

try.cont:                                         ; preds = %invoke.cont2, %invoke.cont
  br label %try.cont12

lpad1:                                            ; preds = %catch
  %8 = landingpad { ptr, i32 }
          cleanup
          catch ptr @_ZTI4Base
  %9 = extractvalue { ptr, i32 } %8, 0
  store ptr %9, ptr %exn.slot, align 8
  %10 = extractvalue { ptr, i32 } %8, 1
  store i32 %10, ptr %ehselector.slot, align 4
  call void @__cxa_end_catch() #7
  br label %catch.dispatch3

catch.dispatch3:                                  ; preds = %lpad1, %catch.dispatch
  %sel4 = load i32, ptr %ehselector.slot, align 4
  %11 = call i32 @llvm.eh.typeid.for.p0(ptr @_ZTI4Base) #7
  %matches5 = icmp eq i32 %sel4, %11
  br i1 %matches5, label %catch6, label %eh.resume

catch6:                                           ; preds = %catch.dispatch3
  %exn7 = load ptr, ptr %exn.slot, align 8
  %12 = call ptr @__cxa_begin_catch(ptr %exn7) #7
  store ptr %12, ptr %b, align 8
  %13 = load ptr, ptr %b, align 8
  %value = getelementptr inbounds %struct.Base, ptr %13, i32 0, i32 0
  %14 = load i32, ptr %value, align 4
  %call10 = invoke i32 (ptr, ...) @printf(ptr noundef @.str.6, i32 noundef %14)
          to label %invoke.cont9 unwind label %lpad8

invoke.cont9:                                     ; preds = %catch6
  call void @__cxa_end_catch()
  br label %try.cont12

try.cont12:                                       ; preds = %invoke.cont9, %try.cont
  ret void

lpad8:                                            ; preds = %catch6
  %15 = landingpad { ptr, i32 }
          cleanup
  %16 = extractvalue { ptr, i32 } %15, 0
  store ptr %16, ptr %exn.slot, align 8
  %17 = extractvalue { ptr, i32 } %15, 1
  store i32 %17, ptr %ehselector.slot, align 4
  invoke void @__cxa_end_catch()
          to label %invoke.cont11 unwind label %terminate.lpad

invoke.cont11:                                    ; preds = %lpad8
  br label %eh.resume

eh.resume:                                        ; preds = %invoke.cont11, %catch.dispatch3
  %exn13 = load ptr, ptr %exn.slot, align 8
  %sel14 = load i32, ptr %ehselector.slot, align 4
  %lpad.val = insertvalue { ptr, i32 } poison, ptr %exn13, 0
  %lpad.val15 = insertvalue { ptr, i32 } %lpad.val, i32 %sel14, 1
  resume { ptr, i32 } %lpad.val15

terminate.lpad:                                   ; preds = %lpad8
  %18 = landingpad { ptr, i32 }
          catch ptr null
  %19 = extractvalue { ptr, i32 } %18, 0
  call void @__clang_call_terminate(ptr %19) #9
  unreachable
}

; Function Attrs: mustprogress noinline optnone uwtable
define dso_local void @_Z9rethroweri(i32 noundef %x) #0 personality ptr @__gxx_personality_v0 {
entry:
  %x.addr = alloca i32, align 4
  %exn.slot = alloca ptr, align 8
  %ehselector.slot = alloca i32, align 4
  %e = alloca ptr, align 8
  %g = alloca %struct.Guard, align 8
  store i32 %x, ptr %x.addr, align 4
  %0 = load i32, ptr %x.addr, align 4
  invoke void @_Z11throw_errori(i32 noundef %0)
          to label %invoke.cont unwind label %lpad

invoke.cont:                                      ; preds = %entry
  br label %try.cont

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
  %name = getelementptr inbounds %struct.Guard, ptr %g, i32 0, i32 0
  store ptr @.str.7, ptr %name, align 8
  %6 = load ptr, ptr %e, align 8
  %value = getelementptr inbounds %struct.Base, ptr %6, i32 0, i32 0
  %7 = load i32, ptr %value, align 4
  %call = invoke i32 (ptr, ...) @printf(ptr noundef @.str.8, i32 noundef %7)
          to label %invoke.cont2 unwind label %lpad1

invoke.cont2:                                     ; preds = %catch
  invoke void @__cxa_rethrow() #8
          to label %unreachable unwind label %lpad1

lpad1:                                            ; preds = %invoke.cont2, %catch
  %8 = landingpad { ptr, i32 }
          cleanup
  %9 = extractvalue { ptr, i32 } %8, 0
  store ptr %9, ptr %exn.slot, align 8
  %10 = extractvalue { ptr, i32 } %8, 1
  store i32 %10, ptr %ehselector.slot, align 4
  call void @_ZN5GuardD2Ev(ptr noundef nonnull align 8 dereferenceable(8) %g) #7
  invoke void @__cxa_end_catch()
          to label %invoke.cont3 unwind label %terminate.lpad

invoke.cont3:                                     ; preds = %lpad1
  br label %eh.resume

try.cont:                                         ; preds = %invoke.cont
  ret void

eh.resume:                                        ; preds = %invoke.cont3, %catch.dispatch
  %exn4 = load ptr, ptr %exn.slot, align 8
  %sel5 = load i32, ptr %ehselector.slot, align 4
  %lpad.val = insertvalue { ptr, i32 } poison, ptr %exn4, 0
  %lpad.val6 = insertvalue { ptr, i32 } %lpad.val, i32 %sel5, 1
  resume { ptr, i32 } %lpad.val6

terminate.lpad:                                   ; preds = %lpad1
  %11 = landingpad { ptr, i32 }
          catch ptr null
  %12 = extractvalue { ptr, i32 } %11, 0
  call void @__clang_call_terminate(ptr %12) #9
  unreachable

unreachable:                                      ; preds = %invoke.cont2
  unreachable
}

declare void @__cxa_rethrow()

; Function Attrs: mustprogress noinline optnone uwtable
define dso_local void @_Z9catch_alli(i32 noundef %x) #0 personality ptr @__gxx_personality_v0 {
entry:
  %x.addr = alloca i32, align 4
  %exn.slot = alloca ptr, align 8
  %ehselector.slot = alloca i32, align 4
  store i32 %x, ptr %x.addr, align 4
  %0 = load i32, ptr %x.addr, align 4
  invoke void @_Z9rethroweri(i32 noundef %0)
          to label %invoke.cont unwind label %lpad

invoke.cont:                                      ; preds = %entry
  br label %try.cont

lpad:                                             ; preds = %entry
  %1 = landingpad { ptr, i32 }
          catch ptr null
  %2 = extractvalue { ptr, i32 } %1, 0
  store ptr %2, ptr %exn.slot, align 8
  %3 = extractvalue { ptr, i32 } %1, 1
  store i32 %3, ptr %ehselector.slot, align 4
  br label %catch

catch:                                            ; preds = %lpad
  %exn = load ptr, ptr %exn.slot, align 8
  %4 = call ptr @__cxa_begin_catch(ptr %exn) #7
  %call = invoke i32 (ptr, ...) @printf(ptr noundef @.str.9)
          to label %invoke.cont2 unwind label %lpad1

invoke.cont2:                                     ; preds = %catch
  call void @__cxa_end_catch()
  br label %try.cont

try.cont:                                         ; preds = %invoke.cont2, %invoke.cont
  ret void

lpad1:                                            ; preds = %catch
  %5 = landingpad { ptr, i32 }
          cleanup
  %6 = extractvalue { ptr, i32 } %5, 0
  store ptr %6, ptr %exn.slot, align 8
  %7 = extractvalue { ptr, i32 } %5, 1
  store i32 %7, ptr %ehselector.slot, align 4
  invoke void @__cxa_end_catch()
          to label %invoke.cont3 unwind label %terminate.lpad

invoke.cont3:                                     ; preds = %lpad1
  br label %eh.resume

eh.resume:                                        ; preds = %invoke.cont3
  %exn4 = load ptr, ptr %exn.slot, align 8
  %sel = load i32, ptr %ehselector.slot, align 4
  %lpad.val = insertvalue { ptr, i32 } poison, ptr %exn4, 0
  %lpad.val5 = insertvalue { ptr, i32 } %lpad.val, i32 %sel, 1
  resume { ptr, i32 } %lpad.val5

terminate.lpad:                                   ; preds = %lpad1
  %8 = landingpad { ptr, i32 }
          catch ptr null
  %9 = extractvalue { ptr, i32 } %8, 0
  call void @__clang_call_terminate(ptr %9) #9
  unreachable
}

; Function Attrs: mustprogress noinline optnone uwtable
define dso_local void @_Z7allowedi(i32 noundef %x) #0 personality ptr @__gxx_personality_v0 {
entry:
  %x.addr = alloca i32, align 4
  %g = alloca %struct.Guard, align 8
  %exn.slot = alloca ptr, align 8
  %ehselector.slot = alloca i32, align 4
  store i32 %x, ptr %x.addr, align 4
  %name = getelementptr inbounds %struct.Guard, ptr %g, i32 0, i32 0
  store ptr @.str.10, ptr %name, align 8
  %0 = load i32, ptr %x.addr, align 4
  invoke void @_Z11throw_errori(i32 noundef %0)
          to label %invoke.cont unwind label %lpad

invoke.cont:                                      ; preds = %entry
  call void @_ZN5GuardD2Ev(ptr noundef nonnull align 8 dereferenceable(8) %g) #7
  ret void

lpad:                                             ; preds = %entry
  %1 = landingpad { ptr, i32 }
          cleanup
          filter [1 x ptr] [ptr @_ZTI5Error]
  %2 = extractvalue { ptr, i32 } %1, 0
  store ptr %2, ptr %exn.slot, align 8
  %3 = extractvalue { ptr, i32 } %1, 1
  store i32 %3, ptr %ehselector.slot, align 4
  call void @_ZN5GuardD2Ev(ptr noundef nonnull align 8 dereferenceable(8) %g) #7
  br label %filter.dispatch

filter.dispatch:                                  ; preds = %lpad
  %sel = load i32, ptr %ehselector.slot, align 4
  %ehspec.fails = icmp slt i32 %sel, 0
  br i1 %ehspec.fails, label %ehspec.unexpected, label %eh.resume

ehspec.unexpected:                                ; preds = %filter.dispatch
  %exn = load ptr, ptr %exn.slot, align 8
  call void @__cxa_call_unexpected(ptr %exn) #8
  unreachable

eh.resume:                                        ; preds = %filter.dispatch
  %exn1 = load ptr, ptr %exn.slot, align 8
  %sel2 = load i32, ptr %ehselector.slot, align 4
  %lpad.val = insertvalue { ptr, i32 } poison, ptr %exn1, 0
  %lpad.val3 = insertvalue { ptr, i32 } %lpad.val, i32 %sel2, 1
  resume { ptr, i32 } %lpad.val3
}

declare void @__cxa_call_unexpected(ptr)

; Function Attrs: mustprogress noinline optnone uwtable
define dso_local void @_Z14through_filteri(i32 noundef %x) #0 personality ptr @__gxx_personality_v0 {
entry:
  %x.addr = alloca i32, align 4
  %exn.slot = alloca ptr, align 8
  %ehselector.slot = alloca i32, align 4
  %e = alloca ptr, align 8
  store i32 %x, ptr %x.addr, align 4
  %0 = load i32, ptr %x.addr, align 4
  invoke void @_Z7allowedi(i32 noundef %0)
          to label %invoke.cont unwind label %lpad

invoke.cont:                                      ; preds = %entry
  br label %try.cont

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
  %value = getelementptr inbounds %struct.Base, ptr %6, i32 0, i32 0
  %7 = load i32, ptr %value, align 4
  %call = invoke i32 (ptr, ...) @printf(ptr noundef @.str.11, i32 noundef %7)
          to label %invoke.cont2 unwind label %lpad1

invoke.cont2:                                     ; preds = %catch
  call void @__cxa_end_catch()
  br label %try.cont

try.cont:                                         ; preds = %invoke.cont2, %invoke.cont
  ret void

lpad1:                                            ; preds = %catch
  %8 = landingpad { ptr, i32 }
          cleanup
  %9 = extractvalue { ptr, i32 } %8, 0
  store ptr %9, ptr %exn.slot, align 8
  %10 = extractvalue { ptr, i32 } %8, 1
  store i32 %10, ptr %ehselector.slot, align 4
  invoke void @__cxa_end_catch()
          to label %invoke.cont3 unwind label %terminate.lpad

invoke.cont3:                                     ; preds = %lpad1
  br label %eh.resume

eh.resume:                                        ; preds = %invoke.cont3, %catch.dispatch
  %exn4 = load ptr, ptr %exn.slot, align 8
  %sel5 = load i32, ptr %ehselector.slot, align 4
  %lpad.val = insertvalue { ptr, i32 } poison, ptr %exn4, 0
  %lpad.val6 = insertvalue { ptr, i32 } %lpad.val, i32 %sel5, 1
  resume { ptr, i32 } %lpad.val6

terminate.lpad:                                   ; preds = %lpad1
  %11 = landingpad { ptr, i32 }
          catch ptr null
  %12 = extractvalue { ptr, i32 } %11, 0
  call void @__clang_call_terminate(ptr %12) #9
  unreachable
}

; Function Attrs: mustprogress noinline optnone uwtable
define dso_local void @_Z8two_padsi(i32 noundef %x) #0 personality ptr @__gxx_personality_v0 {
entry:
  %x.addr = alloca i32, align 4
  %exn.slot = alloca ptr, align 8
  %ehselector.slot = alloca i32, align 4
  %v = alloca i32, align 4
  %e = alloca ptr, align 8
  store i32 %x, ptr %x.addr, align 4
  %0 = load i32, ptr %x.addr, align 4
  invoke void @_Z9throw_inti(i32 noundef %0)
          to label %invoke.cont unwind label %lpad

invoke.cont:                                      ; preds = %entry
  br label %try.cont

lpad:                                             ; preds = %entry
  %1 = landingpad { ptr, i32 }
          catch ptr @_ZTIi
  %2 = extractvalue { ptr, i32 } %1, 0
  store ptr %2, ptr %exn.slot, align 8
  %3 = extractvalue { ptr, i32 } %1, 1
  store i32 %3, ptr %ehselector.slot, align 4
  br label %catch.dispatch

catch.dispatch:                                   ; preds = %lpad
  %sel = load i32, ptr %ehselector.slot, align 4
  %4 = call i32 @llvm.eh.typeid.for.p0(ptr @_ZTIi) #7
  %matches = icmp eq i32 %sel, %4
  br i1 %matches, label %catch, label %eh.resume

catch:                                            ; preds = %catch.dispatch
  %exn = load ptr, ptr %exn.slot, align 8
  %5 = call ptr @__cxa_begin_catch(ptr %exn) #7
  %6 = load i32, ptr %5, align 4
  store i32 %6, ptr %v, align 4
  %7 = load i32, ptr %v, align 4
  %call = invoke i32 (ptr, ...) @printf(ptr noundef @.str.12, i32 noundef %7)
          to label %invoke.cont2 unwind label %lpad1

invoke.cont2:                                     ; preds = %catch
  call void @__cxa_end_catch() #7
  br label %try.cont

try.cont:                                         ; preds = %invoke.cont2, %invoke.cont
  %8 = load i32, ptr %x.addr, align 4
  invoke void @_Z11throw_errori(i32 noundef %8)
          to label %invoke.cont4 unwind label %lpad3

invoke.cont4:                                     ; preds = %try.cont
  br label %try.cont14

lpad1:                                            ; preds = %catch
  %9 = landingpad { ptr, i32 }
          cleanup
  %10 = extractvalue { ptr, i32 } %9, 0
  store ptr %10, ptr %exn.slot, align 8
  %11 = extractvalue { ptr, i32 } %9, 1
  store i32 %11, ptr %ehselector.slot, align 4
  call void @__cxa_end_catch() #7
  br label %eh.resume

lpad3:                                            ; preds = %try.cont
  %12 = landingpad { ptr, i32 }
          catch ptr @_ZTI5Error
  %13 = extractvalue { ptr, i32 } %12, 0
  store ptr %13, ptr %exn.slot, align 8
  %14 = extractvalue { ptr, i32 } %12, 1
  store i32 %14, ptr %ehselector.slot, align 4
  br label %catch.dispatch5

catch.dispatch5:                                  ; preds = %lpad3
  %sel6 = load i32, ptr %ehselector.slot, align 4
  %15 = call i32 @llvm.eh.typeid.for.p0(ptr @_ZTI5Error) #7
  %matches7 = icmp eq i32 %sel6, %15
  br i1 %matches7, label %catch8, label %eh.resume

catch8:                                           ; preds = %catch.dispatch5
  %exn9 = load ptr, ptr %exn.slot, align 8
  %16 = call ptr @__cxa_begin_catch(ptr %exn9) #7
  store ptr %16, ptr %e, align 8
  %17 = load ptr, ptr %e, align 8
  %value = getelementptr inbounds %struct.Base, ptr %17, i32 0, i32 0
  %18 = load i32, ptr %value, align 4
  %call12 = invoke i32 (ptr, ...) @printf(ptr noundef @.str.13, i32 noundef %18)
          to label %invoke.cont11 unwind label %lpad10

invoke.cont11:                                    ; preds = %catch8
  call void @__cxa_end_catch()
  br label %try.cont14

try.cont14:                                       ; preds = %invoke.cont11, %invoke.cont4
  ret void

lpad10:                                           ; preds = %catch8
  %19 = landingpad { ptr, i32 }
          cleanup
  %20 = extractvalue { ptr, i32 } %19, 0
  store ptr %20, ptr %exn.slot, align 8
  %21 = extractvalue { ptr, i32 } %19, 1
  store i32 %21, ptr %ehselector.slot, align 4
  invoke void @__cxa_end_catch()
          to label %invoke.cont13 unwind label %terminate.lpad

invoke.cont13:                                    ; preds = %lpad10
  br label %eh.resume

eh.resume:                                        ; preds = %invoke.cont13, %catch.dispatch5, %lpad1, %catch.dispatch
  %exn15 = load ptr, ptr %exn.slot, align 8
  %sel16 = load i32, ptr %ehselector.slot, align 4
  %lpad.val = insertvalue { ptr, i32 } poison, ptr %exn15, 0
  %lpad.val17 = insertvalue { ptr, i32 } %lpad.val, i32 %sel16, 1
  resume { ptr, i32 } %lpad.val17

terminate.lpad:                                   ; preds = %lpad10
  %22 = landingpad { ptr, i32 }
          catch ptr null
  %23 = extractvalue { ptr, i32 } %22, 0
  call void @__clang_call_terminate(ptr %23) #9
  unreachable
}

; Function Attrs: mustprogress noinline optnone uwtable
define dso_local void @_Z9forbiddeni(i32 noundef %x) #0 personality ptr @__gxx_personality_v0 {
entry:
  %x.addr = alloca i32, align 4
  %exn.slot = alloca ptr, align 8
  %ehselector.slot = alloca i32, align 4
  store i32 %x, ptr %x.addr, align 4
  %0 = load i32, ptr %x.addr, align 4
  invoke void @_Z9throw_inti(i32 noundef %0)
          to label %invoke.cont unwind label %lpad

invoke.cont:                                      ; preds = %entry
  ret void

lpad:                                             ; preds = %entry
  %1 = landingpad { ptr, i32 }
          filter [1 x ptr] [ptr @_ZTI5Error]
  %2 = extractvalue { ptr, i32 } %1, 0
  store ptr %2, ptr %exn.slot, align 8
  %3 = extractvalue { ptr, i32 } %1, 1
  store i32 %3, ptr %ehselector.slot, align 4
  br label %filter.dispatch

filter.dispatch:                                  ; preds = %lpad
  %sel = load i32, ptr %ehselector.slot, align 4
  %ehspec.fails = icmp slt i32 %sel, 0
  br i1 %ehspec.fails, label %ehspec.unexpected, label %eh.resume

ehspec.unexpected:                                ; preds = %filter.dispatch
  %exn = load ptr, ptr %exn.slot, align 8
  call void @__cxa_call_unexpected(ptr %exn) #8
  unreachable

eh.resume:                                        ; preds = %filter.dispatch
  %exn1 = load ptr, ptr %exn.slot, align 8
  %sel2 = load i32, ptr %ehselector.slot, align 4
  %lpad.val = insertvalue { ptr, i32 } poison, ptr %exn1, 0
  %lpad.val3 = insertvalue { ptr, i32 } %lpad.val, i32 %sel2, 1
  resume { ptr, i32 } %lpad.val3
}

; Function Attrs: mustprogress noinline nounwind optnone uwtable
define dso_local void @_Z9no_excepti(i32 noundef %x) #1 personality ptr @__gxx_personality_v0 {
entry:
  %x.addr = alloca i32, align 4
  store i32 %x, ptr %x.addr, align 4
  %0 = load i32, ptr %x.addr, align 4
  invoke void @_Z9throw_inti(i32 noundef %0)
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

; Function Attrs: mustprogress noinline norecurse optnone uwtable
define dso_local noundef i32 @main(i32 noundef %argc, ptr noundef %argv) #5 {
entry:
  %retval = alloca i32, align 4
  %argc.addr = alloca i32, align 4
  %argv.addr = alloca ptr, align 8
  store i32 0, ptr %retval, align 4
  store i32 %argc, ptr %argc.addr, align 4
  store ptr %argv, ptr %argv.addr, align 8
  %0 = load i32, ptr %argc.addr, align 4
  %cmp = icmp sgt i32 %0, 1
  br i1 %cmp, label %land.lhs.true, label %if.end

land.lhs.true:                                    ; preds = %entry
  %1 = load ptr, ptr %argv.addr, align 8
  %arrayidx = getelementptr inbounds ptr, ptr %1, i64 1
  %2 = load ptr, ptr %arrayidx, align 8
  %call = call i32 @strcmp(ptr noundef %2, ptr noundef @.str.14) #10
  %cmp1 = icmp eq i32 %call, 0
  br i1 %cmp1, label %if.then, label %if.end

if.then:                                          ; preds = %land.lhs.true
  %call2 = call i32 (ptr, ...) @printf(ptr noundef @.str.15)
  %3 = load ptr, ptr @stdout, align 8
  %call3 = call i32 @fflush(ptr noundef %3)
  call void @_Z9forbiddeni(i32 noundef 7)
  store i32 3, ptr %retval, align 4
  br label %return

if.end:                                           ; preds = %land.lhs.true, %entry
  %4 = load i32, ptr %argc.addr, align 4
  %cmp4 = icmp sgt i32 %4, 1
  br i1 %cmp4, label %land.lhs.true5, label %if.end12

land.lhs.true5:                                   ; preds = %if.end
  %5 = load ptr, ptr %argv.addr, align 8
  %arrayidx6 = getelementptr inbounds ptr, ptr %5, i64 1
  %6 = load ptr, ptr %arrayidx6, align 8
  %call7 = call i32 @strcmp(ptr noundef %6, ptr noundef @.str.16) #10
  %cmp8 = icmp eq i32 %call7, 0
  br i1 %cmp8, label %if.then9, label %if.end12

if.then9:                                         ; preds = %land.lhs.true5
  %call10 = call i32 (ptr, ...) @printf(ptr noundef @.str.17)
  %7 = load ptr, ptr @stdout, align 8
  %call11 = call i32 @fflush(ptr noundef %7)
  call void @_Z9no_excepti(i32 noundef 8) #7
  store i32 4, ptr %retval, align 4
  br label %return

if.end12:                                         ; preds = %land.lhs.true5, %if.end
  call void @_Z7by_typei(i32 noundef 1)
  call void @_Z6nestedi(i32 noundef 2)
  call void @_Z9catch_alli(i32 noundef 3)
  call void @_Z14through_filteri(i32 noundef 4)
  call void @_Z8two_padsi(i32 noundef 5)
  %call13 = call i32 (ptr, ...) @printf(ptr noundef @.str.18)
  store i32 0, ptr %retval, align 4
  br label %return

return:                                           ; preds = %if.end12, %if.then9, %if.then
  %8 = load i32, ptr %retval, align 4
  ret i32 %8
}

; Function Attrs: nounwind willreturn memory(read)
declare i32 @strcmp(ptr noundef, ptr noundef) #6

declare i32 @fflush(ptr noundef) #3

attributes #0 = { mustprogress noinline optnone uwtable "frame-pointer"="non-leaf" "no-trapping-math"="true" "stack-protector-buffer-size"="8" "target-cpu"="generic" "target-features"="+fp-armv8,+neon,+outline-atomics,+v8a,-fmv" }
attributes #1 = { mustprogress noinline nounwind optnone uwtable "frame-pointer"="non-leaf" "no-trapping-math"="true" "stack-protector-buffer-size"="8" "target-cpu"="generic" "target-features"="+fp-armv8,+neon,+outline-atomics,+v8a,-fmv" }
attributes #2 = { nounwind memory(none) }
attributes #3 = { "frame-pointer"="non-leaf" "no-trapping-math"="true" "stack-protector-buffer-size"="8" "target-cpu"="generic" "target-features"="+fp-armv8,+neon,+outline-atomics,+v8a,-fmv" }
attributes #4 = { noinline noreturn nounwind uwtable "frame-pointer"="non-leaf" "no-trapping-math"="true" "stack-protector-buffer-size"="8" "target-cpu"="generic" "target-features"="+fp-armv8,+neon,+outline-atomics,+v8a,-fmv" }
attributes #5 = { mustprogress noinline norecurse optnone uwtable "frame-pointer"="non-leaf" "no-trapping-math"="true" "stack-protector-buffer-size"="8" "target-cpu"="generic" "target-features"="+fp-armv8,+neon,+outline-atomics,+v8a,-fmv" }
attributes #6 = { nounwind willreturn memory(read) "frame-pointer"="non-leaf" "no-trapping-math"="true" "stack-protector-buffer-size"="8" "target-cpu"="generic" "target-features"="+fp-armv8,+neon,+outline-atomics,+v8a,-fmv" }
attributes #7 = { nounwind }
attributes #8 = { noreturn }
attributes #9 = { noreturn nounwind }
attributes #10 = { nounwind willreturn memory(read) }

!llvm.module.flags = !{!0, !1, !2, !3, !4}
!llvm.ident = !{!5}

!0 = !{i32 1, !"wchar_size", i32 4}
!1 = !{i32 8, !"PIC Level", i32 2}
!2 = !{i32 7, !"PIE Level", i32 2}
!3 = !{i32 7, !"uwtable", i32 2}
!4 = !{i32 7, !"frame-pointer", i32 1}
!5 = !{!"Debian clang version 19.1.7 (3+b1)"}
