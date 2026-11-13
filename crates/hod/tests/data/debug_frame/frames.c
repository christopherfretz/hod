// WP-150: functions without unwind tables in a `-g` module
// (`spec/eh.md` §10): a leaf, a function with a larger frame, callers.
static int leaf(int x) {
  int y = x * 3;
  return y + 1;
}

int with_locals(int a, int b) {
  int arr[40];
  for (int i = 0; i < 40; i++)
    arr[i] = a * i + b;
  return arr[a % 40] + arr[b % 40];
}

int calls(int x) { return leaf(x) + with_locals(x, 2); }

int main(void) { return calls(3) == 10 ? 0 : 1; }
