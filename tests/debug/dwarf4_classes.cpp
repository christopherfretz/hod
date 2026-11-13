// WP-114: classes.cpp written as DWARF 4 (spec/dwarf.md §6.20): the same program
// and the same sessions as classes.cpp, with `-gdwarf-4`.
#include "classes.cpp"

//@ frontend: clang-19 clang-22
//@ gdb: break geo::Box::area
//@ gdb: break Ring<short, 3>::push
//@ gdb: break classes.cpp:42
//@ gdb: run
//@ gdb: print v
//@ gdb: print *this
//@ gdb: delete 2
//@ gdb: continue
//@ gdb: print *this
//@ gdb: print this->w
//@ gdb: print kind
//@ gdb: print geo::Shape::made
//@ gdb: bt
//@ gdb: up
//@ gdb: print shape.kind
//@ gdb: ptype geo::Box
//@ gdb: continue
//@ gdb: info args
//@ gdb: print bigger
//@ gdb: continue
//@ gdb: info args
//@ gdb: continue
//@ gdb-check: Breakpoint 2, Ring<short, 3>::push (this=0x
//@ gdb-check: $1 = (const short &) @0x
//@ gdb-check: $2 = {items = {0, 0, 0}, next = 0}
//@ gdb-check: Breakpoint 1, geo::Box::area (this=0x
//@ gdb-check: <vtable for geo::Box+16>, kind = geo::detail::Kind::Box, static made = 1
//@ gdb-check: $4 = 3
//@ gdb-check: $5 = geo::detail::Kind::Box
//@ gdb-check: $6 = 1
//@ gdb-check: #1  0x
//@ gdb-check: measure (shape=...)
//@ gdb-check: $7 = geo::detail::Kind::Box
//@ gdb-check: type = struct geo::Box : public geo::Shape {
//@ gdb-check: Breakpoint 3.1, largest<long> (a=@0x
//@ gdb-check: $8 = 20
//@ gdb-check: Breakpoint 3.2, largest<double> (a=@0x
//@ gdb-check: exited normally
//@ lldb: breakpoint set --name geo::Box::area
//@ lldb: breakpoint set --file classes.cpp --line 42
//@ lldb: run
//@ lldb: frame variable *this
//@ lldb: frame variable this->w this->h
//@ lldb: target variable geo::Shape::made
//@ lldb: bt 2
//@ lldb: up
//@ lldb: frame variable shape.kind
//@ lldb: continue
//@ lldb: frame variable a b bigger
//@ lldb: continue
//@ lldb: frame variable bigger
//@ lldb: continue
//@ lldb-check: stop reason = breakpoint 1.1
//@ lldb-check: (geo::Box) *this = {
//@ lldb-check: (int) this->w = 3
//@ lldb-check: (int) geo::Shape::made = 1
//@ lldb-check: `measure(shape=
//@ lldb-check: (geo::detail::Kind) shape.kind = Box
//@ lldb-check: (long) bigger = 20
//@ lldb-check: (double) bigger = 2.5
//@ lldb-check: exited with status = 0
//@ cflags: -gdwarf-4
