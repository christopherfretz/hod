void g(int);
struct D { ~D(); };
int f(int x) { try { D d; g(x); } catch (int e) { return e; } return 0; }
