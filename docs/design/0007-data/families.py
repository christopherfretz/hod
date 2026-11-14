#!/usr/bin/env python3
r"""Design 0007, section 5.1: the refused AVX-512 names of the census by
family, and the modules freed when the families are built in order.

Input: a TSV of `<module id>\t<intrinsic name>`, one line per intrinsic a
saved module declares among those hod refuses after WP-202, made by

    grep -o '^declare[^@]*@llvm\.[A-Za-z0-9_.]*' MODULE | sed 's/.*@//' |
      grep -E '^llvm\.(x86\.avx512|x86\.aesni\..*\.(256|512)$|x86\.pclmulqdq\.(256|512)$|x86\.vgf2p8.*\.512$|x86\.vcvtneps2bf16|x86\.avx2\.gather|masked\.)'

for each distinct module. Usage: families.py DECLS.tsv ORDER (the
families, comma-separated, in the order they are built).
"""
import re,sys,collections
def fam(n):
    n=re.sub(r'^llvm\.(x86\.)?','',n)
    if n.startswith('masked.compressstore') or n.startswith('masked.expandload'): return 'masked-compress'
    if n.startswith('masked.'): return 'masked-loadstore'
    if n.startswith('avx2.gather'): return 'avx2-gather(WP-207)'
    if n.startswith('aesni.'): return 'VAES'
    if n.startswith('pclmulqdq'): return 'VPCLMULQDQ'
    if n.startswith('vgf2p8'): return 'GFNI-512'
    if n.startswith('avx512fp16'): return 'FP16'
    if n.startswith('avx512bf16') or n.startswith('vcvtneps2bf16'): return 'BF16'
    if re.search(r'(permvar|vpermi2var)\.qi|pmultishift',n): return 'VBMI'
    if re.match(r'avx512\.mask\.compress\.v\d+i(8|16)$',n): return 'VBMI2'
    if re.search(r'fpclass|q2ps|2u?qq',n): return 'DQ'
    if re.search(r'\.w\.|\.w$|\.b\.512|pack(ss|us)(wb|dw)\.512|psad\.bw|hr\.sw|\.hi\.|dbpsadbw',n): return 'BW'
    if n.startswith('avx512.mask.gather') or n.startswith('avx512.mask.scatter'): return 'F-gather/scatter'
    return 'F/VL'
mods=collections.defaultdict(set); names=collections.defaultdict(set)
for l in open(sys.argv[1]):
    m,n=l.split(); mods[m].add(n); names[fam(n)].add(n)
order=sys.argv[2].split(',')
for f in names: 
    if f not in order: print('UNORDERED',f)
done=set()
def freed(done): return sum(1 for m,s in mods.items() if all(fam(n) in done for n in s))
print('modules with refused intrinsics:',len(mods),'freed at start',freed(done))
for f in order:
    blk=sum(1 for m,s in mods.items() if any(fam(n)==f for n in s))
    done.add(f); print(f"{f:22} names {len(names[f]):4} in-modules {blk:3} freed-cumulative {freed(done):3}")
for m,s in sorted(mods.items()): print(m, sorted(collections.Counter(fam(n) for n in s).items()))
