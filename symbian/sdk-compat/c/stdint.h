/* SPDX-License-Identifier: Apache-2.0 */
/* P.I.P.S.'s <stdint.h> gives C++ its limit and constant macros (INT64_MAX, SIZE_MAX, UINT64_C...) only when asked
   for, the C++98 rule; C++11 and later always have them (README.md) */
#ifndef __STDC_LIMIT_MACROS
#define __STDC_LIMIT_MACROS
#endif
#ifndef __STDC_CONSTANT_MACROS
#define __STDC_CONSTANT_MACROS
#endif
#include_next <stdint.h>
