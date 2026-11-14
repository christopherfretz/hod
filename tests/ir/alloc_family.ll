; "alloc-family", as clang and kiln at -O1 and above put it (with
; allockind and allocsize) on the C library's allocation functions:
; malloc, calloc, realloc, free and aligned_alloc are the family "malloc".
; Only the optimizer reads it (InstCombine removes a matching
; allocation/free pair of one family and leaves a mismatched pair
; alone); llc 19 and 22 write the same object without it. hod drops it:
; on declarations, on a definition (a family of its own), and in the
; attribute group of a call site.
;@ covers: spec/ir.md §5.7 attributes (alloc-family)
;@ stdout: sum 28
;@ stdout: grown 7 77
;@ stdout: zeroed 0
;@ stdout: pool 5

@.sum = private unnamed_addr constant [8 x i8] c"sum %d\0A\00"
@.grown = private unnamed_addr constant [13 x i8] c"grown %d %d\0A\00"
@.zeroed = private unnamed_addr constant [11 x i8] c"zeroed %d\0A\00"
@.pool = private unnamed_addr constant [9 x i8] c"pool %d\0A\00"

declare i32 @printf(ptr, ...)
declare noalias ptr @malloc(i64 noundef) #0
declare noalias ptr @calloc(i64 noundef, i64 noundef) #1
declare noalias ptr @realloc(ptr allocptr, i64 noundef) #2
declare void @free(ptr allocptr noundef) #3
declare noalias ptr @aligned_alloc(i64 allocalign noundef, i64 noundef) #4

; An allocator of its own family, defined here.
define noalias ptr @pool_alloc(i64 %n) noinline #5 {
  %p = call ptr @malloc(i64 %n) #6
  ret ptr %p
}

define void @pool_free(ptr %p) noinline #7 {
  call void @free(ptr %p) #6
  ret void
}

define i32 @sum(i32 %n) noinline {
entry:
  %bytes = mul i32 %n, 4
  %w = zext i32 %bytes to i64
  %a = call ptr @malloc(i64 %w)
  br label %fill
fill:
  %i = phi i32 [ 0, %entry ], [ %i1, %fill ]
  %wi = zext i32 %i to i64
  %p = getelementptr i32, ptr %a, i64 %wi
  store i32 %i, ptr %p
  %i1 = add i32 %i, 1
  %more = icmp ult i32 %i1, %n
  br i1 %more, label %fill, label %add
add:
  %j = phi i32 [ 0, %fill ], [ %j1, %add ]
  %s = phi i32 [ 0, %fill ], [ %s1, %add ]
  %wj = zext i32 %j to i64
  %q = getelementptr i32, ptr %a, i64 %wj
  %e = load i32, ptr %q
  %s1 = add i32 %s, %e
  %j1 = add i32 %j, 1
  %done = icmp eq i32 %j1, %n
  br i1 %done, label %out, label %add
out:
  call void @free(ptr %a)
  ret i32 %s1
}

define i32 @main() {
  %s = call i32 @sum(i32 8)
  call i32 (ptr, ...) @printf(ptr @.sum, i32 %s)

  %a = call ptr @malloc(i64 8) #6
  store i32 7, ptr %a
  %b = call ptr @realloc(ptr %a, i64 400) #6
  %b1 = getelementptr i32, ptr %b, i64 99
  store i32 77, ptr %b1
  %v0 = load i32, ptr %b
  %v1 = load i32, ptr %b1
  call i32 (ptr, ...) @printf(ptr @.grown, i32 %v0, i32 %v1)
  call void @free(ptr %b)

  %z = call ptr @calloc(i64 16, i64 4)
  %z1 = getelementptr i32, ptr %z, i64 15
  %zv = load i32, ptr %z1
  call i32 (ptr, ...) @printf(ptr @.zeroed, i32 %zv)
  call void @free(ptr %z)

  %al = call ptr @aligned_alloc(i64 64, i64 64)
  call void @free(ptr %al)

  %p = call ptr @pool_alloc(i64 4)
  store i32 5, ptr %p
  %pv = load i32, ptr %p
  call i32 (ptr, ...) @printf(ptr @.pool, i32 %pv)
  call void @pool_free(ptr %p)
  ret i32 0
}

attributes #0 = { mustprogress nofree nounwind willreturn allockind("alloc,uninitialized") allocsize(0) memory(inaccessiblemem: readwrite) "alloc-family"="malloc" }
attributes #1 = { mustprogress nofree nounwind willreturn allockind("alloc,zeroed") allocsize(0,1) memory(inaccessiblemem: readwrite) "alloc-family"="malloc" }
attributes #2 = { mustprogress nounwind willreturn allockind("realloc") allocsize(1) memory(argmem: readwrite, inaccessiblemem: readwrite) "alloc-family"="malloc" }
attributes #3 = { mustprogress nounwind willreturn allockind("free") memory(argmem: readwrite, inaccessiblemem: readwrite) "alloc-family"="malloc" }
attributes #4 = { mustprogress nofree nounwind willreturn allockind("alloc,uninitialized,aligned") allocsize(1) memory(inaccessiblemem: readwrite) "alloc-family"="malloc" }
attributes #5 = { nounwind allockind("alloc,uninitialized") allocsize(0) "alloc-family"="pool" }
attributes #6 = { nounwind "alloc-family"="malloc" }
attributes #7 = { nounwind allockind("free") "alloc-family"="pool" }
