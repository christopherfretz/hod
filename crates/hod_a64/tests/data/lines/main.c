int sum_to(int n);
int classify(int v);
int scan(const int *v, int n);
extern int counter;

int main(void) {
  int data[4] = {3, 9, 2, 7};
  int a = sum_to(3);
  int b = classify(1);
  int c = classify(7);
  int d = scan(data, 4);
  return (a + b + c + d + counter) == 0;
}
