#!/usr/bin/env python3
"""WP-306: which test functions llc lowers with a branch.

    python3 branches.py FILE.s...

Prints, for every `t_*` function of llc's assembly that has a basic-block
label (`.LBB`) in its body, the function's name. The probe programs of
`gen.py --probe` have no branch of their own, so a label is llc's.
"""

import re
import sys


def main():
    for path in sys.argv[1:]:
        current, labels = None, set()
        with open(path, encoding="utf-8") as f:
            for line in f:
                m = re.match(r"^(t_[A-Za-z0-9_]+):", line)
                if m:
                    current = m.group(1)
                    continue
                if line.startswith(".Lfunc_end"):
                    current = None
                    continue
                if current and re.match(r"^\.LBB\d+_\d+:", line):
                    labels.add(current)
        print(path, " ".join(sorted(labels)) or "-")


if __name__ == "__main__":
    main()
