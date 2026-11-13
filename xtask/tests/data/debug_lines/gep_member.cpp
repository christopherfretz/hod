// FINDING (WP-099), class `gep-placement` (hod only): `impl` is a
// zero-offset `getelementptr` of `this` on a line of its own (line 12).
// `llc` folds it into the call's argument; hod has an instruction for it
// there (a reload of `this`), so line 12 has a row in hod's table only.
// 213 hod-only pairs in 23 modules (libstdc++'s `_M_realloc_append`).
struct Impl { int n; };
void put(Impl &i, int v) { i.n = v; }
struct V {
  Impl impl; int k;
  void add(int x) {
    put(
        impl,
        x + k);
  }
};

int main() { V v{{0}, 2}; v.add(1); return v.impl.n == 3 ? 0 : 1; }
//@ frontend: clang-19 clang-22
//@ gdb: break gep_member.cpp:12
//@ gdb: run
//@ gdb: continue
//@ gdb-check: Breakpoint 1,
//@ gdb-check: exited normally
//@ lldb: breakpoint set --file gep_member.cpp --line 12
//@ lldb: run
//@ lldb: continue
//@ lldb-check: stop reason = breakpoint 1.1
//@ lldb-check: exited with status = 0
