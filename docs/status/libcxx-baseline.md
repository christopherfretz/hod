# libc++ test suite: the baseline without hod

The long-term acceptance gate is libc++'s test suite built by kiln with hod
as the code generator (`docs/VALIDATION.md`, L8). This is the reference
point: the same suite with kiln and LLVM's `llc`, in the environment hod is
developed in. A hod run is compared with this table, not with 100%.

| | |
|---|---|
| date | 2026-11-11 |
| kiln | 1.0.1 at `bd60e1829`, release build |
| suite | `llvmorg-22.1.8`, `libcxx/test/std` and `libcxx/test/libcxx` |
| libc++ | 22.1.8 (static `libc++`, `libc++abi`, `libc++experimental`) |
| target | `aarch64-unknown-linux-gnu` |
| command | kiln's `cargo xtask libcxx --tag llvmorg-22.1.8 std libcxx --timeout 20 -j 3 --mem-limit 3072` |
| wall time | 3176 s at `-j 3` on a shared host |

| directory | total | pass | fail | xfail | xpass | unsupported | timeout | pass rate |
|---|---:|---:|---:|---:|---:|---:|---:|---:|
| `std` | 8424 | 8090 | 4 | 36 | 1 | 292 | 1 | 99.5% |
| `libcxx` | 823 | 519 | 0 | 11 | 0 | 293 | 0 | 97.9% |
| **total** | 9247 | 8609 | 4 | 47 | 1 | 585 | 1 | 99.4% |

Pass rate is pass / (total − unsupported).

Not passing, all independent of the code generator:

- 4 failures in `std/input.output/filesystems` (`directory_iterator` and
  `recursive_directory_iterator` constructors, `remove`, `remove_all`):
  assertions about permission errors, which do not occur for the root user
  the container runs as.
- 1 timeout: `std/input.output/file.streams/fstreams/ifstream.members/offset_range.pass.cpp`
  (a multi-gigabyte file under the 20 s limit).
- 1 unexpected pass: `std/utilities/variant/variant.hash/hash.depr.verify.cpp`.

kiln's own published figure for this suite is 99.4% as well
(`docs/status/libcxx-1.0.md` in kiln); the counts differ by which tests the
environment's features leave unsupported.
