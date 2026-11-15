/* The processor a program runs on, as /proc/cpuinfo lists it: a `model
 * name` line (the first argument) and a `flags` line with the vector
 * features hod's AVX-512 execution tests require (hod_testutil::avx512),
 * read from CPUID. Run under an emulator (Intel SDE), it describes the
 * emulated processor, which the emulator's /proc/cpuinfo does not: the
 * file the sde action names as HOD_X64_AVX512_RUNNER_CPUINFO (WP-238,
 * spec/harness.md "CI" item 8). */
#include <cpuid.h>
#include <stdio.h>

struct bit {
    unsigned leaf, sub, reg, bit;
    const char *name;
};

enum { EAX, EBX, ECX, EDX };

static const struct bit bits[] = {
    {1, 0, ECX, 12, "fma"},
    {1, 0, ECX, 28, "avx"},
    {1, 0, ECX, 29, "f16c"},
    {7, 0, EBX, 3, "bmi1"},
    {7, 0, EBX, 5, "avx2"},
    {7, 0, EBX, 8, "bmi2"},
    {7, 0, EBX, 16, "avx512f"},
    {7, 0, EBX, 17, "avx512dq"},
    {7, 0, EBX, 21, "avx512ifma"},
    {7, 0, EBX, 26, "avx512pf"},
    {7, 0, EBX, 27, "avx512er"},
    {7, 0, EBX, 28, "avx512cd"},
    {7, 0, EBX, 30, "avx512bw"},
    {7, 0, EBX, 31, "avx512vl"},
    {7, 0, ECX, 1, "avx512vbmi"},
    {7, 0, ECX, 6, "avx512_vbmi2"},
    {7, 0, ECX, 8, "gfni"},
    {7, 0, ECX, 9, "vaes"},
    {7, 0, ECX, 10, "vpclmulqdq"},
    {7, 0, ECX, 11, "avx512_vnni"},
    {7, 0, ECX, 12, "avx512_bitalg"},
    {7, 0, ECX, 14, "avx512_vpopcntdq"},
    {7, 0, EDX, 8, "avx512_vp2intersect"},
    {7, 0, EDX, 23, "avx512_fp16"},
    {7, 1, EAX, 4, "avx_vnni"},
    {7, 1, EAX, 5, "avx512_bf16"},
};

int main(int argc, char **argv) {
    unsigned max = __get_cpuid_max(0, 0);
    printf("model name\t: %s\nflags\t\t:", argc > 1 ? argv[1] : "unknown");
    for (unsigned i = 0; i < sizeof bits / sizeof *bits; i++) {
        unsigned r[4] = {0, 0, 0, 0};
        if (bits[i].leaf > max) continue;
        __cpuid_count(bits[i].leaf, bits[i].sub, r[EAX], r[EBX], r[ECX], r[EDX]);
        if (r[bits[i].reg] >> bits[i].bit & 1) printf(" %s", bits[i].name);
    }
    printf("\n");
    return 0;
}
