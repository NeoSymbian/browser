// SPDX-License-Identifier: Apache-2.0
// The C++ runtime for ARMv7 C++ on NeoSymbian (cxxrt/src/lib.rs): __cxa_guard_* and __emutls_get_address over
// P.I.P.S.'s pthreads (GCC for arm-none-eabi emits calls to both: function-local statics are thread-safe by default,
// and thread_local uses emulated TLS), and libstdc++'s std::__throw_* helpers as fatal errors.
#include <chrono>
#include <errno.h>
#include <pthread.h>
#include <stdint.h>
#include <stdlib.h>
#include <string.h>
#include <time.h>
#include <unistd.h>

namespace {

// One recursive lock for every guarded static: an initialiser may run another (nested statics on one thread).
pthread_mutex_t gGuardLock;
pthread_once_t gGuardOnce = PTHREAD_ONCE_INIT;

void InitGuardLock() {
	pthread_mutexattr_t a;
	pthread_mutexattr_init(&a);
	pthread_mutexattr_settype(&a, PTHREAD_MUTEX_RECURSIVE);
	pthread_mutex_init(&gGuardLock, &a);
	pthread_mutexattr_destroy(&a);
}

}  // namespace

// ARM EABI guard: a 32-bit word, bit 0 set once the object is initialised.
extern "C" int __cxa_guard_acquire(int* aGuard) {
	if (__atomic_load_n(aGuard, __ATOMIC_ACQUIRE) & 1)
		return 0;
	pthread_once(&gGuardOnce, InitGuardLock);
	pthread_mutex_lock(&gGuardLock);
	if (*aGuard & 1) {
		pthread_mutex_unlock(&gGuardLock);
		return 0;
	}
	return 1;  // the caller initialises, then releases (or aborts)
}

extern "C" void __cxa_guard_release(int* aGuard) {
	__atomic_store_n(aGuard, 1, __ATOMIC_RELEASE);
	pthread_mutex_unlock(&gGuardLock);
}

extern "C" void __cxa_guard_abort(int*) {
	pthread_mutex_unlock(&gGuardLock);
}

// ---- emulated TLS: GCC's __emutls_object (libgcc emutls.c's layout); each thread_local variable gets an index, each
// thread an array of per-variable blocks behind one pthread key
struct __emutls_object {
	unsigned int size;
	unsigned int align;
	union {
		uintptr_t offset;  // 1-based index, 0 until the variable is first used
		void* ptr;
	} loc;
	void* templ;  // initial value, or null for zero
};

namespace {

struct Blocks {
	uintptr_t count;
	void* block[1];  // each malloc'd: [raw pointer][padding][the aligned object]
};

pthread_key_t gTlsKey;
pthread_once_t gTlsOnce = PTHREAD_ONCE_INIT;
pthread_mutex_t gTlsLock = PTHREAD_MUTEX_INITIALIZER;
uintptr_t gTlsCount;

void FreeBlocks(void* aBlocks) {
	Blocks* b = static_cast<Blocks*>(aBlocks);
	for (uintptr_t i = 0; i < b->count; ++i)
		if (b->block[i])
			free(static_cast<void**>(b->block[i])[-1]);
	free(b);
}

void InitTls() {
	pthread_key_create(&gTlsKey, FreeBlocks);
}

void* NewObject(const __emutls_object* aObj) {
	uintptr_t align = aObj->align < sizeof(void*) ? sizeof(void*) : aObj->align;
	char* raw = static_cast<char*>(malloc(aObj->size + align + sizeof(void*)));
	if (!raw)
		abort();
	uintptr_t at = (reinterpret_cast<uintptr_t>(raw) + sizeof(void*) + align - 1) & ~(align - 1);
	void* obj = reinterpret_cast<void*>(at);
	static_cast<void**>(obj)[-1] = raw;
	if (aObj->templ)
		memcpy(obj, aObj->templ, aObj->size);
	else
		memset(obj, 0, aObj->size);
	return obj;
}

}  // namespace

// GCC declares it built in as void* (void*)
extern "C" void* __emutls_get_address(void* aObject) {
	__emutls_object* aObj = static_cast<__emutls_object*>(aObject);
	pthread_once(&gTlsOnce, InitTls);
	uintptr_t index = __atomic_load_n(&aObj->loc.offset, __ATOMIC_ACQUIRE);
	if (!index) {
		pthread_mutex_lock(&gTlsLock);
		index = aObj->loc.offset;
		if (!index) {
			index = ++gTlsCount;
			__atomic_store_n(&aObj->loc.offset, index, __ATOMIC_RELEASE);
		}
		pthread_mutex_unlock(&gTlsLock);
	}
	Blocks* b = static_cast<Blocks*>(pthread_getspecific(gTlsKey));
	if (!b || b->count < index) {
		uintptr_t count = index + 16;
		Blocks* nb = static_cast<Blocks*>(realloc(b, sizeof(Blocks) + (count - 1) * sizeof(void*)));
		if (!nb)
			abort();
		memset(&nb->block[b ? nb->count : 0], 0, (count - (b ? nb->count : 0)) * sizeof(void*));
		nb->count = count;
		b = nb;
		pthread_setspecific(gTlsKey, b);
	}
	void*& slot = b->block[index - 1];
	if (!slot)
		slot = NewObject(aObj);
	return slot;
}

// ---- libstdc++'s throw helpers (bits/functexcept.h): no exceptions here, so each is a fatal error (as SpiderMonkey's
// mozalloc does); weak, so a library's own definitions win
extern "C" [[noreturn]] void cxxrt_fatal(const char* aWhat, const char* aDetail);

#define CXXRT_THROW0(name) \
	namespace std { [[noreturn]] __attribute__((weak)) void name() { cxxrt_fatal(#name, nullptr); } }
#define CXXRT_THROW1(name) \
	namespace std { [[noreturn]] __attribute__((weak)) void name(const char* aWhat) { cxxrt_fatal(#name, aWhat); } }
CXXRT_THROW0(__throw_bad_alloc)
CXXRT_THROW0(__throw_bad_array_new_length)
CXXRT_THROW0(__throw_bad_cast)
CXXRT_THROW0(__throw_bad_typeid)
CXXRT_THROW0(__throw_bad_exception)
CXXRT_THROW0(__throw_bad_function_call)
CXXRT_THROW1(__throw_logic_error)
CXXRT_THROW1(__throw_domain_error)
CXXRT_THROW1(__throw_invalid_argument)
CXXRT_THROW1(__throw_length_error)
CXXRT_THROW1(__throw_out_of_range)
CXXRT_THROW1(__throw_runtime_error)
CXXRT_THROW1(__throw_range_error)
CXXRT_THROW1(__throw_overflow_error)
CXXRT_THROW1(__throw_underflow_error)
namespace std {
[[noreturn]] __attribute__((weak)) void __throw_out_of_range_fmt(const char* aFmt, ...) {
	cxxrt_fatal("__throw_out_of_range_fmt", aFmt);
}
[[noreturn]] __attribute__((weak)) void __throw_system_error(int) { cxxrt_fatal("__throw_system_error", nullptr); }
}  // namespace std

// ---- the one libstdc++ function body std::this_thread::sleep_for reaches (it lives in libstdc++'s .a)
namespace std::this_thread {
__attribute__((weak)) void __sleep_for(chrono::seconds aSecs, chrono::nanoseconds aNsecs) {
	struct timespec t = {static_cast<time_t>(aSecs.count()), static_cast<long>(aNsecs.count())};
	while (nanosleep(&t, &t) == -1 && errno == EINTR) {
	}
}
}  // namespace std::this_thread

// ---- the stack protector (SpiderMonkey's hardening builds with -fstack-protector-strong): the canary, from the
// kernel's secure generator before any other constructor runs (priority 101; efile.ld sorts .init_array by priority)
extern "C" {
uintptr_t __stack_chk_guard = 0xe0d4c0deu;
[[noreturn]] void __stack_chk_fail() { cxxrt_fatal("stack smashing detected", nullptr); }
}

// euser's Math::Random (the secure generator), declared here: e32's headers and libstdc++'s <new> both define
// placement new
class Math {
public:
	static unsigned long Random();
};

// ---- POSIX pieces P.I.P.S. lacks (declared by symbian/sdk-compat/c): read-write locks over a mutex, fchown
extern "C" {
int pthread_rwlock_init(pthread_rwlock_t* l, const pthread_rwlockattr_t*) { return pthread_mutex_init(&l->m, nullptr); }
int pthread_rwlock_destroy(pthread_rwlock_t* l) { return pthread_mutex_destroy(&l->m); }
int pthread_rwlock_rdlock(pthread_rwlock_t* l) { return pthread_mutex_lock(&l->m); }
int pthread_rwlock_wrlock(pthread_rwlock_t* l) { return pthread_mutex_lock(&l->m); }
int pthread_rwlock_tryrdlock(pthread_rwlock_t* l) { return pthread_mutex_trylock(&l->m); }
int pthread_rwlock_trywrlock(pthread_rwlock_t* l) { return pthread_mutex_trylock(&l->m); }
int pthread_rwlock_unlock(pthread_rwlock_t* l) { return pthread_mutex_unlock(&l->m); }
int fchown(int, uid_t, gid_t) { return 0; }
}

// euser's User::AllocLen, declared the same way: P.I.P.S.'s malloc allocates from the thread's RHeap
class User {
public:
	static int AllocLen(const void* aCell);
};

// malloc_usable_size (glibc's), which P.I.P.S. lacks: Servo's allocator and SpiderMonkey use it for memory reports
extern "C" size_t malloc_usable_size(void* p) { return p ? size_t(User::AllocLen(p)) : 0; }

__attribute__((constructor(101))) static void SeedStackGuard() {
	__stack_chk_guard = Math::Random() | 0xffu;  // never 0 in the low byte: a C string cannot copy over it
}

// ---- array new/delete: as the single-object forms (build's start-up code and euser provide those); weak, in case
// the EXE's start-up code provides them too
__attribute__((weak)) void* operator new[](std::size_t aSize) { return ::operator new(aSize); }
__attribute__((weak)) void operator delete[](void* aPtr) noexcept { ::operator delete(aPtr); }

// ---- the ARM EH personality routines the C++ code's unwind tables name (e32 has __aeabi_unwind_cpp_pr0): nothing
// unwinds here (Rust panics abort, the C++ has no exceptions), and leaving them undefined would pull libgcc's
// unwinder in over Rust std's _Unwind_* stubs
extern "C" {
[[noreturn]] void __aeabi_unwind_cpp_pr1() { cxxrt_fatal("unwinding (__aeabi_unwind_cpp_pr1)", nullptr); }
[[noreturn]] void __aeabi_unwind_cpp_pr2() { cxxrt_fatal("unwinding (__aeabi_unwind_cpp_pr2)", nullptr); }
}
