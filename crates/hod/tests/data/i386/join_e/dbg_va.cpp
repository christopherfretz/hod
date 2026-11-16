// i386 join E: debug information of join D's constructs. Variadic
// functions (`va_start`, `va_arg` of `int`, `long long`, `double`, a
// pointer, `va_copy`) and structs passed and returned by value; built by
// clang at `-O0 -g`.
#include <cstdarg>
#include <cstdio>

struct Small {
  short a;
  char b;
};

struct Big {
  int x[5];
  double d;
};

static long long sum(int count, ...) {
  va_list ap;
  va_start(ap, count);
  va_list again;
  va_copy(again, ap);
  long long s = 0;
  for (int i = 0; i < count; ++i) {
    switch (i % 4) {
    case 0: s += va_arg(ap, int); break;
    case 1: s += va_arg(ap, long long); break;
    case 2: s += static_cast<long long>(va_arg(ap, double) * 10.0); break;
    default: s += *va_arg(ap, const int *); break;
    }
  }
  int first = va_arg(again, int);
  va_end(again);
  va_end(ap);
  return s * 3 + first;
}

static Big widen(Small s, Big b) {
  Big r = b;
  for (int i = 0; i < 5; ++i)
    r.x[i] += s.a * i + s.b;
  r.d = b.d * 0.5 + s.a;
  return r;
}

static Small shrink(Big b) {
  Small s{static_cast<short>(b.x[0] + b.x[4]), static_cast<char>(b.x[2])};
  return s;
}

int main() {
  int k = 41;
  long long v = sum(8, 1, 1LL << 40, 2.75, &k, -5, -(1LL << 35), -0.5, &k);
  Small s{300, 7};
  Big b{{1, 2, 3, 4, 5}, 6.25};
  Big w = widen(s, b);
  Small t = shrink(w);
  std::printf("%lld %d %d %d %d %d %g %d %d\n", v, w.x[0], w.x[1], w.x[2], w.x[3], w.x[4],
              w.d, t.a, t.b);
  return 0;
}
