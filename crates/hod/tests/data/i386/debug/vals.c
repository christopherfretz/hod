// WP-314: the locations of an i64, float and double parameters and
// locals, an sret result, a struct expanded into its fields and a byval
// argument on i386.
typedef struct { int a, b, c, d; } Quad;
typedef struct { int v[6]; double x; } Big;
long long g64 = 0x100000002LL;
long long wide(long long x, int k) { long long y = x * k + g64; return y; }
float scale(float f, double d) { float r = f * 2.0f; double q = d + r; return (float)q; }
Quad make(int base) { Quad q = { base, base + 1, base + 2, base + 3 }; return q; }
int sum(Quad q, int k) { int s = q.a + q.b + q.c + q.d; return s + k; }
int big(Big b, int k) { int t = b.v[0] + b.v[5] + (int)b.x; return t * k; }
int main(void) {
  long long w = wide(3, 4);
  float f = scale(1.5f, 2.25);
  Quad q = make(10);
  int s = sum(q, 1);
  Big b = { { 1, 2, 3, 4, 5, 6 }, 7.0 };
  int t = big(b, 2);
  return (int)(w - 12 - g64) + (int)f - 5 + s - 47 + t - 28;
}
