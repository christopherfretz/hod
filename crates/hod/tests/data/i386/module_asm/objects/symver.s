# `.symver` (the x86-64 case "symver", the module's functions made
# variables): an alias of a defined name, a rename with `@@@` or of an
# undefined name, nothing for an unreferenced undefined one.
.symver foo, foo@@V1
.symver foo, foo@V0
.symver bar, bar@V1
.symver wk, wk@V3
.symver hid, hid@V4
.symver unused, unused@V5
.text
.globl tf
.type tf,@function
tf: movl $7, %eax
ret
.size tf, .-tf
.symver tf, tf@@V2
.symver loc, loc@V6
.symver ext2, ext2@@@V7
.symver ext3, ext3@V8
call ext3
call bar
jmp ext2
.weak tw
tw:
ret
.symver tw, tw@@@V9
