#!/usr/bin/env python3
"""Design 0007, section 5.1: the instructions `llc -O0` writes per family.

`gen_calls.py DECLS > all.ll` writes one function per declared intrinsic;
`llc-22 -O0 all.ll -o all22.s`; then `llc_mnemonics.py DECLS all22.s`
prints, per family (`families.py`), the distinct instructions other than
moves and argument glue.
"""
import re,collections,sys
src=open(__file__.replace('llc_mnemonics.py','families.py')).read().split('mods=collections')[0]
exec(src)
names=[]
for l in open(sys.argv[1]):
    m=re.match(r'declare (.*?) @([\w.]+)\(',l.strip())
    if m and 'gather' not in m.group(2) and 'scatter' not in m.group(2): names.append(m.group(2))
cur=None; per=collections.defaultdict(set)
glue=re.compile(r'^(v?mov|kmov|ret|vzeroupper|vpmov[bwdq]2m|vpmovm2[bwdq]|movb|movw|vmovd|push|pop|sub|add|lea|and)')
for l in open(sys.argv[2]):
    m=re.match(r'^f(\d+):',l)
    if m: cur=int(m.group(1)); continue
    m=re.match(r'^\s+([a-z][a-z0-9]*)\s',l)
    if m and cur is not None and not glue.match(m.group(1)) or (m and cur is not None and re.match(r'vmovdqu(8|16|32|64)|vmovup[sd]',m.group(1)) and 'masked' in names[cur]):
        per[fam(names[cur])].add(m.group(1))
tot=set()
for f,s in sorted(per.items()): print(f,len(s),' '.join(sorted(s))); tot|=s
print('total distinct',len(tot))
