// SPDX-License-Identifier: Apache-2.0
// libstdc++'s configuration (newlib) over P.I.P.S.'s C library: see ../README.md
#include_next <bits/c++config.h>
// libstdc++'s .a is built for newlib and never linked: templates it keeps there (extern template: std::string's
// out-of-line members...) are instantiated in our own code instead
#undef _GLIBCXX_EXTERN_TEMPLATE
#define _GLIBCXX_EXTERN_TEMPLATE 0
