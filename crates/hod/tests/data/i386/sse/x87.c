/* WP-317: the x87-only side of ops.c's mixed calls, built by the cross
   C compiler without SSE (its i686 default). */
double sse_back(double x);
double x87_twice(double x) { return x * 2.0 + 1e-17; }
float x87_half(float x) { return x / 3.0f; }
double x87_calls_back(double x) { return sse_back(x) / 7.0; }
