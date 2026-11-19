/* SPDX-License-Identifier: Apache-2.0 */
/* P.I.P.S. declares the C89/C99 math functions and their float forms but few long double forms, and lacks log2,
   tgamma, nan and nexttoward; libstdc++ (built for newlib) imports them all into std::. C++
   gets them here: on ARM EABI long double is double, so the long double forms call the double functions; log2,
   tgamma and nan are written over what P.I.P.S. has (README.md). P.I.P.S. maps a few of these names to the double
   functions by macro: undone first. */
#include_next <math.h>
#ifndef NEOSYMBIAN_MATH
#define NEOSYMBIAN_MATH
#ifdef __cplusplus
#undef acosl
inline long double acosl(long double x) { return acos(x); }
#undef asinl
inline long double asinl(long double x) { return asin(x); }
#undef atanl
inline long double atanl(long double x) { return atan(x); }
#undef cbrtl
inline long double cbrtl(long double x) { return cbrt(x); }
#undef ceill
inline long double ceill(long double x) { return ceil(x); }
#undef cosl
inline long double cosl(long double x) { return cos(x); }
#undef coshl
inline long double coshl(long double x) { return cosh(x); }
#undef erfl
inline long double erfl(long double x) { return erf(x); }
#undef erfcl
inline long double erfcl(long double x) { return erfc(x); }
#undef expl
inline long double expl(long double x) { return exp(x); }
#undef exp2l
inline long double exp2l(long double x) { return exp2(x); }
#undef expm1l
inline long double expm1l(long double x) { return expm1(x); }
#undef floorl
inline long double floorl(long double x) { return floor(x); }
#undef lgammal
inline long double lgammal(long double x) { return lgamma(x); }
#undef log10l
inline long double log10l(long double x) { return log10(x); }
#undef log1pl
inline long double log1pl(long double x) { return log1p(x); }
#undef logbl
inline long double logbl(long double x) { return logb(x); }
#undef logl
inline long double logl(long double x) { return log(x); }
#undef nearbyintl
inline long double nearbyintl(long double x) { return nearbyint(x); }
#undef rintl
inline long double rintl(long double x) { return rint(x); }
#undef roundl
inline long double roundl(long double x) { return round(x); }
#undef sinl
inline long double sinl(long double x) { return sin(x); }
#undef sinhl
inline long double sinhl(long double x) { return sinh(x); }
#undef sqrtl
inline long double sqrtl(long double x) { return sqrt(x); }
#undef tanl
inline long double tanl(long double x) { return tan(x); }
#undef tanhl
inline long double tanhl(long double x) { return tanh(x); }
#undef acoshl
inline long double acoshl(long double x) { return acosh(x); }
#undef asinhl
inline long double asinhl(long double x) { return asinh(x); }
#undef atanhl
inline long double atanhl(long double x) { return atanh(x); }
#undef atan2l
inline long double atan2l(long double x, long double y) { return atan2(x, y); }
#undef fmodl
inline long double fmodl(long double x, long double y) { return fmod(x, y); }
#undef powl
inline long double powl(long double x, long double y) { return pow(x, y); }
#undef hypotl
inline long double hypotl(long double x, long double y) { return hypot(x, y); }
#undef remainderl
inline long double remainderl(long double x, long double y) { return remainder(x, y); }
#undef nextafterl
inline long double nextafterl(long double x, long double y) { return nextafter(x, y); }
#undef fminl
inline long double fminl(long double x, long double y) { return fmin(x, y); }
#undef fmal
inline long double fmal(long double x, long double y, long double z) { return fma(x, y, z); }
#undef frexpl
inline long double frexpl(long double x, int* e) { return frexp(x, e); }
#undef ldexpl
inline long double ldexpl(long double x, int e) { return ldexp(x, e); }
#undef ldexpf
inline float ldexpf(float x, int e) { return ldexp(x, e); }
#undef scalbnl
inline long double scalbnl(long double x, int e) { return scalbn(x, e); }
#undef modfl
inline long double modfl(long double x, long double* i) { double d; long double r = modf(x, &d); *i = d; return r; }
#undef remquol
inline long double remquol(long double x, long double y, int* q) { return remquo(x, y, q); }
#undef lrintl
inline long lrintl(long double x) { return lrint(x); }
#undef llrintl
inline long long llrintl(long double x) { return llrint(x); }
#undef lroundl
inline long lroundl(long double x) { return lround(x); }
#undef llroundl
inline long long llroundl(long double x) { return llround(x); }
// log2: exact for powers of two (frexp), log(m)/ln 2 between them; 0, negatives, infinities and NaN as log does
#undef log2
inline double log2(double x) { int e; if (!(x > 0) || x == __builtin_inf()) return log(x); double m = frexp(x, &e); return m == 0.5 ? e - 1 : e + log(m) * 1.4426950408889634; }
#undef log2f
inline float log2f(float x) { return float(log2(double(x))); }
#undef log2l
inline long double log2l(long double x) { return log2(double(x)); }
// tgamma: the sign and the log of |gamma| from lgamma_r
#undef tgamma
inline double tgamma(double x) { if (x == 0) return 1 / x; int s; double l = lgamma_r(x, &s); return s * exp(l); }
#undef tgammaf
inline float tgammaf(float x) { return float(tgamma(double(x))); }
#undef tgammal
inline long double tgammal(long double x) { return tgamma(double(x)); }
#undef nan
inline double nan(const char* s) { return __builtin_nan(s); }
#undef nanf
inline float nanf(const char* s) { return __builtin_nanf(s); }
#undef nanl
inline long double nanl(const char* s) { return __builtin_nanl(s); }
#undef nexttoward
inline double nexttoward(double x, long double y) { return nextafter(x, double(y)); }
#undef nexttowardl
inline long double nexttowardl(long double x, long double y) { return nextafter(double(x), double(y)); }
#endif  // __cplusplus
#endif  // NEOSYMBIAN_MATH
