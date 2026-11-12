struct Pair { int a; long b; };
struct Big { long x[5]; };
struct Many { float f[4]; };

int fact(int n);
int shadow(int v);
long by_value(struct Pair p, struct Big big, double d, float g);
long on_stack(long a0, long a1, long a2, long a3, long a4, long a5,
              long a6, long a7, struct Pair spilled, long last);
double floats(float f, double d, struct Many m);
int sum_ints(int count, ...);
int pointers(int *p, const char *s, int arr[3]);
int labels(int n);
int local_type(int v);

int main(void) {
  struct Pair p = {3, 40};
  struct Big big = {{1, 2, 3, 4, 5}};
  struct Many m = {{1.5f, 2.5f, 3.5f, 4.5f}};
  int data[3] = {11, 12, 13};
  long r = fact(4);
  r += shadow(2);
  r += by_value(p, big, 1.25, 2.5f);
  r += on_stack(1, 2, 3, 4, 5, 6, 7, 8, p, 9);
  r += (long)floats(0.5f, 0.25, m);
  r += sum_ints(3, 10, 20, 30);
  r += pointers(data, "hi", data);
  r += labels(3);
  r += local_type(6);
  return r == 0;
}
