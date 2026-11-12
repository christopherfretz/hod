// FINDING (WP-099), class `load-placement` (llc only): `n->~T()` of a
// trivial destructor leaves a load of `n` whose value is unused (line 9).
// `llc` merges it with the load of `n` for the call on line 10 and keeps
// line 9's location; hod drops the dead load. 40 pairs in 13 modules
// (libstdc++'s `_Rb_tree::_M_construct_node` catch handler).
struct T { int x; };
void put(T *n) { n->x = 1; }
void h(T *n) {
  n->~T();
  put(n);
}

int main() { T t{0}; h(&t); return t.x == 1 ? 0 : 1; }
//@ frontend: clang-19 clang-22
//@ gdb: break dead_load.cpp:9
//@ gdb: run
//@ gdb: continue
//@ gdb-check: Breakpoint 1,
//@ gdb-check: exited normally
//@ lldb: breakpoint set --file dead_load.cpp --line 9
//@ lldb: run
//@ lldb: continue
//@ lldb-check: stop reason = breakpoint 1.1
//@ lldb-check: exited with status = 0
