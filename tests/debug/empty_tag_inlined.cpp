// FINDING (WP-156): an empty-class parameter of an `always_inline`
// callee, as libstdc++'s `std::__distance(first, last,
// random_access_iterator_tag)` has. The SysV x86-64 ABI passes no
// argument for an empty record, so after inlining clang declares the
// tag at `ptr poison` (`#dbg_declare(ptr poison, ...)`); `llc` drops the
// declaration and writes no DIE for `tag`. hod refused the module
// (`debug:declare-address`) on x86-64 only: on AArch64 the tag is an
// `i8` argument with a slot. The other parameters and the caller's
// variables must still read back.
struct tag {};

__attribute__((always_inline)) inline long span(const int *first,
                                                const int *last, tag t) {
  long n = last - first;
  return n;
}

long count(const int *p, long len) {
  long got = span(p, p + len, tag{});
  return got;
}

int main() {
  int values[5] = {1, 2, 3, 4, 5};
  long c = count(values, 5);
  return c == 5 ? 0 : 1;
}
//@ tier: quick
//@ gdb: break empty_tag_inlined.cpp:15
//@ gdb: break empty_tag_inlined.cpp:20
//@ gdb: run
//@ gdb: print n
//@ gdb: print *first
//@ gdb: print last - first
//@ gdb: up
//@ gdb: print len
//@ gdb: continue
//@ gdb: print got
//@ gdb: print len
//@ gdb: continue
//@ gdb-check: Breakpoint 1, span
//@ gdb-check: = 5
//@ gdb-check: = 1
//@ gdb-check: = 5
//@ gdb-check: #1
//@ gdb-check: = 5
//@ gdb-check: Breakpoint 2, count
//@ gdb-check: = 5
//@ gdb-check: = 5
//@ gdb-check: exited normally
//@ lldb: breakpoint set --file empty_tag_inlined.cpp --line 15
//@ lldb: breakpoint set --file empty_tag_inlined.cpp --line 20
//@ lldb: run
//@ lldb: frame variable n first last
//@ lldb: continue
//@ lldb: frame variable got len
//@ lldb: continue
//@ lldb-check: (long) n = 5
//@ lldb-check: (long) got = 5
//@ lldb-check: (long) len = 5
//@ lldb-check: exited with status = 0
