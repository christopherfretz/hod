/* i386 join E (6): debug information of SSE2 functions
 * (`-march=pentium4 -g`): `float`, `double`, `_Float16` and vector
 * parameters and locals. */
#include <stdio.h>

typedef float v4f __attribute__((vector_size(16)));
typedef double v2d __attribute__((vector_size(16)));
typedef int v4i __attribute__((vector_size(16)));

static double scale(float f, double d, _Float16 h, v4f v, v2d w, v4i n) {
  float ff = f * 2.0f;
  double dd = d + ff;
  _Float16 hh = h * (_Float16)3.0f;
  v4f vv = v * ff;
  v2d ww = w + (v2d){dd, -dd};
  v4i nn = n + (v4i){1, 2, 3, 4};
  return dd + (double)hh + vv[0] + vv[3] + ww[1] + nn[2];
}

static v4f twice(v4f v, float f) {
  v4f out = v + v;
  out[1] = f;
  return out;
}

static _Float16 half_of(_Float16 h) {
  _Float16 r = h / (_Float16)2.0f;
  return r;
}

int main(void) {
  v4f v = {1.0f, -2.0f, 0.5f, 4.0f};
  v2d w = {0.25, 8.0};
  v4i n = {10, 20, 30, 40};
  double r = scale(1.5f, -0.75, (_Float16)2.5f, v, w, n);
  v4f t = twice(v, 9.5f);
  _Float16 h = half_of((_Float16)7.0f);
  printf("%.17g %.9g %.9g %.9g\n", r, t[0], t[1], (double)h);
  return 0;
}
