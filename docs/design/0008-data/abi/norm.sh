#!/bin/sh
# print a .s without directives other than labels and data, for reading and diffing
grep -v '^\s*\.\(cfi_\|p2align\|type\|size\|file\|globl\|text\|ident\|addrsig\|section\s*"\.note\)' "$1" | grep -v '^\.Lfunc_end\|^$'
