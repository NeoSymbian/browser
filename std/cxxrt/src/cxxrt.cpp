// SPDX-License-Identifier: Apache-2.0
// The C++ runtime for ARMv7 C++ on NeoSymbian (cxxrt/src/lib.rs): __cxa_guard_* and __emutls_get_address over
// P.I.P.S.'s pthreads (GCC for arm-none-eabi emits calls to both: function-local statics are thread-safe by default,
// and thread_local uses emulated TLS), and libstdc++'s std::__throw_* helpers as fatal errors.
#include <pthread.h>
#include <stdlib.h>
#include <string.h>
#include <stdint.h>

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
