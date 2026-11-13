; Loops with phis, a nested loop, a dense and a sparse switch, an
; indirectbr through a table of block addresses, and memory: a local
; array written and read through getelementptr, narrow loads and stores
; with their extensions, a global table of pointers. The program makes
; no call; the exit status says which check failed (100 + its number),
; or that all passed (7). The expected values are worked out by hand in
; the comments.
;@ covers: spec/ir.md §5.4 control flow, memory (call-free; WP-043)
;@ tier: quick
;@ exit: 7

@n = global i32 10
@bytes = global [4 x i8] c"\01\80\FF\7F"
@words = global [3 x i16] [i16 -2, i16 300, i16 32767]
@cells = global [3 x i64] [i64 5, i64 7, i64 11]
@table = global [3 x ptr] [ptr getelementptr (i8, ptr @cells, i64 16), ptr @cells, ptr getelementptr (i8, ptr @cells, i64 8)]
@targets = internal constant [3 x ptr] [ptr blockaddress(@main, %ib0), ptr blockaddress(@main, %ib1), ptr blockaddress(@main, %ib2)]

define i32 @main() {
entry:
  %n = load volatile i32, ptr @n
  br label %sum

; 1. sum of 1..n = 55
sum:
  %i = phi i32 [ 1, %entry ], [ %i.next, %sum ]
  %acc = phi i32 [ 0, %entry ], [ %acc.next, %sum ]
  %acc.next = add i32 %acc, %i
  %i.next = add i32 %i, 1
  %more = icmp sle i32 %i.next, %n
  br i1 %more, label %sum, label %sum.done
sum.done:
  %c1 = icmp ne i32 %acc.next, 55
  br i1 %c1, label %fail1, label %nested

; 2. sum over i < n, j < i of (i xor j) = 297
nested:
  %oi = phi i32 [ 0, %sum.done ], [ %oi.next, %inner.done ]
  %oacc = phi i32 [ 0, %sum.done ], [ %iacc.out, %inner.done ]
  br label %inner
inner:
  %j = phi i32 [ 0, %nested ], [ %j.next, %inner.body ]
  %iacc = phi i32 [ %oacc, %nested ], [ %iacc.next, %inner.body ]
  %jlt = icmp slt i32 %j, %oi
  br i1 %jlt, label %inner.body, label %inner.done
inner.body:
  %x = xor i32 %oi, %j
  %iacc.next = add i32 %iacc, %x
  %j.next = add i32 %j, 1
  br label %inner
inner.done:
  %iacc.out = phi i32 [ %iacc, %inner ]
  %oi.next = add i32 %oi, 1
  %omore = icmp slt i32 %oi.next, %n
  br i1 %omore, label %nested, label %nested.done
nested.done:
  %c2 = icmp ne i32 %iacc.out, 297
  br i1 %c2, label %fail2, label %switch.head

; 3. for k in 0..n: dense switch adds 1, 10, 100, 1000 for k = 0..3,
; 5 otherwise: 1111 + 6 * 5 = 1141
switch.head:
  %k = phi i32 [ 0, %nested.done ], [ %k.next, %switch.join ]
  %sacc = phi i32 [ 0, %nested.done ], [ %sacc.next, %switch.join ]
  switch i32 %k, label %d.other [
    i32 0, label %d0
    i32 1, label %d1
    i32 2, label %d2
    i32 3, label %d3
  ]
d0:
  br label %switch.join
d1:
  br label %switch.join
d2:
  br label %switch.join
d3:
  br label %switch.join
d.other:
  br label %switch.join
switch.join:
  %add = phi i32 [ 1, %d0 ], [ 10, %d1 ], [ 100, %d2 ], [ 1000, %d3 ], [ 5, %d.other ]
  %sacc.next = add i32 %sacc, %add
  %k.next = add i32 %k, 1
  %kmore = icmp slt i32 %k.next, %n
  br i1 %kmore, label %switch.head, label %switch.done
switch.done:
  %c3 = icmp ne i32 %sacc.next, 1141
  br i1 %c3, label %fail3, label %sparse

; 4. a sparse switch on an i64: -1000000 goes to the second case
sparse:
  %big = sext i32 %n to i64
  %key = mul i64 %big, -100000
  switch i64 %key, label %fail4 [
    i64 7, label %fail4
    i64 -1000000, label %indirect
    i64 4294967296, label %fail4
  ]

; 5. indirectbr: block 2 of the table, which returns 1 for the check
indirect:
  %slot = getelementptr [3 x ptr], ptr @targets, i64 0, i64 2
  %dest = load ptr, ptr %slot
  indirectbr ptr %dest, [label %ib0, label %ib1, label %ib2]
ib0:
  br label %ib.join
ib1:
  br label %ib.join
ib2:
  br label %ib.join
ib.join:
  %which = phi i32 [ 0, %ib0 ], [ 0, %ib1 ], [ 1, %ib2 ]
  %c5 = icmp ne i32 %which, 1
  br i1 %c5, label %fail5, label %memory

; 6. a local array: a[i] = i * i for i < 8, then a[3] + a[7] = 58
memory:
  %arr = alloca [8 x i32], align 4
  br label %fill
fill:
  %fi = phi i64 [ 0, %memory ], [ %fi.next, %fill ]
  %fi32 = trunc i64 %fi to i32
  %sq = mul i32 %fi32, %fi32
  %elt = getelementptr inbounds [8 x i32], ptr %arr, i64 0, i64 %fi
  store i32 %sq, ptr %elt, align 4
  %fi.next = add i64 %fi, 1
  %fmore = icmp ult i64 %fi.next, 8
  br i1 %fmore, label %fill, label %filled
filled:
  %p3 = getelementptr inbounds i32, ptr %arr, i64 3
  %v3 = load i32, ptr %p3, align 4
  %p7 = getelementptr inbounds i32, ptr %arr, i64 7
  %v7 = load i32, ptr %p7, align 4
  %s37 = add i32 %v3, %v7
  %c6 = icmp ne i32 %s37, 58
  br i1 %c6, label %fail6, label %narrow

; 7. narrow loads: sext of bytes[1] (0x80) is -128, zext of bytes[2]
; (0xFF) is 255, sext of words[0] is -2, zext of words[1] is 300:
; -128 + 255 - 2 + 300 = 425
narrow:
  %b1p = getelementptr [4 x i8], ptr @bytes, i64 0, i64 1
  %b1 = load volatile i8, ptr %b1p
  %b1s = sext i8 %b1 to i32
  %b2p = getelementptr [4 x i8], ptr @bytes, i64 0, i64 2
  %b2 = load volatile i8, ptr %b2p
  %b2z = zext i8 %b2 to i32
  %w0 = load volatile i16, ptr @words
  %w0s = sext i16 %w0 to i32
  %w1p = getelementptr [3 x i16], ptr @words, i64 0, i64 1
  %w1 = load volatile i16, ptr %w1p
  %w1z = zext i16 %w1 to i32
  %n1 = add i32 %b1s, %b2z
  %n2 = add i32 %n1, %w0s
  %n3 = add i32 %n2, %w1z
  %c7 = icmp ne i32 %n3, 425
  br i1 %c7, label %fail7, label %narrow.store

; 8. narrow stores: store i16 0x1234 into words[2], then its bytes
; are 0x34, 0x12; byte 1 sign-extended plus the i16 is 18 + 4660 = 4678
narrow.store:
  %w2p = getelementptr [3 x i16], ptr @words, i64 0, i64 2
  store volatile i16 4660, ptr %w2p
  %hi.p = getelementptr i8, ptr %w2p, i64 1
  %hi = load volatile i8, ptr %hi.p
  %his = sext i8 %hi to i32
  %w2 = load volatile i16, ptr %w2p
  %w2s = sext i16 %w2 to i32
  %ns = add i32 %his, %w2s
  %c8 = icmp ne i32 %ns, 4678
  br i1 %c8, label %fail8, label %pointers

; 9. a table of pointers into @cells: *table[0] + *table[1] * *table[2]
; = 11 + 5 * 7 = 46; and the pointer difference table[0] - table[1] = 16
pointers:
  %t0 = load ptr, ptr @table
  %t1p = getelementptr [3 x ptr], ptr @table, i64 0, i64 1
  %t1 = load ptr, ptr %t1p
  %t2p = getelementptr [3 x ptr], ptr @table, i64 0, i64 2
  %t2 = load ptr, ptr %t2p
  %v0 = load i64, ptr %t0
  %v1 = load i64, ptr %t1
  %v2 = load i64, ptr %t2
  %prod = mul i64 %v1, %v2
  %tot = add i64 %v0, %prod
  %c9 = icmp ne i64 %tot, 46
  br i1 %c9, label %fail9, label %difference
difference:
  %i0 = ptrtoint ptr %t0 to i64
  %i1 = ptrtoint ptr %t1 to i64
  %diff = sub i64 %i0, %i1
  %c10 = icmp ne i64 %diff, 16
  br i1 %c10, label %fail10, label %done

done:
  ret i32 7
fail1:
  ret i32 101
fail2:
  ret i32 102
fail3:
  ret i32 103
fail4:
  ret i32 104
fail5:
  ret i32 105
fail6:
  ret i32 106
fail7:
  ret i32 107
fail8:
  ret i32 108
fail9:
  ret i32 109
fail10:
  ret i32 110
}
