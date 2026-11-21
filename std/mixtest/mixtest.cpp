// SPDX-License-Identifier: Apache-2.0
// MIXTEST.EXE's C++ side (docs browser/wb2-std.md, WB2c): one EXE mixing Rust with std, e32 and Symbian C++.
// - mix_cpp(): Symbian C++ (a CBase array under TRAP, a descriptor) that calls back into Rust;
// - mix_layout_*(): P.I.P.S.'s sizes, offsets and constants as its headers give them (PIPSPROB's layout.inc), for
//   the Rust side to compare with the NeoSymbian/libc fork.
#include <stddef.h>
#include <errno.h>
#include <fcntl.h>
#include <unistd.h>
#include <dirent.h>
#include <time.h>
#include <pthread.h>
#include <poll.h>
#include <netdb.h>
#include <sys/types.h>
#include <sys/select.h>
#include <sys/stat.h>
#include <sys/time.h>
#include <sys/uio.h>
#include <sys/mman.h>
#include <sys/ioctl.h>
#include <sys/socket.h>
#include <netinet/in.h>
#include <netinet/tcp.h>
#include <limits.h>
#include <sys/syslimits.h>
#include <e32base.h>

#define ABSENT (-0x7fffffffL)

struct Row {
    const char* name;
    long value;
};
enum SmallEnum { ESmallA, ESmallB };
static const Row kLayout[] = {
#include "layout.inc"
    {"sizeof(enum)", (long)sizeof(SmallEnum)},
};

extern "C" {

int mix_layout_count() { return sizeof kLayout / sizeof kLayout[0]; }
const char* mix_layout_name(int i) { return kLayout[i].name; }
long mix_layout_value(int i) { return kLayout[i].value; }

// Sums 1..n through a CArrayFixFlat built under TRAP, each element first passed through the Rust callback; returns
// the sum, or the leave code. Also checks a descriptor round trip.
int mix_cpp(int n, int (*rust_cb)(int)) {
    TInt sum = 0;
    TRAPD(err, {
        CArrayFixFlat<TInt>* a = new (ELeave) CArrayFixFlat<TInt>(4);
        CleanupStack::PushL(a);
        for (TInt i = 1; i <= n; i++) a->AppendL(rust_cb(i));
        for (TInt i = 0; i < a->Count(); i++) sum += (*a)[i];
        CleanupStack::PopAndDestroy(a);
    });
    if (err != KErrNone) return err;
    TBuf8<16> b;
    b.Format(_L8("n=%d"), n);
    return b.Length() >= 3 ? sum : -1;
}

}  // extern "C"
