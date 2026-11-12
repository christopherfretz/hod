/* Globals of every shape (spec/ll.md §10): what clang makes of this file
   is the differential corpus for initializers, symbols and sections. */
#include <stddef.h>
#include <stdint.h>

/* Integers of every width, signed and not. */
char c0 = 'a';
signed char c1 = -1;
unsigned char c2 = 255;
short s0 = -12345;
unsigned short s1 = 54321;
int i0 = -1;
unsigned i1 = 0xdeadbeef;
long l0 = -0x123456789abcdefL;
unsigned long l1 = 0xfedcba9876543210UL;
__int128 w0 = ((__int128)0x0123456789abcdefLL << 64) | 0xfedcba9876543210ULL;
unsigned __int128 w1 = ~(unsigned __int128)0;
__int128 w2 = -1;
#ifdef __x86_64__
_BitInt(200) wide0 = -12345678901234567890wb;
unsigned _BitInt(129) wide1 = 0x1ffffffffffffffffffffffffffffffffuwb;
#endif
_BitInt(100) wide2 = -1234567890123456789012345wb;
_BitInt(7) narrow0 = -3;
_Bool b0 = 1;

/* Floating point of every format. */
float f0 = 1.5f;
float f1 = -0.0f;
float f2 = 0.1f;
float f3 = __builtin_inff();
float f4 = __builtin_nanf("");
float f5 = 1e-45f;
double d0 = 3.141592653589793;
double d1 = -1e308;
double d2 = 4.9e-324;
double d3 = __builtin_nan("0x1234");
long double ld0 = 1.0L;
long double ld1 = -2.5e100L;
long double ld2 = 0.1L;
_Float16 h0 = 1.5f16;
_Float16 h1 = -65504.0f16;

/* Strings. */
char str0[] = "hello";
const char str1[] = "with \"quotes\" and \\ and \n and \x01\xff";
const char *str2 = "pointed to";
const char *str3 = "pointed to";
const int wstr[] = { 'w', 'i', 'd', 'e', 0 };
const unsigned short ustr[] = u"utf16";
const char padded[16] = "short";
char zeros[100];
const char brace_string[] = "} { ; \" /* */";

/* Aggregates with padding. */
struct padded { char a; int b; char c; long d; short e; };
struct padded p0 = { 1, 2, 3, 4, 5 };
struct nested { struct padded in[2]; char tail; };
struct nested n0 = { { { 1, 2, 3, 4, 5 }, { 6, 7, 8, 9, 10 } }, 11 };
struct __attribute__((packed)) packed { char a; int b; short c; long d; };
struct packed k0 = { 1, 2, 3, 4 };
struct bits { unsigned a : 3; int b : 13; unsigned long c : 40; char d; };
struct bits bf0 = { 5, -100, 0x123456789aUL, 7 };
union u { char c; double d; void *p; char bytes[24]; };
union u u0 = { .c = 9 };
union u u1 = { .d = 2.0 };
union u u2 = { .p = &i0 };
struct flex { int n; int tail[]; };
struct flex fx0 = { 3, { 1, 2, 3 } };
int array2[3][4] = { { 1, 2, 3, 4 }, { 5, 6 }, { 0 } };
long sparse[64] = { [3] = 3, [40] = 40 };
struct empty {};
struct empty e0;
struct aligned { char c; } __attribute__((aligned(32)));
struct aligned al0 = { 4 };
typedef int v4i __attribute__((vector_size(16)));
v4i vec0 = { 1, 2, 3, 4 };
typedef float v4f __attribute__((vector_size(16)));
v4f vec1 = { 1.0f, 2.0f, 3.0f, 4.0f };
typedef char v3c __attribute__((vector_size(8)));
v3c vec2 = { 1, 2, 3 };

/* Addresses. */
extern int ext0;
extern int ext_array[];
int *ptr0 = &i0;
int *ptr1 = &ext0;
int *ptr2 = &array2[1][2];
int *ptr3 = &ext_array[1000];
int *ptr4 = &ext_array[-1];
char *ptr5 = &str0[3];
void *ptr6 = (void *)0x1000;
void *ptr7 = 0;
uintptr_t int_of_ptr = (uintptr_t)&i0 + 8;
unsigned int_of_ptr32[2] = { 1, 2 };
struct with_ptrs { char tag; void *a; int n; void *b; };
struct with_ptrs wp0 = { 1, &p0.d, 2, &n0.in[1].c };
int (*fptr0)(void);
void forward(void);
void (*fptr1)(void) = forward;
void *self_ref = &self_ref;
const void *const table_of_ptrs[] = { &c0, &s0, &i0, &l0, str0 + 1, 0, forward };

/* Linkage, visibility, sections, alignment. */
static int static0 = 5;
static const int static1 = 6;
int *static_user = &static0;
const int *static_user1 = &static1;
__attribute__((weak)) int weak0 = 7;
extern int weak_ref __attribute__((weak));
int *weak_user = &weak_ref;
__attribute__((visibility("hidden"))) int hidden0 = 8;
__attribute__((visibility("protected"))) int protected0 = 9;
extern int hidden_ext __attribute__((visibility("hidden")));
int *hidden_user = &hidden_ext;
__attribute__((section("my_section"))) int in_section = 10;
__attribute__((section("my_section"))) int in_section2 = 11;
__attribute__((section(".rodata.custom"))) const int in_rodata = 12;
__attribute__((aligned(64))) int aligned0 = 13;
__attribute__((aligned(1))) long underaligned = 14;
__attribute__((used)) static int used0 = 15;
__attribute__((used, retain)) static int retained0 = 16;
int common0;
int tentative[10];
const int const0 = 17;
const double const_d = 2.5;
const long const_array[] = { 1, 2, 3, 4, 5, 6, 7, 8, 9 };
char big_zero[5000];
char big_data[300] = { 1 };

/* Thread-local storage in every model. */
_Thread_local int tls0 = 1;
_Thread_local int tls_zero;
static _Thread_local int tls_static = 2;
__attribute__((tls_model("local-dynamic"))) _Thread_local int tls_ld = 3;
__attribute__((tls_model("initial-exec"))) _Thread_local int tls_ie = 4;
__attribute__((tls_model("local-exec"))) _Thread_local int tls_le = 5;
extern _Thread_local int tls_ext;
int tls_reader(void) { return tls0 + tls_zero + tls_static + tls_ld + tls_ie + tls_le + tls_ext; }

/* Aliases and an IFUNC. */
int aliased = 18;
extern int alias0 __attribute__((alias("aliased")));
extern int alias_weak __attribute__((weak, alias("aliased")));
extern int alias_hidden __attribute__((alias("aliased"), visibility("hidden")));
struct padded aliased_struct = { 1, 2, 3, 4, 5 };
extern long alias_member __attribute__((alias("aliased_struct")));
void forward(void) {}
void alias_fn(void) __attribute__((alias("forward")));
static void local_alias(void) __attribute__((alias("forward")));
void (*alias_user)(void) = local_alias;
static void *resolver(void) { return forward; }
void ifunc0(void) __attribute__((ifunc("resolver")));
void (*ifunc_user)(void) = ifunc0;

/* Constructors and destructors with priorities. */
__attribute__((constructor)) static void ctor_default(void) {}
__attribute__((constructor(101))) static void ctor_101(void) {}
__attribute__((constructor(65534))) void ctor_late(void) {}
__attribute__((destructor)) static void dtor_default(void) {}
__attribute__((destructor(7))) static void dtor_7(void) {}

/* A table of label differences and block addresses. */
int labels(int n) {
  static const void *const absolute[] = { &&first, &&second, &&third };
  static const int relative[] = { &&first - &&first, &&second - &&first, &&third - &&first };
  goto *(&&first + relative[n % 3]);
first:
  return 1;
second:
  return (int)(intptr_t)absolute[1];
third:
  return 3;
}

int reader(void) { return static0 + static1 + used0 + retained0 + (int)sizeof(struct empty); }
