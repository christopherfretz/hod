/* Debug-information survey input, C (spec/ir.md §6; tests/debug_survey.rs).
   No header is included, so the IR depends only on the compiler. */
typedef unsigned long size_type;
typedef int (*callback)(int, void *);
enum color { RED, GREEN = 5, BLUE = -1 };
enum big : unsigned long long { HUGE_VALUE = 0xffffffffffffffffULL };
struct bits { unsigned a : 3; unsigned b : 5; int : 0; signed c : 7; };
union overlay { int i; float f; char bytes[4]; };
struct node { struct node *next; const char *name; struct opaque *hidden; union overlay u; };
struct flex { int n; long tail[]; };
struct packed_s { char c; int i; } __attribute__((packed));
struct aligned_s { int x __attribute__((aligned(16))); };
typedef struct { double re, im; } complex_t;
typedef int vec4 __attribute__((vector_size(16)));

int global_counter = 1;
static const double static_const = 2.5;
static int file_static;
__thread int tls_counter;
extern int external_var;
char text[] = "text";
const volatile int cv_qualified = 3;
int *restrict *ptr_to_restrict;
_Atomic int atomic_int;
_Complex double complex_var;
__int128 wide_var;
_Bool flag_var;
vec4 vector_var;
int matrix[3][4];
complex_t typedef_var;
struct aligned_s aligned_var;
struct packed_s packed_var;
callback cb_var;

__attribute__((always_inline)) static inline int leaf(int x) { int twice = x * 2; return twice + 1; }
__attribute__((always_inline)) static inline int middle(int y) { int m = leaf(y) + leaf(y + 1); return m; }
int outer(int z) { return middle(z) + middle(z * 2); }

__attribute__((noreturn)) void die(const char *why);
static int helper(struct node *n, enum color c) {
  int depth = 0;
  for (struct node *p = n; p; p = p->next) { int local = depth; depth = local + 1; }
  { int depth2 = depth; if (c == RED) depth = depth2 * 2; }
  return depth;
}
int vla(int n) { int array[n]; long two[n][n + 1]; array[0] = n; two[0][0] = 1; return array[0] + (int)sizeof(two); }
struct flex *flexible(struct flex *f) { return f; }
complex_t ret_struct(double a) { complex_t r = { a, -a }; return r; }
struct big_s { long a[8]; };
struct big_s by_value(struct big_s in, int k) { in.a[0] = k; return in; }
int with_bits(struct bits *b) { return b->a + b->b + b->c; }
int variadic(int count, ...) {
  __builtin_va_list ap; __builtin_va_start(ap, count); int total = 0;
  for (int i = 0; i < count; i++) total += __builtin_va_arg(ap, int);
  __builtin_va_end(ap); return total;
}
int labels(int x) {
  if (x > 3) goto done;
  x += 1;
again:
  x *= 2;
  if (x < 100) goto again;
done:
  return x;
}
int statics(void) { static int calls; static const char *const names[] = { "a", "b" }; calls++; return calls + names[0][0]; }
int switcher(enum color c, enum big b) { switch (c) { case RED: return 1; case GREEN: return 2; default: return (int)b; } }
int uses_callback(callback f, void *ctx) { return f(1, ctx); }
int old_style();
int old_style(a, b) int a; char b; { return a + b; }
int main(int argc, char **argv) {
  struct node head = { 0, argv[0], 0, { argc } };
  if (argc > 100) die("too many");
  return helper(&head, GREEN) + outer(argc) + vla(argc) + labels(argc) + statics() + file_static
       + (int)static_const + tls_counter + external_var;
}
