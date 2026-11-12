/* LLVM 22 only (spec/ll.md §3.2): block comments, ptrtoaddr, captures,
   and the attributes that 19 does not know. A } and a " in here. */

@target = global i32 /* between tokens */ 1
@address = global i64 ptrtoaddr (ptr @target to i64)
@address_plus = global i64 add (i64 ptrtoaddr (ptr getelementptr (i8, ptr @target, i64 2) to i64), i64 5)

declare void @captures(ptr captures(none), ptr captures(address), ptr captures(address, provenance), ptr captures(ret: address, provenance), ptr captures(address_is_null, read_provenance))
declare void @initializes(ptr initializes((0, 4), (8, 12)), ptr dead_on_return)
declare void @blocking() sanitize_realtime_blocking
declare void @more_attrs() nodivergencesource sanitize_realtime sanitize_alloc_token sanitize_type
declare noundef nofpclass(nan) float @ret_attrs(float nofpclass(all))

define void @body(ptr captures(none) %p) /* after the header */ {
  /* a } in a block comment in a body { */
  %a = ptrtoaddr ptr %p to i64
  ret void /* } */
}

define void @after() {
  ret void
}
