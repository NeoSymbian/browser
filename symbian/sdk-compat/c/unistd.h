/* SPDX-License-Identifier: Apache-2.0 */
/* NeoSymbian: P.I.P.S.'s unistd.h, plus what it lacks that portable code calls. browser std/cxxrt implements these. */
#include_next <unistd.h>
#ifndef NEOSYMBIAN_UNISTD_EXTRA
#define NEOSYMBIAN_UNISTD_EXTRA
#ifdef __cplusplus
extern "C" {
#endif
/* Symbian files have no owners: accepted and ignored (sqlite's unix VFS) */
int fchown(int fd, uid_t owner, gid_t group);
#ifdef __cplusplus
}
#endif
#endif
