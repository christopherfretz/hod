inline int shared(int x) {
  int r = x * 3;
  if (r > 20)
    r -= 20;
  return r;
}
