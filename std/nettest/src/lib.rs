// SPDX-License-Identifier: Apache-2.0
//! NETTEST.EXE (docs browser-std.md, WB2d): Tokio's current_thread runtime on NeoSymbian (mio's poll selector over
//! P.I.P.S.), then a plain HTTP fetch with std and with Tokio over NeoNet. Prints `net <name> ... ok|FAILED|skipped`.

use std::io::{Read, Write};
use std::net::{TcpStream, ToSocketAddrs};
use std::time::{Duration, Instant};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

e32::main!(main);

const HOST: &str = "example.com";

enum Outcome {
    Pass(String),
    Failed(String),
    Skipped(String),
}
use Outcome::{Failed, Pass, Skipped};

fn runtime() -> tokio::runtime::Runtime {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("tokio runtime")
}

/// Timers, tasks, a channel and a wake from another OS thread (spawn_blocking): mio's poll + pipe waker.
fn tokio_runtime() -> Outcome {
    let rt = runtime();
    rt.block_on(async {
        let t = Instant::now();
        tokio::time::sleep(Duration::from_millis(100)).await;
        let slept = t.elapsed().as_millis();
        if !(80..1000).contains(&slept) {
            return Failed(format!("sleep(100 ms) took {slept} ms"));
        }
        let (tx, mut rx) = tokio::sync::mpsc::channel::<u32>(4);
        let producer = tokio::spawn(async move {
            for i in 0..10 {
                tx.send(i).await.unwrap();
            }
        });
        let mut sum = 0;
        while let Some(v) = rx.recv().await {
            sum += v;
        }
        producer.await.unwrap();
        let from_thread = tokio::task::spawn_blocking(|| {
            std::thread::sleep(Duration::from_millis(50));
            7
        })
        .await
        .unwrap();
        let timeout =
            tokio::time::timeout(Duration::from_millis(50), std::future::pending::<()>()).await;
        if sum != 45 || from_thread != 7 || timeout.is_ok() {
            return Failed(format!(
                "sum {sum}, blocking {from_thread}, timeout {timeout:?}"
            ));
        }
        Pass(format!(
            "sleep {slept} ms, mpsc, spawn_blocking wake, timeout"
        ))
    })
}

fn tokio_tcp_loopback() -> Outcome {
    let rt = runtime();
    rt.block_on(async {
        let l = match tokio::net::TcpListener::bind("127.0.0.1:0").await {
            Ok(l) => l,
            Err(e) => return Skipped(format!("bind: {e}")),
        };
        let addr = l.local_addr().unwrap();
        let server = tokio::spawn(async move {
            let (mut s, _) = l.accept().await.unwrap();
            let mut b = [0u8; 4];
            s.read_exact(&mut b).await.unwrap();
            s.write_all(&b).await.unwrap();
        });
        let mut c = match tokio::net::TcpStream::connect(addr).await {
            Ok(c) => c,
            Err(e) => return Failed(format!("connect {addr}: {e}")),
        };
        c.write_all(b"ping").await.unwrap();
        let mut b = [0u8; 4];
        c.read_exact(&mut b).await.unwrap();
        server.await.unwrap();
        if &b != b"ping" {
            return Failed(format!("echo {b:?}"));
        }
        Pass(format!("echo over {addr}"))
    })
}

fn status_line(resp: &[u8]) -> String {
    String::from_utf8_lossy(resp)
        .lines()
        .next()
        .unwrap_or("")
        .to_string()
}

fn std_http() -> Outcome {
    let t = Instant::now();
    let addrs: Vec<_> = match (HOST, 80).to_socket_addrs() {
        Ok(a) => a.collect(),
        Err(e) => return Skipped(format!("DNS {HOST}: {e} (no network?)")),
    };
    let dns = t.elapsed().as_millis();
    let mut s = match addrs
        .iter()
        .find_map(|a| TcpStream::connect_timeout(a, Duration::from_secs(10)).ok())
    {
        Some(s) => s,
        None => return Failed(format!("connect to {addrs:?} failed")),
    };
    s.set_read_timeout(Some(Duration::from_secs(10))).unwrap();
    s.write_all(format!("GET / HTTP/1.0\r\nHost: {HOST}\r\n\r\n").as_bytes())
        .unwrap();
    let mut resp = Vec::new();
    let _ = s.read_to_end(&mut resp);
    let line = status_line(&resp);
    if !line.starts_with("HTTP/1.") {
        return Failed(format!("response {line:?} ({} bytes)", resp.len()));
    }
    Pass(format!(
        "{line:?}, {} bytes, DNS {dns} ms -> {}, total {} ms",
        resp.len(),
        addrs[0],
        t.elapsed().as_millis()
    ))
}

fn tokio_http() -> Outcome {
    let rt = runtime();
    rt.block_on(async {
        let t = Instant::now();
        let mut s = match tokio::time::timeout(
            Duration::from_secs(15),
            tokio::net::TcpStream::connect((HOST, 80)),
        )
        .await
        {
            Ok(Ok(s)) => s,
            Ok(Err(e)) => return Skipped(format!("connect {HOST}: {e} (no network?)")),
            Err(_) => return Failed("connect timed out".into()),
        };
        s.write_all(format!("GET / HTTP/1.0\r\nHost: {HOST}\r\n\r\n").as_bytes())
            .await
            .unwrap();
        let mut resp = Vec::new();
        match tokio::time::timeout(Duration::from_secs(15), s.read_to_end(&mut resp)).await {
            Ok(Ok(_)) => {}
            other => return Failed(format!("read: {other:?}")),
        }
        let line = status_line(&resp);
        if !line.starts_with("HTTP/1.") {
            return Failed(format!("response {line:?}"));
        }
        Pass(format!(
            "{line:?}, {} bytes in {} ms",
            resp.len(),
            t.elapsed().as_millis()
        ))
    })
}

/// Raw P.I.P.S. socket steps with errno, to see which one Tokio's and std's non-blocking connect trip on.
fn socket_diag() -> Outcome {
    use std::fmt::Write as _;
    let mut out = String::new();
    let errno = || std::io::Error::last_os_error().raw_os_error().unwrap_or(0);
    let addr = match (HOST, 80)
        .to_socket_addrs()
        .ok()
        .and_then(|mut a| a.find(|a| a.is_ipv4()))
    {
        Some(std::net::SocketAddr::V4(a)) => a,
        _ => return Skipped("no IPv4 address for the host".into()),
    };
    let sin = libc::sockaddr_in {
        sin_family: libc::AF_INET as libc::sa_family_t,
        sin_port: addr.port().to_be(),
        sin_addr: libc::in_addr {
            s_addr: u32::from_ne_bytes(addr.ip().octets()),
        },
        sin_zero: [0; 20],
    };
    let len = std::mem::size_of::<libc::sockaddr_in>() as libc::socklen_t;
    // SAFETY: plain libc calls on a socket this function owns, with valid pointers and lengths.
    unsafe {
        // blocking connect
        let fd = libc::socket(libc::AF_INET, libc::SOCK_STREAM, 0);
        let r = libc::connect(fd, &sin as *const _ as *const libc::sockaddr, len);
        let _ = write!(
            out,
            "blocking: socket {fd} connect {r}/{}; ",
            if r < 0 { errno() } else { 0 }
        );
        libc::close(fd);
        // non-blocking connect, as mio does it
        let fd = libc::socket(libc::AF_INET, libc::SOCK_STREAM, 0);
        let fl = libc::fcntl(fd, libc::F_GETFL);
        let r1 = libc::fcntl(fd, libc::F_SETFL, fl | libc::O_NONBLOCK);
        let e1 = if r1 < 0 { errno() } else { 0 };
        let r2 = libc::connect(fd, &sin as *const _ as *const libc::sockaddr, len);
        let e2 = if r2 < 0 { errno() } else { 0 };
        let mut p = libc::pollfd {
            fd,
            events: libc::POLLOUT,
            revents: 0,
        };
        let r3 = libc::poll(&mut p, 1, 5000);
        let mut err: libc::c_int = -1;
        let mut el = std::mem::size_of::<libc::c_int>() as libc::socklen_t;
        let r4 = libc::getsockopt(
            fd,
            libc::SOL_SOCKET,
            libc::SO_ERROR,
            &mut err as *mut _ as *mut libc::c_void,
            &mut el,
        );
        let e4 = if r4 < 0 { errno() } else { 0 };
        let mut peer: libc::sockaddr_in = std::mem::zeroed();
        let mut pl = len;
        let r5 = libc::getpeername(fd, &mut peer as *mut _ as *mut libc::sockaddr, &mut pl);
        let e5 = if r5 < 0 { errno() } else { 0 };
        let _ = write!(
            out,
            "nonblocking: F_SETFL {r1}/{e1} connect {r2}/{e2} poll {r3} rev {:#x} SO_ERROR {r4}/{e4} = {err} getpeername {r5}/{e5}; ",
            p.revents
        );
        libc::close(fd);
        // FIONBIO (std's set_nonblocking)
        let fd = libc::socket(libc::AF_INET, libc::SOCK_STREAM, 0);
        let mut one: libc::c_int = 1;
        let r6 = libc::ioctl(fd, libc::FIONBIO, &mut one);
        let _ = write!(out, "FIONBIO {r6}/{}; ", if r6 < 0 { errno() } else { 0 });
        // bind + listen
        let any = libc::sockaddr_in {
            sin_family: libc::AF_INET as libc::sa_family_t,
            sin_port: 0,
            sin_addr: libc::in_addr { s_addr: 0 },
            sin_zero: [0; 20],
        };
        let r7 = libc::bind(fd, &any as *const _ as *const libc::sockaddr, len);
        let e7 = if r7 < 0 { errno() } else { 0 };
        let r8 = libc::listen(fd, 5);
        let _ = write!(
            out,
            "bind {r7}/{e7} listen {r8}/{}",
            if r8 < 0 { errno() } else { 0 }
        );
        libc::close(fd);
    }
    Pass(out)
}

fn main() -> i32 {
    type Check = (&'static str, fn() -> Outcome);
    let checks: &[Check] = &[
        ("socket_diag", socket_diag),
        ("tokio_runtime", tokio_runtime),
        ("tokio_tcp_loopback", tokio_tcp_loopback),
        ("std_http", std_http),
        ("tokio_http", tokio_http),
    ];
    let (mut ok, mut failed, mut skipped) = (0, 0, 0);
    for (name, f) in checks {
        println!("net {name} ...");
        match f() {
            Pass(d) => {
                ok += 1;
                println!("net {name} ... ok: {d}");
            }
            Failed(d) => {
                failed += 1;
                println!("net {name} ... FAILED: {d}");
            }
            Skipped(d) => {
                skipped += 1;
                println!("net {name} ... skipped: {d}");
            }
        }
    }
    println!("nettest: {ok} ok, {failed} failed, {skipped} skipped");
    if failed == 0 { 0 } else { 1 }
}
