// WP-111, class `post-indexed-load` (hod only): a member's address,
// alone on its line, passed to an `always_inline` function after a
// virtual call through `this` (libstdc++'s
// `_Sp_counted_base::_M_release_last_use`, `shared_ptr_base.h:187`).
// `llc` folds `this + 12` into the post-indexed load of the vtable
// pointer, so its table has no row at that line; hod computes the
// address at its line.
__attribute__((always_inline)) inline int add(int *p, int v) {
  return *p += v;
}

struct Counted {
  virtual void dispose() noexcept {}
  int use = 1;
  int weak = 1;
  void release() noexcept {
    dispose();
    if (add(&weak,
            -1) == 0)
      use = 0;
  }
};

int main() {
  Counted c;
  c.release();
  return c.use + c.weak;
}
//@ frontend: clang-19 clang-22
//@ gdb: break post_index.cpp:18
//@ gdb: run
//@ gdb: continue
//@ gdb-check: Breakpoint 1,
//@ gdb-check: exited normally
//@ lldb: breakpoint set --file post_index.cpp --line 18
//@ lldb: run
//@ lldb: continue
//@ lldb-check: stop reason = breakpoint 1.1
//@ lldb-check: exited with status = 0
