// WP-314: a throw through three frames and a catch, on i386.
struct E { int code; };
static int depth(int n) { if (n == 0) throw E{n + 7}; int local = n * 2; return depth(n - 1) + local; }
int main() {
  int caught = 0;
  try { depth(3); } catch (const E &e) { caught = e.code; }
  return caught - 7;
}
