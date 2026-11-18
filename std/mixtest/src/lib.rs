// SPDX-License-Identifier: Apache-2.0
//! MIXTEST.EXE (docs browser-std.md, WB2c): one EXE mixing Rust's std, the e32 crate and Symbian C++, the WB2 gate.
//! It also checks the NeoSymbian/libc fork against P.I.P.S.'s own headers: the C++ side (mixtest.cpp) carries
//! PIPSPROB's layout table, compiled against them.

use std::collections::HashMap;
use std::ffi::{CStr, c_char, c_int, c_long};
use std::mem::{offset_of, size_of};
use std::sync::atomic::{AtomicU32, Ordering};
use std::thread;

e32::main!(main);

unsafe extern "C" {
    fn mix_layout_count() -> c_int;
    fn mix_layout_name(i: c_int) -> *const c_char;
    fn mix_layout_value(i: c_int) -> c_long;
    fn mix_cpp(n: c_int, cb: extern "C" fn(c_int) -> c_int) -> c_int;
}

static CALLBACKS: AtomicU32 = AtomicU32::new(0);

extern "C" fn square(x: c_int) -> c_int {
    CALLBACKS.fetch_add(1, Ordering::SeqCst);
    x * x
}

#[repr(C)]
#[allow(dead_code)]
enum SmallEnum {
    A,
    B,
}

/// The libc fork's view of each row of PIPSPROB's table it defines.
fn rust_layout() -> HashMap<&'static str, i64> {
    use libc::*;
    let mut m: HashMap<&'static str, i64> = HashMap::new();
    macro_rules! size {
        ($($n:literal => $t:ty),* $(,)?) => { $( m.insert($n, size_of::<$t>() as i64); )* };
    }
    macro_rules! off {
        ($($n:literal => $t:ty, $f:ident);* $(;)?) => { $( m.insert($n, offset_of!($t, $f) as i64); )* };
    }
    macro_rules! val {
        ($($c:ident),* $(,)?) => { $( m.insert(stringify!($c), $c as i64); )* };
    }
    size! {
        "sizeof(long)" => c_long, "sizeof(size_t)" => size_t, "sizeof(off_t)" => off_t, "sizeof(time_t)" => time_t,
        "sizeof(mode_t)" => mode_t, "sizeof(dev_t)" => dev_t, "sizeof(ino_t)" => ino_t, "sizeof(nlink_t)" => nlink_t,
        "sizeof(blkcnt_t)" => blkcnt_t, "sizeof(socklen_t)" => socklen_t, "sizeof(in_port_t)" => in_port_t,
        "sizeof(nfds_t)" => nfds_t, "sizeof(wchar_t)" => wchar_t, "sizeof(pthread_t)" => pthread_t,
        "sizeof(pthread_attr_t)" => pthread_attr_t, "sizeof(pthread_mutex_t)" => pthread_mutex_t,
        "sizeof(pthread_mutexattr_t)" => pthread_mutexattr_t, "sizeof(pthread_cond_t)" => pthread_cond_t,
        "sizeof(pthread_condattr_t)" => pthread_condattr_t, "sizeof(pthread_key_t)" => pthread_key_t,
        "sizeof(fd_set)" => fd_set, "sizeof(struct stat)" => stat, "sizeof(struct dirent)" => dirent,
        "sizeof(struct timespec)" => timespec, "sizeof(struct timeval)" => timeval,
        "sizeof(struct sockaddr)" => sockaddr, "sizeof(struct sockaddr_in)" => sockaddr_in,
        "sizeof(struct sockaddr_in6)" => sockaddr_in6, "sizeof(struct sockaddr_storage)" => sockaddr_storage,
        "sizeof(struct addrinfo)" => addrinfo, "sizeof(struct pollfd)" => pollfd, "sizeof(struct linger)" => linger,
        "sizeof(struct iovec)" => iovec, "sizeof(enum)" => crate::SmallEnum,
    }
    off! {
        "stat.st_dev" => stat, st_dev; "stat.st_ino" => stat, st_ino; "stat.st_mode" => stat, st_mode;
        "stat.st_nlink" => stat, st_nlink; "stat.st_uid" => stat, st_uid; "stat.st_gid" => stat, st_gid;
        "stat.st_rdev" => stat, st_rdev; "stat.st_size" => stat, st_size; "stat.st_atime" => stat, st_atime;
        "stat.st_mtime" => stat, st_mtime; "stat.st_ctime" => stat, st_ctime; "stat.st_blksize" => stat, st_blksize;
        "stat.st_blocks" => stat, st_blocks; "dirent.d_ino" => dirent, d_ino; "dirent.d_name" => dirent, d_name;
        "timespec.tv_nsec" => timespec, tv_nsec; "timeval.tv_usec" => timeval, tv_usec;
        "sockaddr.sa_family" => sockaddr, sa_family; "sockaddr.sa_data" => sockaddr, sa_data;
        "sockaddr_in.sin_port" => sockaddr_in, sin_port; "sockaddr_in.sin_addr" => sockaddr_in, sin_addr;
        "sockaddr_in.sin_zero" => sockaddr_in, sin_zero; "sockaddr_in6.sin6_port" => sockaddr_in6, sin6_port;
        "sockaddr_in6.sin6_flowinfo" => sockaddr_in6, sin6_flowinfo; "sockaddr_in6.sin6_addr" => sockaddr_in6, sin6_addr;
        "sockaddr_in6.sin6_scope_id" => sockaddr_in6, sin6_scope_id; "addrinfo.ai_addrlen" => addrinfo, ai_addrlen;
        "addrinfo.ai_canonname" => addrinfo, ai_canonname; "addrinfo.ai_addr" => addrinfo, ai_addr;
        "addrinfo.ai_next" => addrinfo, ai_next; "pollfd.events" => pollfd, events; "pollfd.revents" => pollfd, revents;
        "linger.l_linger" => linger, l_linger; "iovec.iov_len" => iovec, iov_len;
    }
    val! {
        EPERM, ENOENT, EINTR, EIO, EBADF, EAGAIN, EWOULDBLOCK, ENOMEM, EACCES, EEXIST, ENOTDIR, EISDIR, EINVAL, ENFILE,
        EMFILE, ENOSPC, ESPIPE, EROFS, EPIPE, ERANGE, EDEADLK, ENAMETOOLONG, ENOSYS, ENOTEMPTY, ELOOP, EINPROGRESS,
        EALREADY, ENOTSOCK, EMSGSIZE, EPROTONOSUPPORT, EOPNOTSUPP, ENOTSUP, EAFNOSUPPORT, EADDRINUSE, EADDRNOTAVAIL,
        ENETDOWN, ENETUNREACH, ECONNABORTED, ECONNRESET, ENOBUFS, EISCONN, ENOTCONN, ETIMEDOUT, ECONNREFUSED,
        EHOSTUNREACH, EXDEV, EBUSY, EFBIG, EMLINK, ETXTBSY, O_RDONLY, O_WRONLY, O_RDWR, O_ACCMODE, O_CREAT, O_EXCL,
        O_TRUNC, O_APPEND, O_NONBLOCK, O_NOFOLLOW, O_SYNC, F_DUPFD, F_GETFD, F_SETFD, F_GETFL, F_SETFL, FD_CLOEXEC,
        FD_SETSIZE, S_IFMT, S_IFDIR, S_IFREG, S_IFLNK, S_IFIFO, S_IFSOCK, S_IFCHR, S_IFBLK, SEEK_SET, SEEK_CUR,
        SEEK_END, AF_UNSPEC, AF_INET, AF_INET6, AF_UNIX, AF_LOCAL, SOCK_STREAM, SOCK_DGRAM, SOCK_RAW, SOL_SOCKET,
        SO_REUSEADDR, SO_ERROR, SO_TYPE, SO_RCVTIMEO, SO_SNDTIMEO, SO_BROADCAST, SO_LINGER, SO_KEEPALIVE, SO_RCVBUF,
        SO_SNDBUF, IPPROTO_IP, IPPROTO_IPV6, IPPROTO_TCP, IPPROTO_UDP, TCP_NODELAY, IP_TTL, IPV6_V6ONLY,
        IP_MULTICAST_TTL, IP_MULTICAST_LOOP, IP_ADD_MEMBERSHIP, IPV6_JOIN_GROUP, MSG_PEEK, MSG_DONTWAIT,
        MSG_NOSIGNAL, MSG_WAITALL, SHUT_RD, SHUT_WR, SHUT_RDWR, POLLIN, POLLPRI, POLLOUT, POLLERR, POLLHUP, POLLNVAL,
        FIONBIO, FIONREAD, INADDR_ANY, INADDR_LOOPBACK, AI_PASSIVE, AI_CANONNAME, AI_NUMERICHOST, EAI_AGAIN,
        EAI_FAIL, EAI_FAMILY, EAI_NONAME, EAI_SERVICE, EAI_SOCKTYPE, EAI_SYSTEM, CLOCK_REALTIME, CLOCK_MONOTONIC,
        PTHREAD_STACK_MIN, _SC_PAGESIZE, _SC_NPROCESSORS_ONLN, PATH_MAX, PROT_NONE, PROT_READ, PROT_WRITE,
        MAP_PRIVATE, DT_DIR, DT_REG, DT_LNK, DT_UNKNOWN, STDIN_FILENO, PTHREAD_MUTEX_NORMAL, PTHREAD_MUTEX_RECURSIVE,
        PTHREAD_MUTEX_DEFAULT, PTHREAD_MUTEX_ERRORCHECK, PTHREAD_CREATE_JOINABLE, PTHREAD_CREATE_DETACHED,
    }
    m
}

fn layout_check() -> (usize, Vec<String>) {
    let rust = rust_layout();
    let mut compared = 0;
    let mut bad = Vec::new();
    // SAFETY: the C++ table is static; indices stay below its count.
    for i in 0..unsafe { mix_layout_count() } {
        let name = unsafe { CStr::from_ptr(mix_layout_name(i)) }
            .to_str()
            .unwrap_or("?");
        let c = unsafe { mix_layout_value(i) } as i64;
        if let Some(&r) = rust.get(name) {
            compared += 1;
            if r != c {
                bad.push(format!("{name}: libc {r}, P.I.P.S. {c}"));
            }
        }
    }
    (compared, bad)
}

fn main() -> i32 {
    let mut failed = 0;
    // e32: the kernel debug port and a kernel object, beside std's println on the same port
    e32::dprintln!("mixtest: e32 says hello (pid {})", std::process::id());
    let t0 = e32::sys::ntick_count();

    // std threads calling into C++ that calls back into Rust
    let sums: Vec<i32> = (0..4)
        .map(|k| thread::spawn(move || unsafe { mix_cpp(10 + k, square) }))
        .collect::<Vec<_>>()
        .into_iter()
        .map(|h| h.join().unwrap())
        .collect();
    let want: Vec<i32> = (0..4).map(|k| (1..=10 + k).map(|i| i * i).sum()).collect();
    let cbs = CALLBACKS.load(Ordering::SeqCst);
    if sums == want && cbs == want.len() as u32 * 10 + 6 {
        println!(
            "mix cpp ... ok: 4 std threads -> C++ (TRAP, CArrayFixFlat) -> Rust callbacks: {sums:?}"
        );
    } else {
        failed += 1;
        println!("mix cpp ... FAILED: {sums:?} (want {want:?}), {cbs} callbacks");
    }

    // std collections and files beside e32 calls
    let mut m = HashMap::new();
    m.insert("ticks", e32::sys::ntick_count().wrapping_sub(t0));
    let path = "C:/mixtest.txt";
    let ok = std::fs::write(path, format!("{m:?}")).is_ok()
        && std::fs::read_to_string(path).is_ok_and(|s| s.contains("ticks"));
    let _ = std::fs::remove_file(path);
    if ok {
        println!(
            "mix std ... ok: HashMap + fs on C:, e32 tick count {:?}",
            m["ticks"]
        );
    } else {
        failed += 1;
        println!("mix std ... FAILED: file round trip");
    }

    let (compared, bad) = layout_check();
    if bad.is_empty() && compared > 150 {
        println!(
            "mix layout ... ok: {compared} sizes, offsets and constants match P.I.P.S.'s headers"
        );
    } else {
        failed += 1;
        println!("mix layout ... FAILED: {} of {compared} differ", bad.len());
        for b in &bad {
            println!("mix layout   {b}");
        }
    }
    println!(
        "mixtest: {}",
        if failed == 0 {
            "all ok".to_string()
        } else {
            format!("{failed} FAILED")
        }
    );
    failed
}
