// SPDX-License-Identifier: Apache-2.0
//! STDTEST.EXE (docs browser-std.md, WB2c): Rust's std on NeoSymbian, one check per area std's own tests cover:
//! threads, locks, time, files, environment, collections, formatting, the allocator and sockets. Each check prints
//! `std <name> ... ok|FAILED|skipped: <detail>` (stdout is the kernel debug port), then a summary line.

// The thread_local! initialisers are const already; the lint misreads std's key-based TLS expansion (no #[thread_local])
#![allow(clippy::missing_const_for_thread_local)]

use std::collections::HashMap;
use std::fs::{self, File};
use std::hash::{BuildHasher, RandomState};
use std::io::{BufRead, BufReader, Read, Seek, SeekFrom, Write};
use std::os::unix::fs::FileExt;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Barrier, Condvar, Mutex, OnceLock, RwLock, mpsc};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

e32::main!(main);

enum Outcome {
    Pass(String),
    Failed(String),
    Skipped(String),
}
use Outcome::{Failed, Pass, Skipped};

macro_rules! check {
    ($cond:expr, $($fmt:tt)*) => {
        if !$cond {
            return Failed(format!($($fmt)*));
        }
    };
}

fn ms(d: Duration) -> u128 {
    d.as_millis()
}

fn thread_spawn_join() -> Outcome {
    let handles: Vec<_> = (0..4u64)
        .map(|i| thread::spawn(move || (0..1000u64).map(|x| x * i).sum::<u64>()))
        .collect();
    let sum: u64 = handles.into_iter().map(|h| h.join().unwrap()).sum();
    check!(sum == 499500 * 6, "sum {sum}");
    Pass(format!("4 threads, sum {sum}"))
}

fn thread_scope_and_builder() -> Outcome {
    let mut data = vec![1u32, 2, 3, 4];
    thread::scope(|s| {
        for x in data.iter_mut() {
            s.spawn(move || *x *= 10);
        }
    });
    check!(data == [10, 20, 30, 40], "scoped {data:?}");
    let big = thread::Builder::new()
        .name("big".into())
        .stack_size(8 << 20)
        .spawn(|| {
            // use most of an 8 MiB stack
            let mut buf = [0u8; 6 << 20];
            buf[0] = 1;
            buf[buf.len() - 1] = 2;
            std::hint::black_box(&mut buf);
            (
                thread::current().name().map(String::from),
                buf[0] + buf[buf.len() - 1],
            )
        })
        .unwrap()
        .join()
        .unwrap();
    check!(big.1 == 3, "big stack {big:?}");
    Pass(format!("scoped ok; 8 MiB stack thread named {:?}", big.0))
}

struct SetOnDrop(&'static AtomicBool);
impl Drop for SetOnDrop {
    fn drop(&mut self) {
        self.0.store(true, Ordering::SeqCst);
    }
}

fn thread_local_dtor() -> Outcome {
    static DROPPED: AtomicBool = AtomicBool::new(false);
    thread_local! {
        static COUNTER: std::cell::Cell<u32> = const { std::cell::Cell::new(0) };
        static GUARD: std::cell::RefCell<Option<SetOnDrop>> = const { std::cell::RefCell::new(None) };
    }
    COUNTER.with(|c| c.set(5));
    let other = thread::spawn(|| {
        GUARD.with(|g| *g.borrow_mut() = Some(SetOnDrop(&DROPPED)));
        COUNTER.with(|c| {
            c.set(c.get() + 1);
            c.get()
        })
    })
    .join()
    .unwrap();
    let mine = COUNTER.with(|c| c.get());
    check!(mine == 5 && other == 1, "values {mine} {other}");
    check!(
        DROPPED.load(Ordering::SeqCst),
        "thread_local destructor did not run at thread exit"
    );
    Pass("per-thread values, destructor ran at thread exit".into())
}

fn park_timeout() -> Outcome {
    let t = Instant::now();
    thread::park_timeout(Duration::from_millis(100));
    let e = ms(t.elapsed());
    check!((80..1000).contains(&e), "park_timeout(100 ms) took {e} ms");
    let t2 = Instant::now();
    let main = thread::current();
    let h = thread::spawn(move || {
        thread::sleep(Duration::from_millis(30));
        main.unpark();
    });
    thread::park();
    h.join().unwrap();
    Pass(format!(
        "timeout after {e} ms, unpark after {} ms",
        ms(t2.elapsed())
    ))
}

fn mutex_contention() -> Outcome {
    let m = Arc::new(Mutex::new(0u32));
    let hs: Vec<_> = (0..4)
        .map(|_| {
            let m = m.clone();
            thread::spawn(move || {
                for _ in 0..2000 {
                    *m.lock().unwrap() += 1;
                }
            })
        })
        .collect();
    hs.into_iter().for_each(|h| h.join().unwrap());
    let v = *m.lock().unwrap();
    check!(v == 8000, "count {v}");
    Pass("4 x 2000 increments".into())
}

fn rwlock() -> Outcome {
    let l = Arc::new(RwLock::new(vec![0u32]));
    let readers: Vec<_> = (0..3)
        .map(|_| {
            let l = l.clone();
            thread::spawn(move || (0..200).map(|_| l.read().unwrap().len()).sum::<usize>())
        })
        .collect();
    for i in 0..100 {
        l.write().unwrap().push(i);
    }
    let reads: usize = readers.into_iter().map(|h| h.join().unwrap()).sum();
    check!(l.read().unwrap().len() == 101, "len");
    Pass(format!("3 readers ({reads} items seen), 1 writer"))
}

fn condvar_wait_timeout() -> Outcome {
    let pair = (Mutex::new(false), Condvar::new());
    let t = Instant::now();
    let (_g, r) = pair
        .1
        .wait_timeout(pair.0.lock().unwrap(), Duration::from_millis(100))
        .unwrap();
    let e = ms(t.elapsed());
    check!(
        r.timed_out() && (80..1000).contains(&e),
        "timed_out {} after {e} ms",
        r.timed_out()
    );
    let pair = Arc::new((Mutex::new(false), Condvar::new()));
    let p2 = pair.clone();
    let h = thread::spawn(move || {
        thread::sleep(Duration::from_millis(20));
        *p2.0.lock().unwrap() = true;
        p2.1.notify_one();
    });
    let g = pair
        .1
        .wait_while(pair.0.lock().unwrap(), |ready| !*ready)
        .unwrap();
    check!(*g, "not notified");
    drop(g);
    h.join().unwrap();
    Pass(format!("timeout after {e} ms; notify ok"))
}

fn once_lock_and_barrier() -> Outcome {
    static CELL: OnceLock<u32> = OnceLock::new();
    static INITS: AtomicUsize = AtomicUsize::new(0);
    let b = Arc::new(Barrier::new(3));
    let hs: Vec<_> = (0..3)
        .map(|i| {
            let b = b.clone();
            thread::spawn(move || {
                b.wait();
                *CELL.get_or_init(|| {
                    INITS.fetch_add(1, Ordering::SeqCst);
                    40 + i
                })
            })
        })
        .collect();
    let vals: Vec<u32> = hs.into_iter().map(|h| h.join().unwrap()).collect();
    check!(
        INITS.load(Ordering::SeqCst) == 1 && vals.iter().all(|v| *v == vals[0]),
        "inits {INITS:?} vals {vals:?}"
    );
    Pass(format!("one init of 3 racing threads ({})", vals[0]))
}

fn mpsc_channels() -> Outcome {
    let (tx, rx) = mpsc::channel::<u32>();
    let t = Instant::now();
    let r = rx.recv_timeout(Duration::from_millis(50));
    let e = ms(t.elapsed());
    check!(
        matches!(r, Err(mpsc::RecvTimeoutError::Timeout)) && (40..1000).contains(&e),
        "recv_timeout {r:?} after {e} ms"
    );
    let hs: Vec<_> = (0..3)
        .map(|i| {
            let tx = tx.clone();
            thread::spawn(move || (0..100).for_each(|x| tx.send(i * 1000 + x).unwrap()))
        })
        .collect();
    drop(tx);
    hs.into_iter().for_each(|h| h.join().unwrap());
    let n = rx.iter().count();
    check!(n == 300, "received {n}");
    let (stx, srx) = mpsc::sync_channel(1);
    let h = thread::spawn(move || (0..10).for_each(|x| stx.send(x).unwrap()));
    let s: i32 = srx.iter().sum();
    h.join().unwrap();
    check!(s == 45, "sync_channel sum {s}");
    Pass(format!(
        "timeout after {e} ms; 300 messages from 3 senders; sync_channel"
    ))
}

fn time_monotonic() -> Outcome {
    let a = Instant::now();
    let b = Instant::now();
    check!(b >= a, "Instant went backwards");
    let t = Instant::now();
    thread::sleep(Duration::from_millis(50));
    let e = ms(t.elapsed());
    check!((45..500).contains(&e), "sleep(50 ms) took {e} ms");
    let t = Instant::now();
    thread::sleep(Duration::from_micros(500));
    let us = t.elapsed().as_micros();
    Pass(format!("sleep 50 ms = {e} ms, sleep 0.5 ms = {us} us"))
}

fn system_time() -> Outcome {
    let now = SystemTime::now().duration_since(UNIX_EPOCH).unwrap();
    check!(
        now.as_secs() > 1_600_000_000,
        "SystemTime {}",
        now.as_secs()
    );
    check!(
        now.subsec_nanos() < 1_000_000_000,
        "nanos {}",
        now.subsec_nanos()
    );
    Pass(format!(
        "{} s since 1970, +{} ns",
        now.as_secs(),
        now.subsec_nanos()
    ))
}

const DIR: &str = "C:/stdtest";

fn fs_roundtrip() -> Outcome {
    let _ = fs::remove_dir_all(DIR);
    let sub = format!("{DIR}/sub/deep");
    if let Err(e) = fs::create_dir_all(&sub) {
        return Failed(format!("create_dir_all: {e}"));
    }
    let f = format!("{DIR}/a.txt");
    if let Err(e) = fs::write(&f, b"hello std\nline 2\n") {
        return Failed(format!("write: {e}"));
    }
    let text = fs::read_to_string(&f).unwrap_or_default();
    check!(text == "hello std\nline 2\n", "read back {text:?}");
    let md = fs::metadata(&f).unwrap();
    check!(
        md.is_file() && md.len() == 17,
        "metadata file {} len {}",
        md.is_file(),
        md.len()
    );
    let modified = md.modified().map(|t| {
        t.duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0)
    });
    let lines: Vec<String> = BufReader::new(File::open(&f).unwrap())
        .lines()
        .map(|l| l.unwrap())
        .collect();
    check!(lines == ["hello std", "line 2"], "lines {lines:?}");
    let mut names: Vec<String> = fs::read_dir(DIR)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    names.sort();
    check!(names == ["a.txt", "sub"], "read_dir {names:?}");
    let g = format!("{DIR}/b.txt");
    check!(fs::rename(&f, &g).is_ok(), "rename");
    check!(
        fs::metadata(&f).is_err() && fs::metadata(&g).is_ok(),
        "after rename"
    );
    let canon = fs::canonicalize(&g).map(|p| p.display().to_string());
    check!(fs::remove_file(&g).is_ok(), "remove_file");
    let gone = fs::metadata(&g).err().map(|e| e.kind());
    check!(
        gone == Some(std::io::ErrorKind::NotFound),
        "removed file: {gone:?}"
    );
    check!(fs::remove_dir_all(DIR).is_ok(), "remove_dir_all");
    check!(fs::metadata(DIR).is_err(), "dir still there");
    Pass(format!(
        "dirs, write/read, metadata (mtime {modified:?}), read_dir, rename, canonicalize {canon:?}, remove"
    ))
}

fn fs_positioned() -> Outcome {
    let p = "C:/stdtest.bin";
    let mut f = match File::options()
        .read(true)
        .write(true)
        .create(true)
        .truncate(true)
        .open(p)
    {
        Ok(f) => f,
        Err(e) => return Failed(format!("open: {e}")),
    };
    f.write_all(b"0123456789").unwrap();
    f.seek(SeekFrom::Start(2)).unwrap();
    let mut buf = [0u8; 4];
    f.read_at(&mut buf, 6).unwrap();
    check!(&buf == b"6789", "read_at {buf:?}");
    f.write_at(b"AB", 0).unwrap();
    let pos = f.stream_position().unwrap();
    check!(pos == 2, "position moved to {pos}");
    let mut all = String::new();
    f.seek(SeekFrom::Start(0)).unwrap();
    f.read_to_string(&mut all).unwrap();
    check!(all == "AB23456789", "content {all}");
    f.set_len(4).unwrap();
    check!(f.metadata().unwrap().len() == 4, "set_len");
    drop(f);
    fs::remove_file(p).unwrap();
    Pass("seek, read_at/write_at (pread/pwrite), set_len".into())
}

fn env_and_args() -> Outcome {
    // SAFETY: no other thread reads the environment now.
    unsafe { std::env::set_var("STDTEST_VAR", "neo") };
    let v = std::env::var("STDTEST_VAR");
    check!(v.as_deref() == Ok("neo"), "var {v:?}");
    // SAFETY: as above.
    unsafe { std::env::remove_var("STDTEST_VAR") };
    check!(
        std::env::var("STDTEST_VAR").is_err(),
        "removed var still set"
    );
    let vars = std::env::vars().count();
    let args: Vec<String> = std::env::args().collect();
    let cwd = std::env::current_dir().map(|p| p.display().to_string());
    let tmp = std::env::temp_dir().display().to_string();
    Pass(format!(
        "set/get/remove; {vars} vars; args {args:?}; cwd {cwd:?}; temp {tmp}"
    ))
}

fn hashmap_random() -> Outcome {
    let mut m = HashMap::new();
    for i in 0..10_000u32 {
        m.insert(i, i * 2);
    }
    check!(m.len() == 10_000 && m[&777] == 1554, "map");
    let (a, b) = (RandomState::new(), RandomState::new());
    let mut seeds = std::collections::HashSet::new();
    for _ in 0..8 {
        seeds.insert(RandomState::new().hash_one(42u32));
    }
    check!(
        seeds.len() > 1 || a.hash_one(1u8) != b.hash_one(1u8),
        "RandomState never varies: no randomness"
    );
    Pass(format!(
        "10000 entries; {} distinct hashes of one key over 8 RandomStates",
        seeds.len()
    ))
}

fn float_fmt() -> Outcome {
    let s = format!(
        "{:.3} {} {:e} {}",
        std::f64::consts::PI,
        0.1f32 + 0.2f32,
        1234.5f64,
        (2.0f64).sqrt()
    );
    check!(s == "3.142 0.3 1.2345e3 1.4142135623730951", "{s}");
    let p: f64 = "2.5e-3".parse().unwrap();
    check!(p == 0.0025, "parse {p}");
    let v: f32 = (0..1000).map(|i| (i as f32).sin()).sum();
    Pass(format!("{s}; sum of sin = {v:.4}"))
}

#[repr(align(4096))]
struct Page([u8; 4096]);

fn allocator() -> Outcome {
    let p = Box::new(Page([7; 4096]));
    let addr = &*p as *const Page as usize;
    check!(addr % 4096 == 0, "align(4096) box at {addr:#x}");
    let small: Vec<Box<u64>> = (0..1000).map(Box::new).collect();
    check!(
        small.iter().all(|b| (&**b as *const u64 as usize) % 8 == 0),
        "u64 not 8-aligned"
    );
    let mut v: Vec<u8> = Vec::with_capacity(16);
    for i in 0..(4 << 20) {
        v.push(i as u8);
    }
    let big = vec![1u8; 32 << 20];
    check!(
        big.iter().map(|&b| b as usize).sum::<usize>() == 32 << 20,
        "32 MiB vec"
    );
    drop(big);
    Pass(format!(
        "align 4096 at {addr:#x}, 1000 boxes, 4 MiB grown, 32 MiB vec ({} B pages ok)",
        p.0.len()
    ))
}

fn net_loopback() -> Outcome {
    let l = match std::net::TcpListener::bind("127.0.0.1:0") {
        Ok(l) => l,
        Err(e) => return Skipped(format!("no IP stack here (WB2d tests it over NeoNet): {e}")),
    };
    let addr = l.local_addr().unwrap();
    let h = thread::spawn(move || {
        let (mut s, _) = l.accept().unwrap();
        let mut b = [0u8; 4];
        s.read_exact(&mut b).unwrap();
        s.write_all(&b).unwrap();
    });
    let mut c = std::net::TcpStream::connect(addr).unwrap();
    c.write_all(b"ping").unwrap();
    let mut b = [0u8; 4];
    c.read_exact(&mut b).unwrap();
    h.join().unwrap();
    check!(&b == b"ping", "echo {b:?}");
    Pass(format!("echo over {addr}"))
}

fn main() -> i32 {
    type Check = (&'static str, fn() -> Outcome);
    let checks: &[Check] = &[
        ("thread_spawn_join", thread_spawn_join),
        ("thread_scope_builder", thread_scope_and_builder),
        ("thread_local_dtor", thread_local_dtor),
        ("park_timeout", park_timeout),
        ("mutex", mutex_contention),
        ("rwlock", rwlock),
        ("condvar", condvar_wait_timeout),
        ("once_lock_barrier", once_lock_and_barrier),
        ("mpsc", mpsc_channels),
        ("instant_sleep", time_monotonic),
        ("system_time", system_time),
        ("fs_roundtrip", fs_roundtrip),
        ("fs_positioned", fs_positioned),
        ("env_args", env_and_args),
        ("hashmap_random", hashmap_random),
        ("float_fmt", float_fmt),
        ("allocator", allocator),
        ("net_loopback", net_loopback),
    ];
    let (mut ok, mut failed, mut skipped) = (0, 0, 0);
    println!(
        "stdtest: Rust std on {} ({})",
        std::env::consts::OS,
        std::env::consts::FAMILY
    );
    for (name, f) in checks {
        println!("std {name} ...");
        match f() {
            Pass(d) => {
                ok += 1;
                println!("std {name} ... ok: {d}");
            }
            Failed(d) => {
                failed += 1;
                println!("std {name} ... FAILED: {d}");
            }
            Skipped(d) => {
                skipped += 1;
                println!("std {name} ... skipped: {d}");
            }
        }
    }
    println!("stdtest: {ok} ok, {failed} failed, {skipped} skipped");
    if failed == 0 { 0 } else { 1 }
}
