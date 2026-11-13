// WP-114 (spec/dwarf.md §6.20): the shapes DWARF 4 writes differently
// from DWARF 5: static data members (`DW_TAG_member`), deleted member
// functions (no `DW_AT_deleted`), arrays (the language's lower bound),
// and a unit with several code sections (`.debug_ranges`, from the
// inline function and the template's member).
struct NoCopy {
  NoCopy() = default;
  NoCopy(const NoCopy &) = delete;
  NoCopy &operator=(const NoCopy &) = delete;
  int v = 3;
  static int count;
  static const long big;
};
int NoCopy::count = 1;
const long NoCopy::big = 77;

template <typename T> struct Box {
  T items[4];
  T get(int i) const { return items[i]; }
};

inline int twice(int x) { return x * 2; }

int grid[2][3] = {{1, 2, 3}, {4, 5, 6}};

int use(int n) {
  NoCopy a;
  Box<long> b{{1, 2, 3, 4}};
  int local[5] = {n, n + 1, n + 2, n + 3, n + 4};
  int sum = a.v + NoCopy::count + (int)NoCopy::big + (int)b.get(1);
  sum += twice(local[2]) + grid[1][2];
  return sum;
}

int main() { return use(2) == 0; }

//@ cflags: -std=c++20 -gdwarf-4
//@ gdb: break dwarf4_shapes.cpp:32
//@ gdb: break Box<long>::get
//@ gdb: break twice
//@ gdb: run
//@ gdb: print i
//@ gdb: print items
//@ gdb: continue
//@ gdb: print x
//@ gdb: bt
//@ gdb: continue
//@ gdb: print a
//@ gdb: print NoCopy::count
//@ gdb: print NoCopy::big
//@ gdb: print local
//@ gdb: print grid
//@ gdb: print sum
//@ gdb: ptype NoCopy
//@ gdb: continue
//@ gdb-check: Breakpoint 2, Box<long>::get (this=0x?, i=1) at dwarf4_shapes.cpp:19
//@ gdb-check: $1 = 1
//@ gdb-check: $2 = {1, 2, 3, 4}
//@ gdb-check: Breakpoint 3, twice (x=4) at dwarf4_shapes.cpp:22
//@ gdb-check: $3 = 4
//@ gdb-check: in use (n=2) at dwarf4_shapes.cpp:31
//@ gdb-check: Breakpoint 1, use (n=2) at dwarf4_shapes.cpp:32
//@ gdb-check: $4 = {v = 3, static count = 1, static big = 77}
//@ gdb-check: $5 = 1
//@ gdb-check: $6 = 77
//@ gdb-check: $7 = {2, 3, 4, 5, 6}
//@ gdb-check: $8 = {{1, 2, 3}, {4, 5, 6}}
//@ gdb-check: $9 = 97
//@ gdb-check: static int count;
//@ gdb-check: exited normally
//@ lldb: breakpoint set --file dwarf4_shapes.cpp --line 32
//@ lldb: breakpoint set --name twice
//@ lldb: run
//@ lldb: frame variable x
//@ lldb: continue
//@ lldb: frame variable a local sum
//@ lldb: target variable grid NoCopy::count NoCopy::big
//@ lldb: continue
//@ lldb-check: (int) x = 4
//@ lldb-check: [2] = 4, [3] = 5
//@ lldb-check: (int) sum = 97
//@ lldb-check: (int) NoCopy::count = 1
//@ lldb-check: (const long) NoCopy::big = 77
