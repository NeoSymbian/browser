// SPDX-License-Identifier: Apache-2.0
// libstdc++'s ctype masks for newlib name newlib's <ctype.h> bits; P.I.P.S. has other names: the same values here
#ifndef _U
#define _U 01
#define _L 02
#define _N 04
#define _S 010
#define _P 020
#define _C 040
#define _X 0100
#define _B 0200
#define NEOSYMBIAN_CTYPE_MASKS
#endif
#include_next <bits/ctype_base.h>
#ifdef NEOSYMBIAN_CTYPE_MASKS
#undef _U
#undef _L
#undef _N
#undef _S
#undef _P
#undef _C
#undef _X
#undef _B
#undef NEOSYMBIAN_CTYPE_MASKS
#endif
