int full_fn(int);
int lto_fn(int);
int ddo_fn(int);
int none_fn(int x) { return x ^ 3; }
int main() { return full_fn(1) + lto_fn(2) + ddo_fn(3) + none_fn(3) == 8 ? 0 : 1; }
