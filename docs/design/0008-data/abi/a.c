extern int g; extern void f(int); static int s;
__thread int t; extern __thread int te;
int ret(void){ return 42; }
int use(void){ f(g); return s + t + te; }
long long add(long long a, long long b){ return a+b; }
long long div_(long long a, long long b){ return a/b; }
double fd(double a, float b){ return a*b; }
long double ld(long double a){ return a+1; }
struct S{int a,b,c;}; struct S mk(int x){ struct S s={x,x,x}; return s; }
struct L{long long a; double d; char c;}; int szL(void){ return sizeof(struct L)*100+_Alignof(struct L)*10+__builtin_offsetof(struct L,d); }
int szLD(void){ return sizeof(long double)*10+_Alignof(long double); }
