/* SPDX-License-Identifier: Apache-2.0 */
/* P.I.P.S.'s <ctype.h> predates C99's isblank, which <cctype> imports (README.md) */
#include_next <ctype.h>
#if defined(__cplusplus) && !defined(isblank) && !defined(NEOSYMBIAN_ISBLANK)
#define NEOSYMBIAN_ISBLANK
inline int isblank(int c) { return c == ' ' || c == '\t'; }
#endif
