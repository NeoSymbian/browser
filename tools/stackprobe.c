// SPDX-License-Identifier: Apache-2.0
// stackprobe: LD_PRELOAD interposer for pthread_create that measures each thread's peak stack use (WB0).
// It paints the unused part of a new thread's stack with a pattern, then at thread exit (and at process exit, for
// threads still alive) finds the lowest overwritten byte. Painting commits the stack pages, so never combine this
// with memory measurements. The main thread is not covered.
//   gcc -O2 -shared -fPIC -o stackprobe.so stackprobe.c -ldl -lpthread
//   STACKPROBE_OUT=/tmp/stacks.txt LD_PRELOAD=./stackprobe.so <program>
// Output lines: "stackprobe tid=<tid> name=<name> size=<bytes> used=<bytes> at=<exit|end>"
#define _GNU_SOURCE
#include <dlfcn.h>
#include <fcntl.h>
#include <pthread.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/syscall.h>
#include <unistd.h>

#define PATTERN 0xA5u
#define MAX_THREADS 4096
#define SP_MARGIN 512 // keep clear of the live frames below the current stack pointer

struct rec {
    uintptr_t lo, hi; // painted range [lo, hi); the stack top is top
    uintptr_t top;
    size_t size;
    pid_t tid;
    int done;
};

static struct rec recs[MAX_THREADS];
static int nrecs;
static int out_fd = 2;

typedef int (*create_fn)(pthread_t *, const pthread_attr_t *, void *(*)(void *), void *);

struct start {
    void *(*fn)(void *);
    void *arg;
};

static size_t used_bytes(const struct rec *r) {
    const unsigned char *p = (const unsigned char *)r->lo;
    while ((uintptr_t)p < r->hi && *p == PATTERN) p++;
    return r->top - (uintptr_t)p;
}

static void report(struct rec *r, const char *at) {
    if (__atomic_exchange_n(&r->done, 1, __ATOMIC_ACQ_REL)) return;
    char name[32] = "?", line[160];
    if (r->tid == (pid_t)syscall(SYS_gettid)) pthread_getname_np(pthread_self(), name, sizeof name);
    int n = snprintf(line, sizeof line, "stackprobe tid=%d name=%s size=%zu used=%zu at=%s\n", r->tid, name, r->size,
                     used_bytes(r), at);
    if (n > 0) (void)!write(out_fd, line, (size_t)n);
}

static void on_exit_thread(void *rec) { report(rec, "exit"); }

static void *trampoline(void *p) {
    struct start s = *(struct start *)p;
    free(p);
    struct rec *r = NULL;
    pthread_attr_t a;
    void *base;
    size_t size, guard = 0;
    if (pthread_getattr_np(pthread_self(), &a) == 0) {
        pthread_attr_getstack(&a, &base, &size);
        pthread_attr_getguardsize(&a, &guard);
        pthread_attr_destroy(&a);
        int i = __atomic_fetch_add(&nrecs, 1, __ATOMIC_ACQ_REL);
        if (i < MAX_THREADS) {
            volatile char here;
            r = &recs[i];
            r->top = (uintptr_t)base + size;
            r->size = size;
            r->tid = (pid_t)syscall(SYS_gettid);
            // glibc reports the usable stack (guard excluded) on recent versions; skip the guard size anyway.
            r->lo = (uintptr_t)base + guard;
            r->hi = ((uintptr_t)&here - SP_MARGIN) & ~(uintptr_t)15;
            memset((void *)r->lo, PATTERN, r->hi - r->lo);
        }
    }
    void *ret;
    pthread_cleanup_push(on_exit_thread, r);
    ret = s.fn(s.arg);
    pthread_cleanup_pop(r != NULL);
    return ret;
}

int pthread_create(pthread_t *t, const pthread_attr_t *attr, void *(*fn)(void *), void *arg) {
    static create_fn real;
    if (!real) real = (create_fn)dlsym(RTLD_NEXT, "pthread_create");
    struct start *s = malloc(sizeof *s);
    if (!s) return real(t, attr, fn, arg);
    s->fn = fn;
    s->arg = arg;
    int rc = real(t, attr, trampoline, s);
    if (rc) free(s);
    return rc;
}

__attribute__((constructor)) static void init(void) {
    const char *path = getenv("STACKPROBE_OUT");
    if (path) {
        int fd = open(path, O_WRONLY | O_CREAT | O_APPEND | O_CLOEXEC, 0644);
        if (fd >= 0) out_fd = fd;
    }
}

__attribute__((destructor)) static void fini(void) {
    int n = nrecs < MAX_THREADS ? nrecs : MAX_THREADS;
    for (int i = 0; i < n; i++) report(&recs[i], "end"); // live threads: an approximate, racy snapshot
}
