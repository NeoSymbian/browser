// SPDX-License-Identifier: Apache-2.0
//! US1 (docs user-side-smp.md): stress the parts of P.I.P.S. that were only safe on one core. Each check runs 8
//! threads against the same object for many rounds; a race shows up as a wrong count, an error return, a mismatch,
//! a hang (no summary line) or a panic. On one core (QEMU Syborg) they pass with or without the fixes.

use crate::Outcome::{self, Failed, Pass};
use std::cell::UnsafeCell;
use std::fs;
use std::os::unix::fs::FileExt;
use std::os::unix::io::AsRawFd;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Barrier, Condvar, Mutex};
use std::thread;
use std::time::{Duration, Instant};

const THREADS: usize = 8;
const DIR: &str = "C:/stdtest_smp";

// libpthread exports the libc fork does not declare for Symbian
unsafe extern "C" {
    fn pthread_once(once: *mut libc::pthread_once_t, init: extern "C" fn()) -> libc::c_int;
    fn sem_init(sem: *mut libc::sem_t, pshared: libc::c_int, value: libc::c_uint) -> libc::c_int;
    fn sem_destroy(sem: *mut libc::sem_t) -> libc::c_int;
    fn sem_timedwait(sem: *mut libc::sem_t, abstime: *const libc::timespec) -> libc::c_int;
}

/// The object under test, shared on purpose; P.I.P.S. (not Rust) is what must keep it consistent.
struct Racy<T>(UnsafeCell<T>);
unsafe impl<T> Sync for Racy<T> {}
impl<T> Racy<T> {
    fn new(v: T) -> Self {
        Self(UnsafeCell::new(v))
    }
    fn get(&self) -> *mut T {
        self.0.get()
    }
}

/// `rounds` rounds of `work(thread, round)` on THREADS threads that start each round together; `prepare(round)`
/// runs on the caller before a round, `finish(round)` after it.
fn in_rounds(
    rounds: usize,
    mut prepare: impl FnMut(usize),
    work: impl Fn(usize, usize) + Sync,
    mut finish: impl FnMut(usize),
) {
    let (start, end) = (Barrier::new(THREADS + 1), Barrier::new(THREADS + 1));
    thread::scope(|s| {
        for t in 0..THREADS {
            let (start, end, work) = (&start, &end, &work);
            s.spawn(move || {
                for r in 0..rounds {
                    start.wait();
                    work(t, r);
                    end.wait();
                }
            });
        }
        for r in 0..rounds {
            prepare(r);
            start.wait();
            end.wait();
            finish(r);
        }
    });
}

fn realtime_after(d: Duration) -> libc::timespec {
    let mut ts = libc::timespec {
        tv_sec: 0,
        tv_nsec: 0,
    };
    unsafe { libc::clock_gettime(libc::CLOCK_REALTIME, &mut ts) };
    let ns = ts.tv_nsec as u64 + d.as_nanos() as u64;
    ts.tv_sec += (ns / 1_000_000_000) as libc::time_t;
    ts.tv_nsec = (ns % 1_000_000_000) as _;
    ts
}

/// P3: a timed wait that times out while a notify is taking its node (use-after-free post, lost wakeup).
pub fn condvar_timeout_race() -> Outcome {
    let pair = Arc::new((Mutex::new(()), Condvar::new()));
    let done = Arc::new(AtomicBool::new(false));
    let notifiers: Vec<_> = (0..THREADS / 2)
        .map(|_| {
            let (p, done) = (pair.clone(), done.clone());
            thread::spawn(move || {
                let mut n = 0u64;
                while !done.load(Ordering::Relaxed) {
                    p.1.notify_one();
                    n += 1;
                }
                n
            })
        })
        .collect();
    let t = Instant::now();
    let waiters: Vec<_> = (0..THREADS / 2)
        .map(|_| {
            let p = pair.clone();
            thread::spawn(move || {
                (0..1000)
                    .filter(|_| {
                        let g = p.0.lock().unwrap();
                        !p.1.wait_timeout(g, Duration::from_micros(500))
                            .unwrap()
                            .1
                            .timed_out()
                    })
                    .count()
            })
        })
        .collect();
    let woken: usize = waiters.into_iter().map(|h| h.join().unwrap()).sum();
    done.store(true, Ordering::Relaxed);
    let notifies: u64 = notifiers.into_iter().map(|h| h.join().unwrap()).sum();
    // park_timeout/unpark is the same race through std's pthread parker
    let parker_done = Arc::new(AtomicBool::new(false));
    let parker = {
        let d = parker_done.clone();
        thread::spawn(move || {
            for _ in 0..2000 {
                thread::park_timeout(Duration::from_micros(300));
            }
            d.store(true, Ordering::Release);
        })
    };
    let target = parker.thread().clone();
    let mut unparks = 0u64;
    while !parker_done.load(Ordering::Acquire) {
        target.unpark();
        unparks += 1;
    }
    parker.join().unwrap();
    Pass(format!(
        "4 x 1000 wait_timeout vs {notifies} notify_one ({woken} woken); 2000 park_timeout vs {unparks} unpark; {} ms",
        t.elapsed().as_millis()
    ))
}

/// P4, P5: the first use of PTHREAD_MUTEX_INITIALIZER / PTHREAD_COND_INITIALIZER objects, from 8 threads at once.
pub fn static_init_race() -> Outcome {
    const N: usize = 16;
    struct Objs {
        m: [libc::pthread_mutex_t; N],
        count: [usize; N],
        c: libc::pthread_cond_t,
        own: [libc::pthread_mutex_t; THREADS],
    }
    let fresh = || Objs {
        m: [libc::PTHREAD_MUTEX_INITIALIZER; N],
        count: [0; N],
        c: libc::PTHREAD_COND_INITIALIZER,
        own: [libc::PTHREAD_MUTEX_INITIALIZER; THREADS],
    };
    let o = Racy::new(fresh());
    let errors = AtomicUsize::new(0);
    let (mut lost, rounds) = (0usize, 300);
    in_rounds(
        rounds,
        |_| unsafe { *o.get() = fresh() },
        |t, _| unsafe {
            let o = o.get();
            let mut bad = 0;
            for i in 0..N {
                bad += (libc::pthread_mutex_lock(&raw mut (*o).m[i]) != 0) as usize;
                (*o).count[i] += 1;
                bad += (libc::pthread_mutex_unlock(&raw mut (*o).m[i]) != 0) as usize;
            }
            let own = &raw mut (*o).own[t];
            bad += (libc::pthread_mutex_lock(own) != 0) as usize;
            let ts = realtime_after(Duration::from_micros(200));
            let rc = libc::pthread_cond_timedwait(&raw mut (*o).c, own, &ts);
            bad += (rc != 0 && rc != libc::ETIMEDOUT) as usize;
            bad += (libc::pthread_mutex_unlock(own) != 0) as usize;
            bad += (libc::pthread_cond_signal(&raw mut (*o).c) != 0) as usize;
            errors.fetch_add(bad, Ordering::Relaxed);
        },
        |_| unsafe {
            let o = o.get();
            lost += (*o).count.iter().filter(|&&c| c != THREADS).count();
            for m in (*o).m.iter_mut().chain((*o).own.iter_mut()) {
                libc::pthread_mutex_destroy(m);
            }
            libc::pthread_cond_destroy(&raw mut (*o).c);
        },
    );
    let errors = errors.into_inner();
    check!(
        errors == 0 && lost == 0,
        "{errors} error returns, {lost} lost counts"
    );
    Pass(format!(
        "{rounds} rounds: {N} static mutexes + 1 static cond, first used by {THREADS} threads"
    ))
}

static ONCE_CALLS: AtomicUsize = AtomicUsize::new(0);
static ONCE_DATA: Racy<[u32; 64]> = Racy(UnsafeCell::new([0; 64]));

extern "C" fn once_init() {
    ONCE_CALLS.fetch_add(1, Ordering::Relaxed);
    thread::sleep(Duration::from_micros(50));
    for (i, v) in unsafe { (*ONCE_DATA.get()).iter_mut() }.enumerate() {
        *v = i as u32 + 1;
    }
}

/// P6: pthread_once's unlocked `_EDone` check must not let a caller read data the init routine has not published.
pub fn once_race() -> Outcome {
    let once = Racy::new(libc::PTHREAD_ONCE_INIT);
    let bad = AtomicUsize::new(0);
    let rounds = 300;
    let mut extra_calls = 0;
    in_rounds(
        rounds,
        |_| unsafe {
            *once.get() = libc::PTHREAD_ONCE_INIT;
            *ONCE_DATA.get() = [0; 64];
            ONCE_CALLS.store(0, Ordering::Relaxed);
        },
        |_, _| {
            let rc = unsafe { pthread_once(once.get(), once_init) };
            let sum: u32 = unsafe { (*ONCE_DATA.get()).iter().sum() };
            bad.fetch_add((rc != 0 || sum != 64 * 65 / 2) as usize, Ordering::Relaxed);
        },
        |_| extra_calls += ONCE_CALLS.load(Ordering::Relaxed).abs_diff(1),
    );
    let bad = bad.into_inner();
    check!(
        bad == 0 && extra_calls == 0,
        "{bad} early reads or errors, {extra_calls} extra init calls"
    );
    Pass(format!(
        "{rounds} rounds of pthread_once from {THREADS} threads"
    ))
}

/// P7: fopen/fclose from 8 threads (stdio picks FILE slots without a lock).
pub fn stdio_race() -> Outcome {
    let _ = fs::create_dir_all(DIR);
    let bad = AtomicUsize::new(0);
    let rounds = 100;
    in_rounds(
        rounds,
        |_| {},
        |t, r| unsafe {
            let path = std::ffi::CString::new(format!("{DIR}/f{t}.txt")).unwrap();
            let text = format!("thread {t} round {r}\n");
            let f = libc::fopen(path.as_ptr(), c"w".as_ptr());
            if f.is_null() {
                bad.fetch_add(1, Ordering::Relaxed);
                return;
            }
            let wrote = libc::fwrite(text.as_ptr().cast(), 1, text.len(), f);
            let closed = libc::fclose(f);
            let mut buf = [0u8; 64];
            let f = libc::fopen(path.as_ptr(), c"r".as_ptr());
            let read = if f.is_null() {
                0
            } else {
                libc::fread(buf.as_mut_ptr().cast(), 1, buf.len(), f)
            };
            let closed2 = if f.is_null() { -1 } else { libc::fclose(f) };
            let ok = wrote == text.len()
                && closed == 0
                && closed2 == 0
                && &buf[..read] == text.as_bytes();
            bad.fetch_add(!ok as usize, Ordering::Relaxed);
        },
        |_| {},
    );
    let _ = fs::remove_dir_all(DIR);
    let bad = bad.into_inner();
    check!(
        bad == 0,
        "{bad} of {} fopen/fwrite/fclose/read-back rounds wrong",
        rounds * THREADS
    );
    Pass(format!(
        "{rounds} rounds of fopen/fwrite/fclose + read back on {THREADS} threads"
    ))
}

fn g17(x: f64) -> String {
    let mut buf = [0u8; 64];
    let n = unsafe { libc::snprintf(buf.as_mut_ptr().cast(), buf.len(), c"%.17g".as_ptr(), x) };
    String::from_utf8_lossy(&buf[..n.max(0) as usize]).into_owned()
}

/// P8 (+ P4 through gdtoa's static lock): printf %g from 8 threads while gdtoa grows its cache of powers of 5.
/// The cache only grows once per process, so the first round is the real test; run STDTEST fresh.
pub fn printf_g_race() -> Outcome {
    let values: Vec<f64> = (1..=30)
        .map(|e| 1.234_567_890_123_456_7 * 10f64.powi(e * 10))
        .collect();
    let got: Vec<Mutex<Vec<String>>> = (0..THREADS).map(|_| Mutex::new(Vec::new())).collect();
    in_rounds(
        20,
        |_| {},
        |t, _| {
            let mut out = got[t].lock().unwrap();
            out.clear();
            out.extend(values.iter().map(|&v| g17(v)));
        },
        |_| {},
    );
    let want: Vec<String> = values.iter().map(|&v| g17(v)).collect();
    let bad = got.iter().filter(|g| *g.lock().unwrap() != want).count();
    check!(
        bad == 0,
        "{bad} threads printed differently, e.g. want {:?}",
        want.last()
    );
    Pass(format!(
        "{} values to 1e300 with %.17g on {THREADS} threads",
        values.len()
    ))
}

/// P1, P2: 8 threads write and read one fd while 2 more dup/close it (fd table without its lock, and the
/// descriptor's reference count updated non-atomically on every read/write).
pub fn shared_fd_race() -> Outcome {
    let _ = fs::create_dir_all(DIR);
    let path = format!("{DIR}/shared.bin");
    let f = match fs::OpenOptions::new()
        .create(true)
        .truncate(true)
        .read(true)
        .write(true)
        .open(&path)
    {
        Ok(f) => f,
        Err(e) => return Failed(format!("open {path}: {e}")),
    };
    const REC: usize = 32;
    let rounds = 300;
    let record =
        |t: usize, r: usize| format!("{t:02} {r:05} {:>22}\n", t * 100_000 + r).into_bytes();
    let done = AtomicBool::new(false);
    let (bad, churn) = (AtomicUsize::new(0), AtomicUsize::new(0));
    thread::scope(|s| {
        for _ in 0..2 {
            s.spawn(|| {
                while !done.load(Ordering::Relaxed) {
                    let d = unsafe { libc::dup(f.as_raw_fd()) };
                    let ok = d >= 0 && unsafe { libc::close(d) } == 0;
                    bad.fetch_add(!ok as usize, Ordering::Relaxed);
                    churn.fetch_add(1, Ordering::Relaxed);
                }
            });
        }
        thread::scope(|w| {
            for t in 0..THREADS {
                let (f, bad) = (&f, &bad);
                w.spawn(move || {
                    for r in 0..rounds {
                        let off = ((t * rounds + r) * REC) as u64;
                        let rec = record(t, r);
                        let mut back = [0u8; REC];
                        let ok = f.write_all_at(&rec, off).is_ok()
                            && f.read_exact_at(&mut back, off).is_ok()
                            && back[..] == rec[..];
                        bad.fetch_add(!ok as usize, Ordering::Relaxed);
                    }
                });
            }
        });
        done.store(true, Ordering::Relaxed);
    });
    let all = fs::read(&path).unwrap_or_default();
    drop(f);
    let _ = fs::remove_dir_all(DIR);
    let expect: Vec<u8> = (0..THREADS)
        .flat_map(|t| (0..rounds).flat_map(move |r| record(t, r)))
        .collect();
    let bad = bad.into_inner();
    check!(
        bad == 0 && all == expect,
        "{bad} failed ops; file {} of {} bytes, equal {}",
        all.len(),
        expect.len(),
        all == expect
    );
    Pass(format!(
        "{THREADS} x {rounds} write/read on one fd, {} dup/close meanwhile",
        churn.into_inner()
    ))
}

/// P9: sem_timedwait timing out while a sem_post hands it a token must not leave an extra token behind.
pub fn sem_timeout_race() -> Outcome {
    let sem = Racy::new(std::ptr::null_mut::<libc::c_void>());
    if unsafe { sem_init(sem.get(), 0, 0) } != 0 {
        return Failed("sem_init".into());
    }
    const POSTS: usize = 3000;
    let (got, errors) = (AtomicUsize::new(0), AtomicUsize::new(0));
    let waiting = AtomicBool::new(true);
    thread::scope(|s| {
        for _ in 0..4 {
            s.spawn(|| {
                for _ in 0..1000 {
                    let ts = realtime_after(Duration::from_micros(300));
                    if unsafe { sem_timedwait(sem.get(), &ts) } == 0 {
                        got.fetch_add(1, Ordering::Relaxed);
                    } else if std::io::Error::last_os_error()
                        .raw_os_error()
                        .is_some_and(|e| e != libc::ETIMEDOUT && e != 0)
                    {
                        errors.fetch_add(1, Ordering::Relaxed);
                    }
                }
            });
        }
        s.spawn(|| {
            for i in 0..POSTS {
                if unsafe { libc::sem_post(sem.get()) } != 0 {
                    errors.fetch_add(1, Ordering::Relaxed);
                }
                if i % 4 == 0 {
                    thread::sleep(Duration::from_micros(100));
                }
            }
            waiting.store(false, Ordering::Relaxed);
        });
    });
    let mut drained = 0;
    while unsafe { libc::sem_trywait(sem.get()) } == 0 {
        drained += 1;
    }
    unsafe { sem_destroy(sem.get()) };
    let (got, errors) = (got.into_inner(), errors.into_inner());
    check!(
        got + drained == POSTS && errors == 0 && !waiting.into_inner(),
        "{POSTS} posts but {got} waits + {drained} left = {} tokens; {errors} errors",
        got + drained
    );
    Pass(format!(
        "{POSTS} posts: {got} taken by 4 timed waiters, {drained} left over"
    ))
}
