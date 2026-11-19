/* SPDX-License-Identifier: Apache-2.0 */
/* P.I.P.S. renames vfscanf to __vfscanf by an object-like macro, which <cstdio> #undefs: C++ gets a real vfscanf
   (README.md) */
#include_next <stdio.h>
#if defined(__cplusplus) && defined(vfscanf) && !defined(NEOSYMBIAN_VFSCANF)
#define NEOSYMBIAN_VFSCANF
#undef vfscanf
inline int vfscanf(FILE* __restrict f, const char* __restrict fmt, va_list ap) { return __vfscanf(f, fmt, ap); }
#endif
