int counter;

int sum_to(int n) {
  int s = 0;
  for (int i = 1; i <= n; i++) {
    s += i;
  }
  return s;
}

int classify(int v) {
  int r;
  switch (v) {
  case 0:
    r = 10;
    break;
  case 1:
    r = 20;
    break;
  default:
    r = v * 3;
  }
  counter += r;
  return r;
}

int scan(const int *v, int n) {
  int best = 0;
  int i = 0;
  while (i < n) {
    if (v[i] > best)
      best = v[i];
    else
      counter++;
    i++;
  }
  return best;
}
