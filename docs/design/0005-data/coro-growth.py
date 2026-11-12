"""Instruction counts of each coroutine before and after LLVM's split (WP-100).

usage: python3 -I coro-growth.py <pre.ll> <post.ll> [...pairs]
A coroutine is a function whose attribute group has presplitcoroutine in
<pre>; after the split it is the ramp plus <name>.resume/.destroy/.cleanup.
"""
import re
import sys

ATTR = re.compile(r"^attributes #(\d+) = \{(.*)\}")
DEFINE = re.compile(r"^define [^@]*@(\"[^\"]+\"|[\w.$]+)\((.*)\)(.*)\{")


def bodies(path):
    text = open(path, encoding="utf-8", errors="replace").read()
    presplit = {m.group(1) for line in text.splitlines()
                if (m := ATTR.match(line)) and "presplitcoroutine" in m.group(2)}
    out, cur = {}, None
    for line in text.splitlines():
        m = DEFINE.match(line)
        if m:
            cur = m.group(1).strip('"')
            out[cur] = [bool(set(re.findall(r"#(\d+)", m.group(3))) & presplit), 0]
            continue
        if cur is not None:
            if line.startswith("}"):
                cur = None
            elif line.startswith("  ") and not line.lstrip().startswith("#dbg"):
                out[cur][1] += 1
    return out


pre_total = post_total = coros = 0
args = sys.argv[1:]
for pre_p, post_p in zip(args[::2], args[1::2]):
    pre, post = bodies(pre_p), bodies(post_p)
    for name, (is_coro, n) in pre.items():
        if not is_coro:
            continue
        coros += 1
        after = sum(post.get(name + s, [0, 0])[1] for s in ["", ".resume", ".destroy", ".cleanup"])
        pre_total += n
        post_total += after
print(f"coroutines {coros}: instructions before split {pre_total}, after (ramp + 3 clones) {post_total}, ratio {post_total / max(pre_total, 1):.2f}")
