// SPDX-License-Identifier: Apache-2.0
//! NEOWEBE's downloads (browser WB5d): one thread each, HTTP(S) through ureq over the engine's rustls provider, into
//! `C:/Data/Downloads/<name>.part`, renamed when complete. Pause stops the transfer and keeps the part; Resume asks
//! for the rest (`Range`); Cancel deletes the part. Progress goes to NEOWEB as `Event::Download` about 4 times a
//! second.

use std::collections::HashMap;
use std::io::{Read, Write};
use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use neoweb_proto::{DownloadOp, DownloadState, Event};

pub const DIR: &str = "C:/Data/Downloads";
const RUN: u8 = 0;
const PAUSE: u8 = 1;
const CANCEL: u8 = 2;

/// A sink for events (the engine's queue to NEOWEB).
pub type Post = Arc<dyn Fn(Event) + Send + Sync>;

#[derive(Default)]
pub struct Downloads {
    next: u32,
    running: HashMap<u32, Arc<Job>>,
}

struct Job {
    id: u32,
    url: String,
    name: String,
    want: AtomicU8,
    /// the thread is gone (paused, done, failed or cancelled): Resume starts another
    idle: Mutex<bool>,
}

impl Downloads {
    pub fn start(&mut self, url: &str, post: Post) {
        // at most 4 at once (review #7)
        if self.running.values().filter(|j| !*j.idle.lock().unwrap()).count() >= 4 {
            println!("neowebe: download {url}: 4 running already");
            return;
        }
        self.next += 1;
        let name = unique_name(&file_name(url));
        // the part is created now, so a second download of the same name picks another (review #8)
        let _ = std::fs::create_dir_all(DIR);
        let _ = std::fs::OpenOptions::new().create_new(true).write(true).open(part(&name));
        let job = Arc::new(Job {
            id: self.next,
            url: url.into(),
            name,
            want: AtomicU8::new(RUN),
            idle: Mutex::new(false),
        });
        self.running.insert(job.id, job.clone());
        spawn(job, post);
    }

    pub fn op(&mut self, id: u32, op: DownloadOp, post: Post) {
        let Some(job) = self.running.get(&id).cloned() else {
            return;
        };
        match op {
            DownloadOp::Pause => job.want.store(PAUSE, Ordering::Release),
            DownloadOp::Cancel => {
                job.want.store(CANCEL, Ordering::Release);
                if *job.idle.lock().unwrap() {
                    // paused: nothing runs to see it
                    let _ = std::fs::remove_file(part(&job.name));
                    post(event(&job, 0, 0, DownloadState::Cancelled));
                }
                self.running.remove(&id);
            }
            DownloadOp::Resume => {
                let mut idle = job.idle.lock().unwrap();
                if *idle && job.want.load(Ordering::Acquire) == PAUSE {
                    *idle = false;
                    job.want.store(RUN, Ordering::Release);
                    drop(idle);
                    spawn(job, post);
                }
            }
        }
    }
}

fn part(name: &str) -> String {
    format!("{DIR}/{name}.part")
}

fn event(job: &Job, received: u64, total: u64, state: DownloadState) -> Event {
    Event::Download {
        id: job.id,
        name: job.name.clone(),
        url: job.url.clone(),
        path: format!("{DIR}/{}", job.name),
        received,
        total,
        state,
    }
}

/// The file name a URL suggests: its path's last segment, percent-decoded, with characters FAT can't hold replaced.
pub fn file_name(url: &str) -> String {
    let path = url.split(['?', '#']).next().unwrap_or("");
    let last = path.rsplit('/').next().unwrap_or("");
    let mut bytes = Vec::new();
    let b = last.as_bytes();
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%'
            && i + 2 < b.len()
            && let Some(v) = std::str::from_utf8(&b[i + 1..i + 3])
                .ok()
                .and_then(|h| u8::from_str_radix(h, 16).ok())
        {
            bytes.push(v);
            i += 3;
        } else {
            bytes.push(b[i]);
            i += 1;
        }
    }
    let name: String = String::from_utf8_lossy(&bytes)
        .chars()
        .map(|c| if "<>:\"/\\|?*".contains(c) || c.is_control() { '_' } else { c })
        .collect();
    if name.is_empty() || name == "." || name == ".." {
        "download".into()
    } else {
        name
    }
}

/// `name`, or `name (2)`… when a file (or a part) of that name is in the folder already.
fn unique_name(name: &str) -> String {
    let taken = |n: &str| {
        std::path::Path::new(&format!("{DIR}/{n}")).exists() || std::path::Path::new(&part(n)).exists()
    };
    if !taken(name) {
        return name.into();
    }
    let (stem, ext) = match name.rfind('.') {
        Some(d) if d > 0 => (&name[..d], &name[d..]),
        _ => (name, ""),
    };
    (2..1000)
        .map(|n| format!("{stem} ({n}){ext}"))
        .find(|n| !taken(n))
        .unwrap_or_else(|| name.into())
}

fn spawn(job: Arc<Job>, post: Post) {
    let _ = std::thread::Builder::new()
        .name("neoweb-download".into())
        .stack_size(256 * 1024)
        .spawn(move || {
            let state = match run(&job, &post) {
                Ok(s) => s,
                Err(e) => {
                    println!("neowebe: download {} failed: {e}", job.url);
                    DownloadState::Failed
                }
            };
            let have = std::fs::metadata(part(&job.name)).map_or(0, |m| m.len());
            // a failed or paused download keeps its part, for Resume
            if state == DownloadState::Cancelled {
                let _ = std::fs::remove_file(part(&job.name));
            }
            *job.idle.lock().unwrap() = true;
            if state != DownloadState::Done {
                post(event(&job, have, 0, state));
            }
        });
}

fn run(job: &Job, post: &Post) -> Result<DownloadState, String> {
    std::fs::create_dir_all(DIR).map_err(|e| format!("{DIR}: {e}"))?;
    let part_path = part(&job.name);
    let have = std::fs::metadata(&part_path).map_or(0, |m| m.len());
    // P.I.P.S. sockets take no TCP_NODELAY or receive/send timeouts (ENOPROTOOPT, os error 42): ureq sets neither
    let agent = ureq::Agent::config_builder()
        .no_delay(false)
        .max_idle_connections(0)
        .build()
        .new_agent();
    let mut req = agent.get(&job.url);
    if have > 0 {
        req = req.header("Range", &format!("bytes={have}-"));
    }
    let resp = req.call().map_err(|e| e.to_string())?;
    // a 206 must continue exactly where the part ends; anything else starts the file again (review #9)
    let resumed = resp.status() == 206
        && resp
            .headers()
            .get("content-range")
            .and_then(|v| v.to_str().ok())
            .is_some_and(|r| r.starts_with(&format!("bytes {have}-")));
    if have > 0 && !resumed && resp.status() == 206 {
        return Err("the server sent another range".into());
    }
    let len: u64 = resp
        .headers()
        .get("content-length")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.parse().ok())
        .unwrap_or(0);
    let (mut received, total) = if resumed { (have, have + len) } else { (0, len) };
    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .write(true)
        .append(resumed)
        .truncate(!resumed)
        .open(&part_path)
        .map_err(|e| format!("{part_path}: {e}"))?;
    let mut body = resp.into_body().into_reader();
    let mut buf = vec![0u8; 64 * 1024];
    let mut last = Instant::now();
    post(event(job, received, total, DownloadState::Downloading));
    loop {
        match job.want.load(Ordering::Acquire) {
            PAUSE => return Ok(DownloadState::Paused),
            CANCEL => return Ok(DownloadState::Cancelled),
            _ => {}
        }
        let n = body.read(&mut buf).map_err(|e| e.to_string())?;
        if n == 0 {
            break;
        }
        file.write_all(&buf[..n]).map_err(|e| e.to_string())?;
        received += n as u64;
        if last.elapsed() >= Duration::from_millis(250) {
            last = Instant::now();
            post(event(job, received, total, DownloadState::Downloading));
        }
    }
    file.flush().map_err(|e| e.to_string())?;
    drop(file);
    if total > 0 && received != total {
        return Err(format!("{received} of {total} bytes"));
    }
    std::fs::rename(&part_path, format!("{DIR}/{}", job.name)).map_err(|e| e.to_string())?;
    post(event(job, received, received, DownloadState::Done));
    Ok(DownloadState::Done)
}

#[cfg(test)]
mod tests {
    use super::file_name;

    #[test]
    fn names_from_urls() {
        assert_eq!(file_name("https://a.org/docs/EKA2%20guide.pdf?x=1"), "EKA2 guide.pdf");
        assert_eq!(file_name("https://a.org/"), "download");
        assert_eq!(file_name("https://a.org/a%3Ab.txt"), "a_b.txt");
    }
}
