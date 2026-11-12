/* An attribute soup (spec/ll.md §10): every function and parameter
   attribute clang can be made to write for these targets. */
#include <setjmp.h>
#include <stdarg.h>

struct big { long a[8]; };
struct small { int a; int b; };
struct hfa { double x, y, z; };

__attribute__((noreturn)) void never_returns(void);
__attribute__((cold)) void cold_fn(void) {}
__attribute__((hot)) void hot_fn(void) {}
__attribute__((noinline)) int noinline_fn(int x) { return x; }
__attribute__((always_inline)) inline int always_inline_fn(int x) { return x; }
__attribute__((nothrow)) void nothrow_fn(void) {}
__attribute__((pure)) int pure_fn(const int *p) { return *p; }
__attribute__((const)) int const_fn(int x) { return x * 2; }
__attribute__((malloc, alloc_size(1), alloc_align(2))) void *alloc_fn(unsigned long n, unsigned long a);
__attribute__((returns_nonnull, nonnull(1))) void *nonnull_fn(void *p) { return p; }
__attribute__((returns_twice)) int twice_fn(void);
__attribute__((naked)) void naked_fn(void) { __asm__("ret"); }
__attribute__((no_stack_protector)) void no_ssp_fn(void) {}
__attribute__((no_instrument_function)) void no_instrument_fn(void) {}
__attribute__((patchable_function_entry(4, 2))) void patchable_fn(void) {}
__attribute__((section("fn_section"))) void sectioned_fn(void) {}
__attribute__((aligned(64))) void aligned_fn(void) {}
__attribute__((weak)) void weak_fn(void) {}
__attribute__((visibility("hidden"))) void hidden_fn(void) {}
__attribute__((visibility("protected"))) void protected_fn(void) {}
__attribute__((used)) static void used_fn(void) {}
__attribute__((minsize)) void minsize_fn(void) {}
__attribute__((optnone, noinline)) void optnone_fn(void) {}
__attribute__((flatten)) void flatten_fn(void) { cold_fn(); }
__attribute__((no_sanitize("address"))) void no_sanitize_fn(void) {}
__attribute__((nocf_check)) void nocf_fn(void) {}
#ifdef __x86_64__
__attribute__((no_caller_saved_registers)) void ncsr_user(void);
__attribute__((ms_abi)) int ms_abi_fn(int x) { return x; }
__attribute__((regcall)) int regcall_fn(int x) { return x; }
#else
void ncsr_user(void);
__attribute__((aarch64_vector_pcs)) void vector_pcs_fn(void) {}
#endif
__attribute__((leaf)) void leaf_fn(void);
__attribute__((no_builtin("memcpy"))) void no_builtin_fn(void) {}
__attribute__((no_builtin)) void no_builtins_fn(void) {}
__attribute__((zero_call_used_regs("used"))) void zero_regs_fn(void) {}
__attribute__((nouwtable)) void nouwtable_fn(void) {}
__attribute__((preserve_most)) void preserve_most_fn(void) {}
__attribute__((preserve_all)) void preserve_all_fn(void) {}
__attribute__((fastcall)) void fastcall_fn(void) {}
__attribute__((swiftcall)) void swift_fn(__attribute__((swift_context)) void *ctx) {}
__attribute__((swiftcall)) void swift_error_fn(__attribute__((swift_context)) void *ctx, __attribute__((swift_error_result)) void **err) {}
__attribute__((swiftasynccall)) void swift_async_fn(__attribute__((swift_async_context)) void *ctx) {}
__attribute__((target("no-sse3"))) void target_fn(void) {}
__attribute__((warn_unused_result)) int checked_fn(void) { return 0; }
__attribute__((nodebug)) void nodebug_fn(void) {}
__attribute__((nomerge)) void nomerge_callee(void);
__attribute__((disable_sanitizer_instrumentation)) void no_instr_fn(void) {}
__attribute__((annotate("note"))) void annotated_fn(void) {}
__attribute__((annotate("note"))) int annotated_var = 1;
__attribute__((convergent)) void convergent_fn(void);
__attribute__((speculative_load_hardening)) void slh_fn(void) {}
__attribute__((noduplicate)) void noduplicate_fn(void);
__attribute__((no_split_stack)) void nosplit_fn(void) {}
__attribute__((malloc)) void *malloc_like(unsigned long);
__attribute__((format(printf, 1, 2))) int printf_like(const char *fmt, ...);
__attribute__((sentinel)) void sentinel_fn(int, ...);
static __attribute__((constructor(200), cold, noinline)) void ctor_soup(void) {}

/* Parameters and return values that carry attributes. */
struct big returns_big(void) { struct big b = { { 1 } }; return b; }
long takes_big(struct big b) { return b.a[0]; }
struct small returns_small(void) { struct small s = { 1, 2 }; return s; }
int takes_small(struct small s) { return s.a + s.b; }
struct hfa returns_hfa(void) { struct hfa h = { 1, 2, 3 }; return h; }
double takes_hfa(struct hfa h) { return h.x; }
signed char narrow_signed(signed char a, short b) { return (signed char)(a + b); }
unsigned char narrow_unsigned(unsigned char a, unsigned short b, _Bool c) { return (unsigned char)(a + b + c); }
__int128 wide_arg(__int128 a, long double b, _Float16 c) { return a + (long)b + (long)c; }
int variadic(int n, ...) {
  va_list ap;
  va_start(ap, n);
  int v = va_arg(ap, int);
  va_end(ap);
  return v;
}
void pointer_args(int *restrict a, const int *restrict b, int n, int c[static 4]) { a[0] = b[n] + c[0]; }
void kr_style();
void (*kr_user)() = kr_style;
int uses_setjmp(jmp_buf env) { return setjmp(env); }
void callers(void) {
  nomerge_callee();
  leaf_fn();
  ncsr_user();
  convergent_fn();
  noduplicate_fn();
  (void)twice_fn();
  (void)alloc_fn(8, 8);
  (void)malloc_like(4);
  (void)printf_like("%d", 1);
  sentinel_fn(1, (void *)0);
  kr_style(1, 2);
  if (takes_small(returns_small()) == 99)
    never_returns();
}
