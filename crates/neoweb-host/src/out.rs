// SPDX-License-Identifier: Apache-2.0
//! Run output: `steps.csv`, PNGs, memory reports, and an optional view on the guest framebuffer (VNC).

use std::fs::{self, File, OpenOptions};
use std::io::{self, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};

use servo::RgbaImage;

pub struct Out {
    dir: PathBuf,
    csv: File,
}

pub struct Row<'a> {
    pub step: usize,
    pub kind: &'a str,
    pub arg: &'a str,
    pub t_start_ms: u64,
    pub t_end_ms: u64,
    pub ok: bool,
    pub frames: u64,
    pub max_frame_gap_ms: u64,
    pub note: &'a str,
}

impl Out {
    pub fn new(dir: &Path) -> io::Result<Self> {
        fs::create_dir_all(dir)?;
        let mut csv = File::create(dir.join("steps.csv"))?;
        writeln!(
            csv,
            "step,kind,arg,t_start_ms,t_end_ms,ok,frames,max_frame_gap_ms,note"
        )?;
        Ok(Self {
            dir: dir.to_path_buf(),
            csv,
        })
    }

    pub fn path(&self, file: &str) -> PathBuf {
        self.dir.join(file)
    }

    pub fn row(&mut self, r: &Row) {
        let q = |s: &str| format!("\"{}\"", s.replace('"', "\"\""));
        let line = format!(
            "{},{},{},{},{},{},{},{},{}",
            r.step,
            r.kind,
            q(r.arg),
            r.t_start_ms,
            r.t_end_ms,
            r.ok as u8,
            r.frames,
            r.max_frame_gap_ms,
            q(r.note)
        );
        eprintln!("[{:>7}] {line}", r.t_end_ms);
        let _ = writeln!(self.csv, "{line}");
        let _ = self.csv.flush();
    }

    pub fn png(&self, name: &str, image: &RgbaImage) -> Result<(), String> {
        image
            .save(self.path(&format!("{name}.png")))
            .map_err(|e| e.to_string())
    }
}

/// The guest's framebuffer (`/dev/fb0`, 32 bpp XRGB), so a run can be watched over VNC.
pub struct Framebuffer {
    file: File,
    width: usize,
    height: usize,
    stride: usize,
}

impl Framebuffer {
    pub fn open(dev: &Path) -> io::Result<Self> {
        let sys = Path::new("/sys/class/graphics").join(dev.file_name().unwrap_or_default());
        let read = |f: &str| fs::read_to_string(sys.join(f)).map(|s| s.trim().to_string());
        let size = read("virtual_size")?;
        let (w, h) = size
            .split_once(',')
            .ok_or_else(|| io::Error::other("bad virtual_size"))?;
        let parse = |s: &str| s.parse::<usize>().map_err(io::Error::other);
        let (width, height) = (parse(w)?, parse(h)?);
        let stride = parse(&read("stride")?)?;
        let file = OpenOptions::new().write(true).open(dev)?;
        Ok(Self {
            file,
            width,
            height,
            stride,
        })
    }

    pub fn blit(&mut self, image: &RgbaImage) -> io::Result<()> {
        let w = self.width.min(image.width() as usize);
        let h = self.height.min(image.height() as usize);
        let mut line = vec![0u8; w * 4];
        for y in 0..h {
            for x in 0..w {
                let p = image.get_pixel(x as u32, y as u32).0;
                line[x * 4..x * 4 + 4].copy_from_slice(&[p[2], p[1], p[0], 0xff]); // RGBA -> BGRX
            }
            self.file.seek(SeekFrom::Start((y * self.stride) as u64))?;
            self.file.write_all(&line)?;
        }
        Ok(())
    }
}
