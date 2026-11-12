// The program of spec/eh.md §9.9 (real unwinding through hod's LSDAs).
// Compiled to lsda.ll with
//   clang++-19 -std=c++14 -O0 -S -emit-llvm -fno-discard-value-names \
//     --target=aarch64-unknown-linux-gnu lsda.cpp -o lsda.ll
// Without an argument it runs every scenario that ends in a catch; with
// "filter" it violates an exception specification (std::unexpected, then
// std::terminate); with "noexcept" it throws out of a noexcept function.
#include <cstdio>
#include <cstring>

struct Base {
  int value;
};
struct Error : Base {
  explicit Error(int v) { value = v; }
};

struct Guard {
  const char *name;
  ~Guard() { std::printf("cleanup %s\n", name); }
};

__attribute__((noinline)) void throw_int(int x) {
  if (x > 0)
    throw x;
}
__attribute__((noinline)) void throw_error(int x) {
  if (x > 0)
    throw Error(x);
}
__attribute__((noinline)) int quiet(int x) noexcept { return x + 1; }

// Several types: the second one matches.
__attribute__((noinline)) void by_type(int x) {
  try {
    throw_int(x);
  } catch (long) {
    std::printf("wrong: long\n");
  } catch (int v) {
    std::printf("caught int %d\n", v);
  } catch (Base &) {
    std::printf("wrong: Base\n");
  }
}

// The inner handlers do not match; a cleanup runs; the outer one does.
__attribute__((noinline)) void inner(int x) {
  Guard g{"inner"};
  try {
    quiet(x);
    throw_error(x);
  } catch (int) {
    std::printf("wrong: int\n");
  } catch (double) {
    std::printf("wrong: double\n");
  }
}
__attribute__((noinline)) void nested(int x) {
  try {
    try {
      inner(x);
    } catch (long) {
      std::printf("wrong: long\n");
    }
  } catch (Base &b) {
    std::printf("caught Base %d\n", b.value);
  }
}

// Catch-all, and a rethrow out of a handler with a cleanup in it.
__attribute__((noinline)) void rethrower(int x) {
  try {
    throw_error(x);
  } catch (Error &e) {
    Guard g{"handler"};
    std::printf("rethrowing %d\n", e.value);
    throw;
  }
}
__attribute__((noinline)) void catch_all(int x) {
  try {
    rethrower(x);
  } catch (...) {
    std::printf("caught all\n");
  }
}

// An exception specification that the exception satisfies.
__attribute__((noinline)) void allowed(int x) throw(Error) {
  Guard g{"allowed"};
  throw_error(x);
}
__attribute__((noinline)) void through_filter(int x) {
  try {
    allowed(x);
  } catch (Error &e) {
    std::printf("caught Error %d through a filter\n", e.value);
  }
}

// Two pads in one function: the WRONG-table test points the first call
// at the second pad.
__attribute__((noinline)) void two_pads(int x) {
  try {
    throw_int(x);
  } catch (int v) {
    std::printf("caught int %d in two_pads\n", v);
  }
  try {
    throw_error(x);
  } catch (Error &e) {
    std::printf("caught Error %d in two_pads\n", e.value);
  }
}

// Violations.
__attribute__((noinline)) void forbidden(int x) throw(Error) { throw_int(x); }
__attribute__((noinline)) void no_except(int x) noexcept { throw_int(x); }

int main(int argc, char **argv) {
  if (argc > 1 && std::strcmp(argv[1], "filter") == 0) {
    std::printf("violating a filter\n");
    std::fflush(stdout);
    forbidden(7);
    return 3;
  }
  if (argc > 1 && std::strcmp(argv[1], "noexcept") == 0) {
    std::printf("throwing out of noexcept\n");
    std::fflush(stdout);
    no_except(8);
    return 4;
  }
  by_type(1);
  nested(2);
  catch_all(3);
  through_filter(4);
  two_pads(5);
  std::printf("done\n");
  return 0;
}
