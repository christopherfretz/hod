// The hardening flags together on x86-64 (`spec/codegen.md` §5.6.30):
// `deep` has a protected 40000-byte frame (`-fstack-protector-strong`:
// the guard word, and the low guard at the bottom of the local area
// since it has an `alloca`), probed in a loop (`-fstack-clash-protection`,
// the `alloca` through the dynamic loop too), starts with `endbr64`
// (`-fcf-protection=full`) and is `preserve_most` (the registers it keeps
// pushed after `rbp`). From its first instruction to its return, at
// every instruction (`nexti`, stepping over calls, through `endbr64`,
// the pushes, the probe loop, the guard's entry code and check, the
// pops) the backtrace is `deep`, `guarded`, `main`: the script counts
// the instructions where it is not, and prints that count (0); the same
// for the second call, out of which the exception is thrown (the loop
// ends at the breakpoint in the handler) and caught in `guarded`.
#include <alloca.h>
#include <cstdio>
#include <cstring>

struct Failure {
  int code;
};

__attribute__((preserve_most)) int deep(int n) {
  char big[40000];
  std::memset(big, n, sizeof big);
  char *more = static_cast<char *>(alloca(n * 1000 + 16));
  std::memset(more, n, n * 1000 + 16);
  int sum = big[0] + big[39999] + more[n];
  if (n > 2)
    throw Failure{sum};
  return sum;
}

int guarded(int n) {
  int result = -1;
  try {
    result = deep(n);
  } catch (const Failure &caught) {
    int seen = caught.code;
    result = seen + 1;
  }
  return result;
}

int main() {
  int a = guarded(1);
  int b = guarded(5);
  std::printf("%d %d\n", a, b);
  return 0;
}
//@ frontend: clang-22
//@ target: x86_64
//@ cflags: -fstack-protector-strong -fstack-clash-protection -fcf-protection=full
//@ gdb: set print frame-arguments presence
//@ gdb: break *deep
//@ gdb: break hardening_x64.cpp:39
//@ gdb: run
//@ gdb: bt 3
//@ gdb: python exec("gdb.execute('set suppress-cli-notifications on')\nbad = []\nsteps = 0\nwhile 'deep' in (gdb.selected_frame().name() or ''):\n  f = gdb.selected_frame()\n  up = [f.older(), f.older() and f.older().older()]\n  if not (up[0] and 'guarded' in (up[0].name() or '') and up[1] and 'main' in (up[1].name() or '')): bad.append(int(f.pc()))\n  gdb.execute('nexti', to_string=True)\n  steps += 1\ngdb.execute('set suppress-cli-notifications off')\nprint('wrong backtraces:', len(bad), [hex(b) for b in bad[:8]], 'stepped:', steps > 20)")
//@ gdb: bt 2
//@ gdb: continue
//@ gdb: python exec("gdb.execute('set suppress-cli-notifications on')\nbad = []\nsteps = 0\nwhile 'deep' in (gdb.selected_frame().name() or ''):\n  f = gdb.selected_frame()\n  up = [f.older(), f.older() and f.older().older()]\n  if not (up[0] and 'guarded' in (up[0].name() or '') and up[1] and 'main' in (up[1].name() or '')): bad.append(int(f.pc()))\n  gdb.execute('nexti', to_string=True)\n  steps += 1\ngdb.execute('set suppress-cli-notifications off')\nprint('wrong backtraces:', len(bad), [hex(b) for b in bad[:8]], 'stepped:', steps > 20)")
//@ gdb: print seen
//@ gdb: bt 2
//@ gdb: continue
//@ gdb-check: Breakpoint 1, deep (...)
//@ gdb-check: wrong backtraces: 0 [] stepped: True
//@ gdb-check: #0  0x? in guarded (...) at hardening_x64.cpp:36
//@ gdb-check: $1 = 15
//@ gdb-check: #0  guarded (...) at hardening_x64.cpp:39
//@ gdb-check: 3 16
//@ gdb-check: exited normally
//@ lldb: settings set frame-format "frame #${frame.index}: ${function.name-without-args}{ at ${line.file.basename}:${line.number}}\n"
//@ lldb: breakpoint set --name deep --skip-prologue false
//@ lldb: breakpoint set --file hardening_x64.cpp --line 39
//@ lldb: run
//@ lldb: script exec("lldb.debugger.SetAsync(False)\nt = lldb.debugger.GetSelectedTarget().GetProcess().GetSelectedThread()\nbad = []\nsteps = 0\nwhile 'deep' in (t.GetFrameAtIndex(0).GetFunctionName() or ''):\n  names = [t.GetFrameAtIndex(i).GetFunctionName() or '' for i in (1, 2)]\n  if not ('guarded' in names[0] and 'main' in names[1]): bad.append(t.GetFrameAtIndex(0).GetPC())\n  t.StepInstruction(True)\n  steps += 1\nprint('wrong backtraces:', len(bad), [hex(b) for b in bad[:8]], 'stepped:', steps > 20)")
//@ lldb: thread backtrace -c 2
//@ lldb: continue
//@ lldb: script exec("lldb.debugger.SetAsync(False)\nt = lldb.debugger.GetSelectedTarget().GetProcess().GetSelectedThread()\nbad = []\nsteps = 0\nwhile 'deep' in (t.GetFrameAtIndex(0).GetFunctionName() or ''):\n  names = [t.GetFrameAtIndex(i).GetFunctionName() or '' for i in (1, 2)]\n  if not ('guarded' in names[0] and 'main' in names[1]): bad.append(t.GetFrameAtIndex(0).GetPC())\n  t.StepInstruction(True)\n  steps += 1\nprint('wrong backtraces:', len(bad), [hex(b) for b in bad[:8]], 'stepped:', steps > 20)")
//@ lldb: frame variable seen
//@ lldb: continue
//@ lldb-check: wrong backtraces: 0 [] stepped: True
//@ lldb-check: frame #0: guarded at hardening_x64.cpp:36
//@ lldb-check: (int) seen = 15
