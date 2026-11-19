/* SPDX-License-Identifier: Apache-2.0 */
/* NeoSymbian: P.I.P.S.'s signal.h, plus C's sig_atomic_t (its machine/signal.h lost the typedef) and the calls libc
 * exports but declares only under SYMBIAN_OE_POSIX_SIGNALS (aws-lc's console.c; libstdc++'s <csignal>). */
#include_next <signal.h>
#ifndef NEOSYMBIAN_SIGNAL_EXTRA
#define NEOSYMBIAN_SIGNAL_EXTRA
typedef int sig_atomic_t;
#ifndef SYMBIAN_OE_POSIX_SIGNALS
#ifdef __cplusplus
extern "C" {
#endif
void (*signal(int, void (*)(int)))(int);
int raise(int sig);
int kill(pid_t pid, int sig);
int sigprocmask(int how, const sigset_t *set, sigset_t *oset);
int sigfillset(sigset_t *set);
int sigaddset(sigset_t *set, int signo);
int sigdelset(sigset_t *set, int signo);
int sigismember(const sigset_t *set, int signo);
#ifdef __cplusplus
}
#endif
#endif
#endif
