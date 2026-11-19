# sdk-compat

Headers for the `armv7-symbian-*` compilers (`tools/symbian-sdk.sh`): `include/` is searched before libstdc++'s
headers, `c/` between libstdc++'s and P.I.P.S.'s (where `<cmath>`'s `#include_next <math.h>` lands).
They bridge the Arm GNU toolchain's libstdc++ (configured for newlib) and the 2010 P.I.P.S. C headers, without
editing either. Each one `#include_next`s the real header and adds or undoes only what differs.

| Header | Why |
|---|---|
| `include/bits/c++config.h` | `_GLIBCXX_EXTERN_TEMPLATE` 0: libstdc++'s `.a` (built for newlib) is never linked, so the templates it would hold are instantiated in our code |
| `include/bits/ctype_base.h` | libstdc++'s newlib ctype masks are spelt `_U`, `_L`...: P.I.P.S.'s `<ctype.h>` has no such names |
| `c/stddef.h` | `max_align_t` (C11/C++11), which P.I.P.S. predates |
| `c/stdio.h` | P.I.P.S. renames `vfscanf` to `__vfscanf` by macro, which `<cstdio>` `#undef`s |
| `c/math.h` | P.I.P.S. has the C89/C99 math functions and their `float` forms but few `long double` ones (some only as macros onto the `double` ones) and no `log2`, `tgamma`, `nan`, `nexttoward`, all of which libstdc++ imports into `std::`; on ARM EABI `long double` is `double`, so C++ gets inline forms over the `double` functions |
| `c/ctype.h` | C99's `isblank`, which P.I.P.S. predates |
| `c/stdint.h` | P.I.P.S. gives C++ the limit and constant macros (`INT64_MAX`, `SIZE_MAX`, `UINT64_C`) only on request (`__STDC_LIMIT_MACROS`), the C++98 rule |
| `c/inttypes.h` | C99's `imaxabs`/`imaxdiv`, which P.I.P.S. predates |
