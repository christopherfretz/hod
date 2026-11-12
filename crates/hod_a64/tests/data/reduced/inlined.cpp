// WP-061: code inlined into a function of a LineTablesOnly unit (an
// always_inline function at -O0): its subprogram DIE needs inlined
// subroutines.
__attribute__((always_inline)) inline int bump(int v) { return v + 1; }
int caller(int v) { return bump(v) * 2; }
