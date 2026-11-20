// SPDX-License-Identifier: Apache-2.0
//! NEOGET.EXE: `neoget http://HOST[:PORT]/PATH OUT` fetches one file over Wi-Fi (plain HTTP/1.0) into OUT, e.g.
//! the browser engine from atlas's LAN file drop (dev-phonedrop, http://192.168.3.20:8099/) into
//! `C:\sys\bin\neowebe.exe`. A ROM EXE, so it has the capabilities to write \sys\bin. It writes OUT.part and
//! renames it only when the body has the announced length.

use std::io::{BufWriter, Read, Write};
use std::net::{TcpStream, ToSocketAddrs};
use std::time::{Duration, Instant};

#[cfg(target_os = "symbian")]
e32::main!(main);

/// `http://host[:port]/path` -> (host, port, path).
pub fn parse_url(url: &str) -> Result<(String, u16, String), String> {
    let rest = url.strip_prefix("http://").ok_or("only http:// URLs")?;
    let (hostport, path) = rest.split_at(rest.find('/').unwrap_or(rest.len()));
    let (host, port) = match hostport.rsplit_once(':') {
        Some((h, p)) => (h, p.parse().map_err(|_| format!("bad port {p:?}"))?),
        None => (hostport, 80),
    };
    if host.is_empty() {
        return Err("no host".into());
    }
    Ok((
        host.into(),
        port,
        if path.is_empty() {
            "/".into()
        } else {
            path.into()
        },
    ))
}

/// The response head (without the blank line) -> (status, Content-Length).
pub fn parse_head(head: &str) -> Result<(u16, Option<u64>), String> {
    let mut lines = head.split("\r\n");
    let status = lines
        .next()
        .and_then(|l| l.split_whitespace().nth(1))
        .and_then(|s| s.parse().ok())
        .ok_or("no HTTP status line")?;
    let len = lines
        .filter_map(|l| l.split_once(':'))
        .find(|(k, _)| k.trim().eq_ignore_ascii_case("content-length"))
        .map(|(_, v)| {
            v.trim()
                .parse()
                .map_err(|_| format!("bad Content-Length {v:?}"))
        })
        .transpose()?;
    Ok((status, len))
}

fn fetch(url: &str, out: &str) -> Result<(u64, Duration), String> {
    let (host, port, path) = parse_url(url)?;
    let started = Instant::now();
    let addrs = (host.as_str(), port)
        .to_socket_addrs()
        .map_err(|e| format!("resolve {host}: {e}"))?;
    let mut s = addrs
        .into_iter()
        .find_map(|a| TcpStream::connect_timeout(&a, Duration::from_secs(10)).ok())
        .ok_or(format!("connect {host}:{port} failed"))?;
    s.set_read_timeout(Some(Duration::from_secs(30))).ok();
    s.write_all(
        format!("GET {path} HTTP/1.0\r\nHost: {host}\r\nUser-Agent: neoget\r\n\r\n").as_bytes(),
    )
    .map_err(|e| format!("send: {e}"))?;

    // the head, then whatever of the body came with it
    let mut buf = vec![0u8; 64 * 1024];
    let mut have = Vec::new();
    let split = loop {
        let n = s.read(&mut buf).map_err(|e| format!("read: {e}"))?;
        if n == 0 {
            return Err("connection closed before the response head".into());
        }
        have.extend_from_slice(&buf[..n]);
        if let Some(i) = have.windows(4).position(|w| w == b"\r\n\r\n") {
            break i;
        }
        if have.len() > 64 * 1024 {
            return Err("response head over 64 KB".into());
        }
    };
    let (status, len) = parse_head(&String::from_utf8_lossy(&have[..split]))?;
    if status != 200 {
        return Err(format!("HTTP {status}"));
    }

    let part = format!("{out}.part");
    let file = std::fs::File::create(&part).map_err(|e| format!("create {part}: {e}"))?;
    let mut w = BufWriter::with_capacity(64 * 1024, file); // 64 KB RFile writes (1 KB ones cost ~13 ms each)
    let mut got = (have.len() - split - 4) as u64;
    w.write_all(&have[split + 4..])
        .map_err(|e| format!("write: {e}"))?;
    let mut shown = Instant::now();
    loop {
        let n = s
            .read(&mut buf)
            .map_err(|e| format!("read after {got} bytes: {e}"))?;
        if n == 0 {
            break;
        }
        w.write_all(&buf[..n]).map_err(|e| format!("write: {e}"))?;
        got += n as u64;
        if shown.elapsed() >= Duration::from_secs(2) {
            shown = Instant::now();
            println!("neoget: {:.1} MB", got as f64 / 1e6);
        }
    }
    w.flush().map_err(|e| format!("write: {e}"))?;
    drop(w);
    if len.is_some_and(|l| l != got) {
        let _ = std::fs::remove_file(&part);
        return Err(format!("got {got} of {} bytes", len.unwrap()));
    }
    let _ = std::fs::remove_file(out); // rename onto an existing file fails on Symbian
    std::fs::rename(&part, out).map_err(|e| format!("rename {part} -> {out}: {e}"))?;
    Ok((got, started.elapsed()))
}

pub fn main() -> i32 {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let [url, out] = args.as_slice() else {
        println!("usage: neoget http://HOST[:PORT]/PATH OUT");
        return 2;
    };
    match fetch(url, out) {
        Ok((n, t)) => {
            let ms = t.as_millis().max(1);
            println!(
                "neoget: {n} bytes in {ms} ms ({} KB/s) -> {out}",
                n as u128 * 1000 / ms / 1000
            );
            0
        }
        Err(e) => {
            println!("neoget: {e}");
            1
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn urls() {
        assert_eq!(
            parse_url("http://192.168.3.20:8099/neowebe.exe").unwrap(),
            ("192.168.3.20".into(), 8099, "/neowebe.exe".into())
        );
        assert_eq!(
            parse_url("http://example.com").unwrap(),
            ("example.com".into(), 80, "/".into())
        );
        assert!(parse_url("https://example.com/").is_err());
        assert!(parse_url("http://:80/").is_err());
        assert!(parse_url("http://h:x/").is_err());
    }

    #[test]
    fn heads() {
        assert_eq!(
            parse_head("HTTP/1.1 200 OK\r\nContent-Type: x\r\ncontent-length: 42").unwrap(),
            (200, Some(42))
        );
        assert_eq!(parse_head("HTTP/1.0 404 Not Found").unwrap(), (404, None));
        assert!(parse_head("garbage").is_err());
        assert!(parse_head("HTTP/1.1 200 OK\r\nContent-Length: nope").is_err());
    }
}
