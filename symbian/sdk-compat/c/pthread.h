/* SPDX-License-Identifier: Apache-2.0 */
/* NeoSymbian: P.I.P.S.'s pthread.h, plus the read-write locks it lacks. browser std/cxxrt implements them over a
 * mutex: readers exclude each other too (correct, less concurrent), as SpiderMonkey's RWLock does here (WB3). */
#include_next <pthread.h>
#ifndef NEOSYMBIAN_PTHREAD_RWLOCK
#define NEOSYMBIAN_PTHREAD_RWLOCK
typedef struct { pthread_mutex_t m; } pthread_rwlock_t;
typedef int pthread_rwlockattr_t;
#define PTHREAD_RWLOCK_INITIALIZER { PTHREAD_MUTEX_INITIALIZER }
#ifdef __cplusplus
extern "C" {
#endif
int pthread_rwlock_init(pthread_rwlock_t *, const pthread_rwlockattr_t *);
int pthread_rwlock_destroy(pthread_rwlock_t *);
int pthread_rwlock_rdlock(pthread_rwlock_t *);
int pthread_rwlock_wrlock(pthread_rwlock_t *);
int pthread_rwlock_tryrdlock(pthread_rwlock_t *);
int pthread_rwlock_trywrlock(pthread_rwlock_t *);
int pthread_rwlock_unlock(pthread_rwlock_t *);
#ifdef __cplusplus
}
#endif
#endif
