// Globals only (spec/dwarf.md §8.6): base and pointer types, namespaces.
int counter = 3;
static int hidden = 11;
int *hidden_ptr = &hidden;
double dval = 2.5;
double *dp = &dval;
int *ip = &counter;
int **pp = &ip;
void *vp = &counter;
char letter = 'x';
char *lp = &letter;
signed char sc = -5;
unsigned char uc = 200;
bool flag = true;
short sh = -300;
unsigned short us = 40000;
long lg = -1234567890123;
unsigned long long ull = 18446744073709551615ull;
float fl = 0.5f;
long double ld = 1.25L;
wchar_t wc = L'w';
char16_t c16 = u'q';
char32_t c32 = U'z';
__int128 i128 = 42;
alignas(32) int aligned = 9;
namespace outer {
int depth = 1;
namespace inner {
long deep = 7;
int *deep_ptr = &depth;
}
unsigned short half = 65535;
}
namespace outer::inner::innermost {
int bottom = -1;
}
inline namespace v1 {
int inl = 5;
}
namespace {
int anon = 2;
}
int *anon_ptr = &anon;
