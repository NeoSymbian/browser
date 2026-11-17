// SPDX-License-Identifier: Apache-2.0
// PIPSPROB.EXE's C side (docs browser-std.md, browser.md WB2b): P.I.P.S. (Open C) as Rust's std and libc ports will
// see it. layout.inc (gen_layout.py) gives the sizes, offsets and constants; the checks below call each function std
// needs and report what happened. The Rust side (src/lib.rs) prints both over the debug port.
#include <stddef.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <errno.h>
#include <limits.h>
#include <fcntl.h>
#include <unistd.h>
#include <dirent.h>
#include <time.h>
#include <pthread.h>
#include <sched.h>
#include <poll.h>
#include <netdb.h>
#include <sys/types.h>
#include <sys/select.h>
#include <sys/syslimits.h>
#include <sys/stat.h>
#include <sys/time.h>
#include <sys/uio.h>
#include <sys/mman.h>
#include <sys/ioctl.h>
#include <sys/socket.h>
#include <netinet/in.h>
#include <netinet/tcp.h>
#include <arpa/inet.h>
#include <e32base.h>

#define ABSENT (-0x7fffffffL)

struct Row {
    const char* name;
    long value;
};
static const Row kLayout[] = {
#include "layout.inc"
};

// ---- the checks: 0 ok, 1 failed, 2 not supported (recorded, not fatal); `d` gets a one-line detail ----

#define OK(...) do { snprintf(d, n, __VA_ARGS__); return 0; } while (0)
#define FAIL(...) do { snprintf(d, n, __VA_ARGS__); return 1; } while (0)
#define NOTSUP(...) do { snprintf(d, n, __VA_ARGS__); return 2; } while (0)

static void* add_one(void* p) { return (void*)((long)p + 1); }

static int c_thread_join(char* d, int n) {
    pthread_t t;
    void* r = 0;
    int e = pthread_create(&t, 0, add_one, (void*)41);
    if (e) FAIL("pthread_create %d", e);
    e = pthread_join(t, &r);
    if (e || (long)r != 42) FAIL("join %d result %ld", e, (long)r);
    OK("joined, result 42");
}

static void* deep(void*) {
    // touch 7 MiB of an 8 MiB stack, a page at a time
    volatile char big[7 << 20];
    for (unsigned i = 0; i < sizeof(big); i += 4096) big[i] = (char)i;
    return (void*)(long)big[4096];
}

static int c_thread_stack_8m(char* d, int n) {
    pthread_attr_t a;
    pthread_attr_init(&a);
    int e = pthread_attr_setstacksize(&a, 8 << 20);
    if (e) FAIL("setstacksize(8 MiB) %d", e);
    pthread_t t;
    e = pthread_create(&t, &a, deep, 0);
    pthread_attr_destroy(&a);
    if (e) FAIL("pthread_create %d", e);
    pthread_join(t, 0);
    OK("8 MiB stack used to 7 MiB");
}

static volatile int g_detached;
static void* mark(void*) { g_detached = 1; return 0; }

static int c_thread_detach_self(char* d, int n) {
    pthread_t t;
    if (int e = pthread_create(&t, 0, mark, 0)) FAIL("create %d", e);
    if (int e = pthread_detach(t)) FAIL("detach %d", e);
    for (int i = 0; i < 100 && !g_detached; ++i) usleep(10000);
    if (!g_detached) FAIL("detached thread never ran");
    OK("detach ok, self %lu", (unsigned long)pthread_self());
}

static pthread_key_t g_key;
static void* key_worker(void*) {
    pthread_setspecific(g_key, (void*)7);
    return pthread_getspecific(g_key);
}

static int c_tls_key(char* d, int n) {
    if (int e = pthread_key_create(&g_key, 0)) FAIL("key_create %d", e);
    pthread_setspecific(g_key, (void*)3);
    pthread_t t;
    void* r = 0;
    pthread_create(&t, 0, key_worker, 0);
    pthread_join(t, &r);
    long mine = (long)pthread_getspecific(g_key);
    pthread_key_delete(g_key);
    if ((long)r != 7 || mine != 3) FAIL("thread saw %ld, main %ld", (long)r, mine);
    OK("per-thread values 3 and 7");
}

static int c_mutex(char* d, int n) {
    pthread_mutex_t m;
    pthread_mutexattr_t a;
    pthread_mutexattr_init(&a);
    int e = pthread_mutexattr_settype(&a, PTHREAD_MUTEX_RECURSIVE);
    if (e) FAIL("settype(RECURSIVE) %d", e);
    pthread_mutex_init(&m, &a);
    e = pthread_mutex_lock(&m) | pthread_mutex_lock(&m) | pthread_mutex_trylock(&m);
    pthread_mutex_unlock(&m);
    pthread_mutex_unlock(&m);
    pthread_mutex_unlock(&m);
    pthread_mutex_destroy(&m);
    pthread_mutexattr_destroy(&a);
    if (e) FAIL("recursive lock %d", e);
    pthread_mutex_t s = PTHREAD_MUTEX_INITIALIZER;
    if ((e = pthread_mutex_lock(&s)) || (e = pthread_mutex_unlock(&s))) FAIL("static initializer %d", e);
    OK("recursive x3, static initializer");
}

static pthread_mutex_t g_m = PTHREAD_MUTEX_INITIALIZER;
static pthread_cond_t g_c = PTHREAD_COND_INITIALIZER;
static int g_ready;
static void* signaller(void*) {
    usleep(20000);
    pthread_mutex_lock(&g_m);
    g_ready = 1;
    pthread_cond_signal(&g_c);
    pthread_mutex_unlock(&g_m);
    return 0;
}

static int c_condvar(char* d, int n) {
    pthread_t t;
    pthread_create(&t, 0, signaller, 0);
    pthread_mutex_lock(&g_m);
    while (!g_ready) pthread_cond_wait(&g_c, &g_m);
    pthread_mutex_unlock(&g_m);
    pthread_join(t, 0);
    OK("signalled");
}

static long ms_between(const timespec& a, const timespec& b) {
    return (b.tv_sec - a.tv_sec) * 1000 + (b.tv_nsec - a.tv_nsec) / 1000000;
}

static int c_condvar_timedwait(char* d, int n) {
    pthread_mutex_t m = PTHREAD_MUTEX_INITIALIZER;
    pthread_cond_t c = PTHREAD_COND_INITIALIZER;
    timespec t0, dl, t1;
    clock_gettime(CLOCK_REALTIME, &t0);
    dl = t0;
    dl.tv_nsec += 100000000;
    if (dl.tv_nsec >= 1000000000) { dl.tv_sec++; dl.tv_nsec -= 1000000000; }
    pthread_mutex_lock(&m);
    int e = pthread_cond_timedwait(&c, &m, &dl);
    pthread_mutex_unlock(&m);
    clock_gettime(CLOCK_REALTIME, &t1);
    long ms = ms_between(t0, t1);
    if (e != ETIMEDOUT || ms < 80 || ms > 1000) FAIL("returned %d after %ld ms (want ETIMEDOUT after ~100)", e, ms);
    OK("ETIMEDOUT after %ld ms", ms);
}

// Not declared by P.I.P.S.'s headers (found at compile time): each a gap the Rust ports work around
static int c_condattr_setclock(char* d, int n) { NOTSUP("pthread_condattr_setclock not in P.I.P.S.: std's condvar uses CLOCK_REALTIME"); }
static int c_pread(char* d, int n) { NOTSUP("pread/pwrite not in P.I.P.S.: FileExt::read_at via lseek+read"); }
static int c_posix_memalign(char* d, int n) { NOTSUP("posix_memalign not in P.I.P.S.: std's allocator aligns over malloc"); }
static int c_poll(char* d, int n) { NOTSUP("poll() not in P.I.P.S. (struct pollfd only); select() is: poll over select"); }

/// Wait up to `ms` for `fd` to be readable, with select() (P.I.P.S. has no poll()).
static int readable(int fd, int ms) {
    fd_set r;
    FD_ZERO(&r);
    FD_SET(fd, &r);
    timeval tv = {ms / 1000, (ms % 1000) * 1000};
    int k = select(fd + 1, &r, 0, 0, &tv);
    return k > 0 && FD_ISSET(fd, &r);
}

static int c_clocks(char* d, int n) {
    timespec r0, m0, m1;
    if (clock_gettime(CLOCK_REALTIME, &r0)) FAIL("REALTIME errno %d", errno);
#ifdef CLOCK_MONOTONIC
    if (clock_gettime(CLOCK_MONOTONIC, &m0)) FAIL("MONOTONIC errno %d", errno);
    timespec ns = {0, 50000000};
    nanosleep(&ns, 0);
    clock_gettime(CLOCK_MONOTONIC, &m1);
    long ms = ms_between(m0, m1);
    if (ms < 40 || ms > 500) FAIL("50 ms nanosleep took %ld ms on MONOTONIC", ms);
    OK("realtime %ld s, monotonic +%ld ms over a 50 ms nanosleep", (long)r0.tv_sec, ms);
#else
    NOTSUP("no CLOCK_MONOTONIC (realtime %ld)", (long)r0.tv_sec);
#endif
}

static int c_sched_yield(char* d, int n) {
    if (sched_yield()) FAIL("errno %d", errno);
    OK("ok");
}

static const char* kFile = "c:\\pipsprobe.tmp";
static const char* kFile2 = "c:\\pipsprobe2.tmp";
static const char* kDir = "c:\\pipsdir";

static int c_file_rw(char* d, int n) {
    int fd = open(kFile, O_CREAT | O_TRUNC | O_RDWR, 0644);
    if (fd < 0) FAIL("open errno %d", errno);
    const char msg[] = "hello pips";
    if (write(fd, msg, sizeof msg) != (ssize_t)sizeof msg) FAIL("write errno %d", errno);
    char buf[16] = {0};
    if (lseek(fd, 6, SEEK_SET) != 6 || read(fd, buf, 5) != 5 || memcmp(buf, "pips", 4)) FAIL("lseek+read got '%s' errno %d", buf, errno);
    struct stat st;
    if (fstat(fd, &st) || st.st_size != (off_t)sizeof msg) FAIL("fstat size %ld errno %d", (long)st.st_size, errno);
    if (ftruncate(fd, 4) || fsync(fd)) FAIL("ftruncate/fsync errno %d", errno);
    close(fd);
    if (stat(kFile, &st) || st.st_size != 4 || !S_ISREG(st.st_mode)) FAIL("stat size %ld mode %o", (long)st.st_size, st.st_mode);
    if (rename(kFile, kFile2)) FAIL("rename errno %d", errno);
    if (unlink(kFile2)) FAIL("unlink errno %d", errno);
    if (open(kFile2, O_RDONLY) >= 0 || errno != ENOENT) FAIL("reopen: errno %d (want ENOENT %d)", errno, ENOENT);
    OK("open/write/lseek/read/fstat/ftruncate/fsync/stat/rename/unlink, ENOENT");
}

static int c_dir_ops(char* d, int n) {
    if (mkdir(kDir, 0755)) FAIL("mkdir errno %d", errno);
    int fd = open("c:\\pipsdir\\a.txt", O_CREAT | O_WRONLY, 0644);
    if (fd >= 0) close(fd);
    DIR* dir = opendir(kDir);
    if (!dir) FAIL("opendir errno %d", errno);
    int entries = 0, saw_a = 0;
    while (struct dirent* e = readdir(dir)) {
        ++entries;
        saw_a |= !strcmp(e->d_name, "a.txt");
    }
    closedir(dir);
    unlink("c:\\pipsdir\\a.txt");
    if (rmdir(kDir)) FAIL("rmdir errno %d", errno);
    if (!saw_a) FAIL("readdir: %d entries, no a.txt", entries);
    OK("mkdir/opendir/readdir (%d entries)/rmdir", entries);
}

static int c_cwd(char* d, int n) {
    char cwd[PATH_MAX], real[PATH_MAX];
    if (!getcwd(cwd, sizeof cwd)) FAIL("getcwd errno %d", errno);
    if (!realpath(".", real)) NOTSUP("cwd '%s', realpath errno %d", cwd, errno);
    OK("cwd '%s', realpath(.) '%s'", cwd, real);
}

static int c_env(char* d, int n) {
    if (setenv("PIPSPROBE", "yes", 1)) FAIL("setenv errno %d", errno);
    const char* v = getenv("PIPSPROBE");
    if (!v || strcmp(v, "yes")) FAIL("getenv '%s'", v ? v : "(null)");
    unsetenv("PIPSPROBE");
    if (getenv("PIPSPROBE")) FAIL("unsetenv left it");
    OK("set/get/unset");
}

static int c_memory(char* d, int n) {
    void* p = malloc(1000);
    p = realloc(p, 100000);
    if (!p) FAIL("realloc");
    free(p);
    long page = sysconf(_SC_PAGESIZE);
    // no MAP_ANON in P.I.P.S.: map a file instead, as far as mmap goes
    int fd = open("c:\\pipsmap.tmp", O_CREAT | O_TRUNC | O_RDWR, 0644);
    ftruncate(fd, 1 << 16);
    void* m = mmap(0, 1 << 16, PROT_READ | PROT_WRITE, MAP_PRIVATE, fd, 0);
    int merr = errno, pr = -2;
    if (m != MAP_FAILED) {
        ((char*)m)[1234] = 7;
        pr = mprotect(m, page, PROT_NONE);
        munmap(m, 1 << 16);
    }
    close(fd);
    unlink("c:\\pipsmap.tmp");
    if (m == MAP_FAILED) NOTSUP("page %ld, malloc/realloc ok; mmap(file) errno %d; no MAP_ANON", page, merr);
    OK("page %ld, malloc/realloc ok, mmap(file) ok, mprotect -> %d (no effect: browser.md); no MAP_ANON", page, pr);
}

static int c_pipe_select(char* d, int n) {
    int p[2];
    if (pipe(p)) NOTSUP("pipe errno %d", errno);
    int r0 = readable(p[0], 0);
    write(p[1], "x", 1);
    int r1 = readable(p[0], 1000);
    char c;
    read(p[0], &c, 1);
    close(p[0]);
    close(p[1]);
    if (r0 || !r1) FAIL("select on a pipe: empty %d, ready %d errno %d", r0, r1, errno);
    OK("pipe + select (empty 0, ready 1)");
}

static int c_errno_values(char* d, int n) {
    errno = 0;
    if (open("c:\\no\\such\\file", O_RDONLY) >= 0) FAIL("opened a missing file");
    OK("missing file -> errno %d (ENOENT %d), ENOTDIR %d", errno, ENOENT, ENOTDIR);
}

static int c_getpid(char* d, int n) {
    OK("pid %d", (int)getpid());
}

// sockets last: if the IP stack is missing, a call may block, and the line printed before it shows where
static int c_tcp_loopback(char* d, int n) {
    int s = socket(AF_INET, SOCK_STREAM, 0);
    if (s < 0) NOTSUP("socket errno %d", errno);
    int one = 1;
    setsockopt(s, SOL_SOCKET, SO_REUSEADDR, &one, sizeof one);
    sockaddr_in a;
    memset(&a, 0, sizeof a);
    a.sin_family = AF_INET;
    a.sin_addr.s_addr = htonl(INADDR_LOOPBACK);
    if (bind(s, (sockaddr*)&a, sizeof a)) FAIL("bind errno %d", errno);
    if (listen(s, 2)) FAIL("listen errno %d", errno);
    socklen_t len = sizeof a;
    getsockname(s, (sockaddr*)&a, &len);
    int c = socket(AF_INET, SOCK_STREAM, 0);
    if (connect(c, (sockaddr*)&a, sizeof a)) FAIL("connect to port %d errno %d", ntohs(a.sin_port), errno);
    int x = accept(s, 0, 0);
    if (x < 0) FAIL("accept errno %d", errno);
    setsockopt(c, IPPROTO_TCP, TCP_NODELAY, &one, sizeof one);
    send(c, "ping", 4, 0);
    int pr = readable(x, 2000);
    char buf[8] = {0};
    int got = recv(x, buf, sizeof buf, 0);
    // non-blocking: an empty socket says EAGAIN
    fcntl(x, F_SETFL, fcntl(x, F_GETFL) | O_NONBLOCK);
    int empty = recv(x, buf + 4, 4, 0);
    int eagain = errno;
    shutdown(c, SHUT_RDWR);
    close(c);
    close(x);
    close(s);
    if (got != 4 || memcmp(buf, "ping", 4)) FAIL("recv %d '%s' errno %d (poll %d)", got, buf, errno, pr);
    if (pr != 1) FAIL("select on the socket returned %d", pr);
    if (empty != -1 || (eagain != EAGAIN && eagain != EWOULDBLOCK)) NOTSUP("non-blocking recv %d errno %d", empty, eagain);
    OK("port %d: connect/accept/send/select/recv, O_NONBLOCK -> EAGAIN", ntohs(a.sin_port));
}

static int c_udp_loopback(char* d, int n) {
    int s = socket(AF_INET, SOCK_DGRAM, 0);
    if (s < 0) NOTSUP("socket errno %d", errno);
    sockaddr_in a;
    memset(&a, 0, sizeof a);
    a.sin_family = AF_INET;
    a.sin_addr.s_addr = htonl(INADDR_LOOPBACK);
    bind(s, (sockaddr*)&a, sizeof a);
    socklen_t len = sizeof a;
    getsockname(s, (sockaddr*)&a, &len);
    int c = socket(AF_INET, SOCK_DGRAM, 0);
    sendto(c, "dgram", 5, 0, (sockaddr*)&a, sizeof a);
    char buf[8] = {0};
    int got = recvfrom(s, buf, sizeof buf, 0, 0, 0);
    close(c);
    close(s);
    if (got != 5) FAIL("recvfrom %d errno %d", got, errno);
    OK("sendto/recvfrom on port %d", ntohs(a.sin_port));
}

static int c_getaddrinfo_local(char* d, int n) {
    addrinfo hints, *res = 0;
    memset(&hints, 0, sizeof hints);
    hints.ai_family = AF_INET;
    hints.ai_socktype = SOCK_STREAM;
    int e = getaddrinfo("127.0.0.1", "80", &hints, &res);
    if (e || !res) NOTSUP("getaddrinfo(127.0.0.1) %d", e);
    int port = ntohs(((sockaddr_in*)res->ai_addr)->sin_port);
    freeaddrinfo(res);
    OK("numeric host resolved, port %d", port);
}

struct Check {
    const char* name;
    int (*fn)(char*, int);
};
static const Check kChecks[] = {
    {"thread_join", c_thread_join},
    {"thread_stack_8m", c_thread_stack_8m},
    {"thread_detach_self", c_thread_detach_self},
    {"tls_key", c_tls_key},
    {"mutex", c_mutex},
    {"condvar", c_condvar},
    {"condvar_timedwait", c_condvar_timedwait},
    {"condattr_setclock", c_condattr_setclock},
    {"clocks", c_clocks},
    {"sched_yield", c_sched_yield},
    {"file_rw", c_file_rw},
    {"dir_ops", c_dir_ops},
    {"cwd", c_cwd},
    {"env", c_env},
    {"memory", c_memory},
    {"pipe_select", c_pipe_select},
    {"poll", c_poll},
    {"pread", c_pread},
    {"posix_memalign", c_posix_memalign},
    {"errno_values", c_errno_values},
    {"getpid", c_getpid},
    {"tcp_loopback", c_tcp_loopback},
    {"udp_loopback", c_udp_loopback},
    {"getaddrinfo_local", c_getaddrinfo_local},
};

extern "C" {
// P.I.P.S. code uses the cleanup stack (opendir panicked E32USER-CBase 69 without one): a C program's crt0 makes it,
// an e32::main! EXE doesn't, so the probe does (and so must std's start-up, WB2c)
int pips_init() { return CTrapCleanup::New() ? 0 : -4; }
int pips_layout_count() { return sizeof kLayout / sizeof kLayout[0]; }
const char* pips_layout_name(int i) { return kLayout[i].name; }
long pips_layout_value(int i) { return kLayout[i].value; }
int pips_check_count() { return sizeof kChecks / sizeof kChecks[0]; }
const char* pips_check_name(int i) { return kChecks[i].name; }
int pips_check_run(int i, char* detail, int len) { return kChecks[i].fn(detail, len); }
}
