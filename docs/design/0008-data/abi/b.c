struct C{char c;}; struct I2{int a,b;}; struct D{double d;}; struct F{float f;}; struct E{}; struct B{int a[5];};
struct C rc(void); struct I2 ri2(void); struct D rd(void); struct F rf(void); struct B rb(void);
void tc(struct C,struct I2,struct D,struct F,struct B);
int use(void){ tc(rc(),ri2(),rd(),rf(),rb()); return 0; }
typedef float v4f __attribute__((vector_size(16))); typedef int v2i __attribute__((vector_size(8)));
v4f vf(v4f a, v4f b, v4f c, v4f d){ return a+d; }
v2i v2(v2i a){ return a; }
long double cld(long double a){return a;}
_Complex float cf(_Complex float a){return a;}
_Complex double cd(_Complex double a){return a;}
__attribute__((regparm(3))) int rp(int a,int b,int c,int d){return a+d;}
__attribute__((fastcall)) int fc(int a,int b,int c){return a+c;}
__attribute__((stdcall)) int sc(int a,int b){return a+b;}
__attribute__((thiscall)) int tcl(int a,int b){return a+b;}
__attribute__((regcall)) int rcl(int a,int b){return a+b;}
__attribute__((preserve_most)) int pm(int a){return a;}
__attribute__((vectorcall)) int vc(int a){return a;}
#include <stdarg.h>
int va(int n, ...){ va_list ap; va_start(ap,n); int r=va_arg(ap,int); double d=va_arg(ap,double); va_end(ap); return r+(int)d; }
