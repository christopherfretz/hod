// Split DWARF (-gsplit-dwarf, WP-062): the object has a skeleton unit,
// the DIEs are in a .dwo the debugger finds beside the program. A
// breakpoint in a member function, a backtrace, a parameter, a local, a
// class member, a global and a static member.
namespace demo {
int limit = 40;

struct Counter {
  static int made;
  int total;
  explicit Counter(int start) : total(start) { made++; }
  int add(int step) {
    int before = total;
    total += step;
    return before;
  }
};

int Counter::made = 0;
} // namespace demo

static int drive(demo::Counter &c, int rounds) {
  int last = 0;
  for (int i = 1; i <= rounds; i++)
    last = c.add(i);
  return last;
}

int main() {
  demo::Counter c(5);
  int got = drive(c, 3);
  return got == 8 && c.total == 11 && demo::limit == 40 ? 0 : 1;
}

//@ cflags: -gsplit-dwarf
//@ frontend: clang-19 clang-22
//@ tier: quick
//@ gdb: break split.cpp:15
//@ gdb: run
//@ gdb: bt
//@ gdb: print step
//@ gdb: print before
//@ gdb: print this->total
//@ gdb: print *this
//@ gdb: print demo::limit
//@ gdb: print demo::Counter::made
//@ gdb: up
//@ gdb: info locals
//@ gdb: print c
//@ gdb: delete 1
//@ gdb: continue
//@ gdb-check: Breakpoint 1, demo::Counter::add (this=0x
//@ gdb-check: step=1) at split.cpp:15
//@ gdb-check: drive (c=..., rounds=3) at split.cpp:25
//@ gdb-check: $1 = 1
//@ gdb-check: $2 = 5
//@ gdb-check: $3 = 6
//@ gdb-check: $4 = {static made = 1, total = 6}
//@ gdb-check: $5 = 40
//@ gdb-check: $6 = 1
//@ gdb-check: i = 1
//@ gdb-check: last = 0
//@ gdb-check: (demo::Counter &) @0x
//@ gdb-check: exited normally
//@ lldb: breakpoint set --file split.cpp --line 15
//@ lldb: run
//@ lldb: bt 3
//@ lldb: frame variable
//@ lldb: frame variable this->total *this
//@ lldb: target variable demo::limit demo::Counter::made
//@ lldb: up
//@ lldb: frame variable
//@ lldb: breakpoint delete --force
//@ lldb: continue
//@ lldb-check: stop reason = breakpoint 1.1
//@ lldb-check: `demo::Counter::add(this=
//@ lldb-check: step=1) at split.cpp:15
//@ lldb-check: `drive(c=
//@ lldb-check: (int) step = 1
//@ lldb-check: (int) before = 5
//@ lldb-check: (int) this->total = 6
//@ lldb-check: (int) demo::limit = 40
//@ lldb-check: (int) demo::Counter::made = 1
//@ lldb-check: (int) rounds = 3
//@ lldb-check: (int) last = 0
//@ lldb-check: exited with status = 0
