// Labels (spec/dwarf.md §6.4, §6.14): `goto` targets in a function, in a
// nested block, in an always-inlined function (an abstract label and its
// concrete copy), and in a function without variables.
int sum_down(int x) {
  int r = 0;
again:
  r += x;
  if (--x > 0)
    goto again;
  {
    int twice = r * 2;
    if (twice > 100)
      goto done;
    r = twice;
  }
done:
  return r;
}

static inline __attribute__((always_inline)) int clamp(int x) {
  if (x < 10)
    goto out;
  x = 10;
out:
  return x;
}

static int counter;

void spin(void) {
again:
  counter++;
  if (counter < 3)
    goto again;
done:
  counter += 10;
}

int main(void) {
  int a = sum_down(3);
  int b = clamp(a);
  spin();
  return a + b + counter == 35 ? 0 : 1;
}
