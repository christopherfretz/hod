// The program of spec/eh.md §7.2. Compiled to unwind.ll with
//   clang++-19 -O0 -S -emit-llvm -fno-discard-value-names \
//     --target=aarch64-unknown-linux-gnu unwind.cpp -o unwind.ll
// An exception thrown in `thrower` unwinds through a large frame, a frame
// with dynamic stack allocation, a COMDAT template frame and a frame with
// a cleanup, is caught and rethrown on the way, and is caught in `main`.
#include <cstdio>

struct Error {
  int value;
};

struct Guard {
  const char *name;
  ~Guard() { std::printf("cleanup %s\n", name); }
};

__attribute__((noinline)) void thrower(int x) {
  if (x > 0)
    throw Error{x};
}

__attribute__((noinline)) int large_frame(int x) {
  volatile char buffer[70000];
  buffer[x] = static_cast<char>(x);
  Guard g{"large"};
  thrower(x);
  return buffer[0];
}

__attribute__((noinline)) int dynamic_frame(int n) {
  char *p = static_cast<char *>(__builtin_alloca(n * 16));
  p[0] = 1;
  int r = large_frame(n);
  return r + p[0];
}

template <typename T> __attribute__((noinline)) T templated(T n) {
  Guard g{"template"};
  return dynamic_frame(n);
}

__attribute__((noinline)) int rethrower(int n) {
  try {
    return templated(n);
  } catch (Error &e) {
    std::printf("rethrowing %d\n", e.value);
    e.value += 100;
    throw;
  }
}

int main() {
  for (int i = 1; i <= 3; ++i) {
    try {
      rethrower(i);
      std::printf("not reached\n");
    } catch (Error &e) {
      std::printf("caught %d\n", e.value);
    }
  }
  try {
    thrower(0);
    std::printf("no throw\n");
  } catch (...) {
    std::printf("not reached\n");
  }
  return 0;
}
