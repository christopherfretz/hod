#!/usr/bin/env python3
"""Design 0007: one function per `declare` line of DECLS (immediates 4 for
an `i32 immarg`, 1 otherwise), every AVX-512 feature on."""
import re,sys
out=["target triple = \"x86_64-unknown-linux-gnu\""]
i=0
for l in open(sys.argv[1]):
    l=l.strip()
    m=re.match(r'declare (.*?) @([\w.]+)\((.*)\)$',l)
    if not m: continue
    ret,name,args=m.groups()
    if 'gather' in name or 'scatter' in name: continue
    out.append(l)
    parts=[a.strip() for a in re.split(r',(?![^<]*>)',args)] if args else []
    ps=[];cs=[]
    for j,a in enumerate(parts):
        ty=re.sub(r'\s+(immarg|noundef|nocapture|readonly|writeonly|captures\([^)]*\)|align \d+)','',a).strip()
        if 'immarg' in a: cs.append(f"{ty} {4 if ty=='i32' else 1}")
        else: ps.append(f"{ty} %a{j}"); cs.append(f"{ty} %a{j}")
    r = '' if ret=='void' else f'%r = '
    out.append(f"define {ret} @f{i}({', '.join(ps)}) #0 {{\n  {r}call {ret} @{name}({', '.join(cs)})\n  ret {ret} {'%r' if ret!='void' else ''}\n}}".replace('ret void ','ret void'))
    i+=1
out.append('attributes #0 = { "target-features"="+avx512f,+avx512vl,+avx512bw,+avx512dq,+avx512vbmi,+avx512vbmi2,+avx512fp16,+avx512bf16,+avxneconvert,+vaes,+vpclmulqdq,+gfni,+evex512,+aes,+pclmul" }')
print("\n".join(out))
