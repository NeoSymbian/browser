// SPDX-License-Identifier: Apache-2.0
// CXXSMOKE's C++ half (lib.rs): what SpiderMonkey's C++ leans on, built with the SDK's armv8-symbian-g++ (C++20,
// libstdc++'s headers over P.I.P.S.).
#include <algorithm>
#include <atomic>
#include <array>
#include <cmath>
#include <cstdint>
#include <cstdio>
#include <cstring>
#include <ctime>
#include <functional>
#include <memory>
#include <optional>
#include <string>
#include <variant>
#include <vector>
#include <arm_neon.h>

namespace {
std::atomic<int> gInits{0};
struct Slow {
	int value;
	Slow() : value(42) {
		gInits.fetch_add(1);
		struct timespec t = {0, 20 * 1000 * 1000};  // 20 ms: the other thread must wait, not run it again
		nanosleep(&t, nullptr);
	}
};
thread_local int tLocal = 7;
thread_local std::array<int, 64> tBig{};  // a zero-initialised block
std::atomic<uint64_t> g64{0};
}  // namespace

// a function-local static initialised from two threads at once: once only
extern "C" int smoke_guard() {
	static Slow s;
	return s.value;
}
extern "C" int smoke_guard_inits() { return gInits.load(); }

// thread_local: starts at 7 on every thread; returns the thread's value after adding aAdd
extern "C" int smoke_tls(int aAdd) {
	tLocal += aAdd;
	tBig[63] += aAdd;
	return tLocal * 1000 + tBig[63];
}

// 64-bit atomics (LDREXD/STREXD)
extern "C" uint64_t smoke_atomic64(uint64_t aAdd) { return g64.fetch_add(aAdd) + aAdd; }

// NEON: 4 floats at once
extern "C" float smoke_neon() {
	float32x4_t a = vdupq_n_f32(1.5f), b = {1.0f, 2.0f, 3.0f, 4.0f};
	float32x4_t c = vmlaq_f32(a, b, b);  // 1.5 + b*b
	return vgetq_lane_f32(c, 0) + vgetq_lane_f32(c, 1) + vgetq_lane_f32(c, 2) + vgetq_lane_f32(c, 3);  // 36
}

// libstdc++ containers and algorithms, std::function, std::variant, std::optional, std::string, <cmath>
extern "C" int smoke_stl(char* aOut, int aLen) {
	std::vector<int> v{5, 3, 9, 1, 7};
	std::sort(v.begin(), v.end());
	std::function<int(int)> twice = [](int x) { return 2 * x; };
	std::variant<int, double> var = 2.5;
	std::optional<int> opt = v.back();
	std::string s = "js";
	s += std::to_string(v.front()) + "-" + std::to_string(twice(*opt));
	auto up = std::make_unique<int>(int(std::floor(std::get<double>(var))));
	std::snprintf(aOut, aLen, "%s/%d/%d", s.c_str(), *up, int(std::sqrt(81.0)));
	return v == std::vector<int>{1, 3, 5, 7, 9} ? 0 : 1;  // aOut: js1-18/2/9
}
