/* WP-246: the checks of `tests/win64cc.rs` (`spec/abi-x64.md` §13.6).
 * Before this text the test writes `CASES` (each callee: its snapshot
 * routine, its address, the kind of its result, whether it is `win64cc`,
 * and where its unwind rows say each `xmm` register is saved, as an
 * offset from its CFA, 0 for none) and `NCASES`. A `win64cc` callee
 * gets `a` in `rcx` and `b` in `rdx` (an `sret` or return address in
 * `rcx`, then `a` in `rdx` and `b` in `r8`), and must keep `rbx`, `rsi`,
 * `rdi`, `r12`..`r15` and `xmm6`..`xmm15`; the `ccc` control gets them
 * in `rdi` and `rsi` and keeps `rbx`, `r12`..`r15`. */
#include <stdint.h>
#include <stdio.h>
#include <string.h>
#include <unwind.h>

enum { RAX, RBX, RCX, RDX, RSI, RDI, R8, R9, R10, R11, R12, R13, R14, R15, XMM = 14 };
/* The DWARF numbers of the general registers in the order above. */
static const int DWARF[14] = {0, 3, 2, 1, 4, 5, 8, 9, 10, 11, 12, 13, 14, 15};

int64_t sink;
static const struct fn_case *cur;
static const uint64_t *cur_in;
static unsigned cur_gprs, cur_xmms;
static int failures, probed;

/* The registers a callee of `kind` returns in, as masks over the order
 * above and over the `xmm` registers. */
static void returns(int kind, unsigned *gprs, unsigned *xmms) {
    *gprs = 0;
    *xmms = 0;
    switch (kind) {
    case K_I64: case K_I32: case K_MEM: *gprs = 1u << RAX; break;
    case K_I128: *gprs = 1u << RAX | 1u << RDX; break;
    case K_F64: case K_F32: case K_V4F: *xmms = 1; break;
    case K_DD: *xmms = 3; break;
    case K_ID: *gprs = 1u << RAX; *xmms = 1; break;
    case K_III: *gprs = 1u << RAX | 1u << RDX | 1u << RCX; break;
    default: break;
    }
}

struct walk { int after; };

/* As in `../preserve_cc/driver.c`: the snapshot routine's registers as
 * the callee's rows recover them, and each kept `xmm` where its row
 * says, at the callee's CFA. */
static _Unwind_Reason_Code step(struct _Unwind_Context *ctx, void *arg) {
    struct walk *w = arg;
    if (w->after) {
        for (int r = 0; r < 14; r++) {
            if (cur_gprs >> r & 1 && _Unwind_GetGR(ctx, DWARF[r]) != cur_in[r]) {
                printf("%s: unwound register %d is %#lx, not %#lx\n", cur->name, r,
                       (unsigned long)_Unwind_GetGR(ctx, DWARF[r]), (unsigned long)cur_in[r]);
                failures++;
            }
        }
        uintptr_t cfa = _Unwind_GetCFA(ctx);
        for (int k = 0; k < 16; k++) {
            if (!(cur_xmms >> k & 1)) continue;
            if (cur->xoff[k] == 0
                || memcmp((const void *)(cfa + cur->xoff[k]), &cur_in[XMM + 2 * k], 16) != 0) {
                printf("%s: xmm%d is not where its row says (%d)\n", cur->name, k, cur->xoff[k]);
                failures++;
            }
        }
        probed++;
        return _URC_END_OF_STACK;
    }
    if (_Unwind_GetRegionStart(ctx) == (uintptr_t)cur->fn) w->after = 1;
    return _URC_NO_REASON;
}

void probe(void) {
    struct walk w = {0};
    if (!cur) return; /* the callers' sums, after the cases */
    _Unwind_Backtrace(step, &w);
}

int main(void) {
    for (int c = 0; c < NCASES; c++) {
        const struct fn_case *f = &CASES[c];
        uint64_t in[48], out[48];
        int64_t buffer = 0, memory[4] = {0, 0, 0, 0};
        for (int i = 0; i < 48; i++) in[i] = 0x0101010101010101ull * (uint64_t)(i + 1) ^ (uint64_t)c << 40;
        int64_t a = 1000 + 7 * c, b = -300 - c;
        if (!f->win64) {
            in[RDI] = (uint64_t)a; in[RSI] = (uint64_t)b;
        } else if (f->kind == K_SRET || f->kind == K_MEM) {
            int64_t *to = f->kind == K_SRET ? &buffer : memory;
            in[RCX] = (uint64_t)(uintptr_t)to; in[RDX] = (uint64_t)a; in[R8] = (uint64_t)b;
        } else {
            in[RCX] = (uint64_t)a; in[RDX] = (uint64_t)b;
        }
        unsigned rg, rx;
        returns(f->kind, &rg, &rx);
        cur = f; cur_in = in;
        cur_gprs = (f->win64 ? 0x3c32u : 0x3c02u) & ~rg;
        cur_xmms = f->win64 ? 0xffc0u & ~rx : 0;
        sink = 0;
        probed = 0;
        memset(out, 0, sizeof out);
        f->snap(in, out);
        if (probed != 1) { printf("%s: the walk did not reach the caller\n", f->name); failures++; }
        for (int r = 0; r < 14; r++) {
            if (cur_gprs >> r & 1 && out[r] != in[r]) {
                printf("%s: register %d is %#lx, not %#lx\n", f->name, r, (unsigned long)out[r],
                       (unsigned long)in[r]);
                failures++;
            }
        }
        for (int k = 0; k < 16; k++) {
            if (cur_xmms >> k & 1 && memcmp(&out[XMM + 2 * k], &in[XMM + 2 * k], 16) != 0) {
                printf("%s: xmm%d changed\n", f->name, k);
                failures++;
            }
        }
        double d0, d1; float v[4]; long double e;
        memcpy(&d0, &out[XMM], 8); memcpy(&d1, &out[XMM + 2], 8);
        memcpy(v, &out[XMM], 16); memcpy(&e, &out[46], 10);
        int ok = 1;
        switch (f->kind) {
        case K_VOID: case K_PLAIN: ok = sink == b; break;
        case K_I64: ok = (int64_t)out[RAX] == a + b; break;
        case K_I32: ok = (int32_t)out[RAX] == (int32_t)(a + b); break;
        case K_I128: ok = (int64_t)out[RAX] == b && (int64_t)out[RDX] == a; break;
        case K_F64: ok = d0 == (double)(a + b); break;
        case K_F32: ok = v[0] == (float)(a + b); break;
        case K_DD: ok = d0 == (double)a && d1 == (double)b; break;
        case K_ID: ok = (int64_t)out[RAX] == a && d0 == (double)b; break;
        case K_III: ok = (int64_t)out[RAX] == a && (int64_t)out[RDX] == b && (int64_t)out[RCX] == a + b; break;
        case K_V4F: ok = v[0] == (float)a && v[1] == (float)b && v[2] == (float)(a + b) && v[3] == (float)(a - b); break;
        case K_FP80: ok = e == (long double)(a + b); break;
        case K_SRET: ok = buffer == a + b; break;
        case K_MEM:
            ok = out[RAX] == (uint64_t)(uintptr_t)memory && memory[0] == a && memory[1] == b
                 && memory[2] == a + b && memory[3] == 7;
            break;
        }
        if (!ok) { printf("%s: wrong result\n", f->name); failures++; }
    }
    cur = NULL;
    printf("callers %ld %ld\n", caller_hod(5, -9), caller_llc(5, -9));
    printf("checked %d, %d failures\n", NCASES, failures);
    return failures != 0;
}
