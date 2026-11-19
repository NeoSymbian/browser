/* SPDX-License-Identifier: Apache-2.0 */
/* P.I.P.S.'s <inttypes.h> has imaxdiv_t but no imaxabs/imaxdiv (C99), which <cinttypes> imports (README.md) */
#include_next <inttypes.h>
#if defined(__cplusplus) && !defined(NEOSYMBIAN_IMAX)
#define NEOSYMBIAN_IMAX
inline intmax_t imaxabs(intmax_t j) { return j < 0 ? -j : j; }
inline imaxdiv_t imaxdiv(intmax_t n, intmax_t d) {
	imaxdiv_t r = {n / d, n % d};
	return r;
}
#endif
