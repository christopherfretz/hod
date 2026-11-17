/* i386 join E (3) and (5): aggregates with `float`, `double` and vector
 * leaves by value, in memory and as results (`sret`), their fields read
 * and written; `memcpy` of a struct with a vector member; a vector local
 * live across a dynamic `alloca`; in SSE2 functions (`-march=pentium4`). */
#include <stdio.h>
#include <string.h>

typedef float v4f __attribute__((vector_size(16)));
typedef double v2d __attribute__((vector_size(16)));

struct FD {
  float f;
  double d;
};

struct WithV {
  int tag;
  v4f v;
  double d;
};

struct Two {
  float a, b;
};

static struct FD mix(struct FD x, float s) {
  struct FD r = {x.f * s, x.d - s};
  return r;
}

static struct WithV bump(struct WithV w, v4f by) {
  w.tag += 1;
  w.v = w.v + by;
  w.d *= -2.0;
  return w;
}

static struct Two swap(struct Two t) {
  struct Two r = {t.b, t.a};
  return r;
}

static void show_v(const char *what, v4f v) {
  printf("%s %.9g %.9g %.9g %.9g\n", what, v[0], v[1], v[2], v[3]);
}

static double across_alloca(int n, v4f seed) {
  v4f keep = seed * seed;
  v2d other = {seed[0], seed[3]};
  double acc = 0;
  for (int round = 1; round <= 3; ++round) {
    float buf[n * round];
    for (int i = 0; i < n * round; ++i)
      buf[i] = keep[i % 4] + i;
    for (int i = 0; i < n * round; ++i)
      acc += buf[i];
    keep = keep + (v4f){0.5f, -0.5f, 1.0f, -1.0f};
    other = other * (v2d){1.5, -2.0};
  }
  show_v("keep", keep);
  return acc + other[0] + other[1];
}

int main(void) {
  struct FD fd = {1.25f, -3.5};
  struct FD m = mix(fd, 0.1f);
  printf("%.9g %.17g\n", m.f, m.d);
  struct WithV w = {3, {1.0f, -0.0f, 1e-40f, 2.5f}, 0.75};
  struct WithV b = bump(w, (v4f){0.25f, 0.25f, 1e-40f, -8.0f});
  printf("%d %.17g\n", b.tag, b.d);
  show_v("b", b.v);
  struct WithV copy;
  memcpy(&copy, &b, sizeof copy);
  copy.v[2] = copy.v[1] - 1.0f;
  show_v("copy", copy.v);
  struct WithV arr[3];
  for (int i = 0; i < 3; ++i) {
    arr[i] = bump(copy, (v4f){(float)i, 0, 0, 0});
    memcpy(&copy, &arr[i], sizeof copy);
  }
  show_v("arr2", arr[2].v);
  printf("%d %.17g\n", arr[2].tag, arr[2].d);
  struct Two t = swap((struct Two){-1.5f, 4.0f});
  printf("%.9g %.9g\n", t.a, t.b);
  printf("%.17g\n", across_alloca(5, (v4f){1.0f, 2.0f, -3.0f, 0.5f}));
  return 0;
}
