/* WP-338: vectors at the boundary without SSE (spec/codegen-x86.md
   section V): each lane an argument of its own; results in eax, edx,
   ecx, st0, or memory. The callee side; lanes.c calls them. */
typedef int v4i __attribute__((vector_size(16)));
typedef long long v2q __attribute__((vector_size(16)));
typedef float v2f __attribute__((vector_size(8)));
typedef double v1d __attribute__((vector_size(8)));
typedef double v4d __attribute__((vector_size(32)));
typedef char v32b __attribute__((vector_size(32)));
typedef short v2h __attribute__((vector_size(4)));
typedef int v2i __attribute__((vector_size(8)));

v4i mix(v2f a, int k, v4i b, v1d c, v2q d, v4d e) {
    v4i r = b;
    r[0] += k;
    r[1] ^= (int)d[1];
    r[2] = (int)(a[1] * 4.0f);
    r[3] = (int)(c[0] + e[3]);
    return r;
}

v32b bytes(char seed) {
    v32b r;
    for (int i = 0; i < 32; i++)
        r[i] = (char)(seed * i + 3);
    return r;
}

v2h halves(v2h a) { return a + (v2h){7, -9}; }
v2i pair(v2i a, v2i b) { return a - b; }
v1d one(v1d a) { return a; }
