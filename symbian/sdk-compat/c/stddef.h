/* SPDX-License-Identifier: Apache-2.0 */
/* P.I.P.S.'s <stddef.h> predates max_align_t (C11/C++11): see README.md */
#include_next <stddef.h>
#if !defined(_GCC_MAX_ALIGN_T) && ((defined(__STDC_VERSION__) && __STDC_VERSION__ >= 201112L) || (defined(__cplusplus) && __cplusplus >= 201103L))
#define _GCC_MAX_ALIGN_T
typedef struct {
	long long __max_align_ll __attribute__((__aligned__(__alignof__(long long))));
	long double __max_align_ld __attribute__((__aligned__(__alignof__(long double))));
} max_align_t;
#endif
