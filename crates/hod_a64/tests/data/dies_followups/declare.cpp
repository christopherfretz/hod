// The test moves each function's `#dbg_declare` from the stack slot to
// another address (spec/codegen.md §9.5, WP-111): a `phi` (what a
// `CoroSplit` ramp declares its frame's variables on), a value from
// another block, a constant offset from a slot.
struct P { long a, b; };
extern "C" P *mk();
extern "C" void use(P *);
extern "C" int cond();
extern "C" void phi() { P *p = mk(); if (cond()) use(p); use(p); }
extern "C" void cross() { P *p = mk(); if (cond()) use(p); }
extern "C" void offset() { P q[2]; use(&q[1]); }
int main() { return 0; }
