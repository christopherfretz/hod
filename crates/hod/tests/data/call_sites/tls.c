// WP-150: a function that describes its calls and reads thread-local
// variables (`spec/dwarf.md` §6.21). The model's runtime calls
// (`__tls_get_addr`, the TLS descriptor's `blr`) get no call site from
// `llc` in any model on either target; the call of `ext` does.
__thread int t_counter;
static __thread int t_local;
extern __thread int t_ext;
int ext(int);

int touch(int a) {
  t_counter += a;
  t_local += ext(a);
  return t_counter + t_local + t_ext;
}
