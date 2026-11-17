/* i386 join E (2): variadic calls with `double`, `float` (promoted to
 * `double`) and vector arguments in SSE2 functions (`-march=pentium4`);
 * the variadic callees read them with `va_arg` as clang lowers it. */
#include <stdarg.h>
#include <stdio.h>

typedef float v4f __attribute__((vector_size(16)));
typedef int v4i __attribute__((vector_size(16)));
typedef double v2d __attribute__((vector_size(16)));

static double take(const char *kinds, ...) {
  va_list ap;
  va_start(ap, kinds);
  double acc = 0.0;
  for (const char *k = kinds; *k; ++k) {
    switch (*k) {
    case 'd': acc = acc * 1.5 + va_arg(ap, double); break;
    case 'i': acc -= va_arg(ap, int); break;
    case 'l': acc += (double)va_arg(ap, long long) / 1024.0; break;
    case 'v': {
      v4f v = va_arg(ap, v4f);
      acc += v[0] - v[1] * v[2] + v[3];
      break;
    }
    case 'w': {
      v4i w = va_arg(ap, v4i);
      acc += (double)(w[0] ^ w[3]) - w[1] + w[2];
      break;
    }
    case 'x': {
      v2d x = va_arg(ap, v2d);
      acc += x[0] / x[1];
      break;
    }
    }
  }
  va_end(ap);
  return acc;
}

static float vfloat(int n, ...) {
  va_list ap;
  va_start(ap, n);
  float f = 0.0f;
  for (int i = 0; i < n; ++i)
    f = f * 0.5f + (float)va_arg(ap, double);
  va_end(ap);
  return f;
}

int main(void) {
  float f = 1.1f, g = -0.0f, tiny = 1e-40f;
  double d = 3.25, nd = -1e300;
  v4f v = {1.5f, -2.0f, 0.25f, 1e-39f};
  v4i w = {7, -8, 0x7fffffff, -1};
  v2d x = {9.0, -0.125};
  printf("%.17g\n", take("d", d));
  printf("%.17g\n", take("didl", d, 3, (double)f, 1LL << 40));
  printf("%.17g\n", take("vdv", v, (double)g, v));
  printf("%.17g\n", take("iwxd", 5, w, x, nd));
  printf("%.17g\n", take("dvidwx", (double)tiny, v, -4, d, w, x));
  printf("%.9g %.9g\n", vfloat(3, f, g, tiny), vfloat(4, d, f, nd, (double)f * 2));
  printf("%.9g %.9g %.17g\n", (double)f, (double)tiny, d);
  return 0;
}
