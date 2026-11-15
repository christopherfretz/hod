#!/bin/sh
# for each probe: how many normalized lines differ 19 vs 22 and dag vs fast
for f in "$@"; do b=${f%.ll}
 printf '%s dag19/22 %s fast19/22 %s dag/fast22 %s\n' $b $(diff <(sh norm.sh $b.dag19.s) <(sh norm.sh $b.dag22.s)|grep -c '^[<>]') $(diff <(sh norm.sh $b.fast19.s) <(sh norm.sh $b.fast22.s)|grep -c '^[<>]') $(diff <(sh norm.sh $b.dag22.s) <(sh norm.sh $b.fast22.s)|grep -c '^[<>]')
done
