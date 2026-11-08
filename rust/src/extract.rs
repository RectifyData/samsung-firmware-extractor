use crate::walker::{walk, Visitor};
use anyhow::Result;
use std::collections::HashSet;
use std::fs::{self, File};
use std::io::{self, BufWriter, Read, Seek, Write};
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};

/// Progress and output hooks. The UI implements this to copy each finished image out.
pub trait Events {
    fn log(&mut self, message: &str);
    /// Called with a finished raw image. The file at `path` is deleted once this returns.
    fn image(&mut self, name: &str, path: &Path) -> Result<()>;
}

/// Decodes every selected `.lz4` image in the firmware into a raw image and reports it via `events`.
pub fn extract_images<R: Read + Seek>(
    input: R,
    name: &str,
    selected: &HashSet<String>,
    work: &Path,
    events: &mut dyn Events,
    cancel: &AtomicBool,
) -> Result<()> {
    fs::create_dir_all(work)?;
    let mut extractor = Extractor { selected, work, events, cancel };
    walk(input, name, &mut extractor, cancel)
}

/// "dir/system.img.lz4" -> "system.img"
pub fn base_name(path: &str) -> String {
    let name = path.rsplit('/').next().unwrap_or(path);
    if name.to_ascii_lowercase().ends_with(".lz4") {
        name[..name.len() - 4].to_owned()
    } else {
        name.to_owned()
    }
}

struct Extractor<'a> {
    selected: &'a HashSet<String>,
    work: &'a Path,
    events: &'a mut dyn Events,
    cancel: &'a AtomicBool,
}

impl Visitor for Extractor<'_> {
    fn wants(&self, path: &str) -> bool {
        self.selected.contains(path)
    }

    fn visit(&mut self, path: &str, _size: Option<u64>, data: &mut dyn Read) -> Result<()> {
        let out_name = base_name(path);
        let tmp = self.work.join(&out_name);
        self.events.log(&format!("Decompressing {path}"));
        let result = self.decode_one(&out_name, &tmp, data);
        let _ = fs::remove_file(&tmp);
        result
    }
}

impl Extractor<'_> {
    fn decode_one(&mut self, out_name: &str, tmp: &Path, data: &mut dyn Read) -> Result<()> {
        let mut guarded = Guarded { inner: data, cancel: self.cancel };
        let bytes = {
            let mut decoder = crate::frame::FrameDecoder::new(&mut guarded);
            let mut out = BufWriter::with_capacity(1 << 20, File::create(tmp)?);
            let n = io::copy(&mut decoder, &mut out)?;
            out.flush()?;
            n
        };
        self.events.log(&format!("  -> {out_name} ({:.1} MiB)", bytes as f64 / 1048576.0));
        if crate::sparse::convert_if_sparse(tmp)? {
            self.events.log("  sparse image converted to raw");
        }
        self.events.image(out_name, tmp)
    }
}

/// Makes reads fail once cancellation is requested, so long decodes stop promptly.
struct Guarded<'a> {
    inner: &'a mut dyn Read,
    cancel: &'a AtomicBool,
}

impl Read for Guarded<'_> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        // io::copy retries on Interrupted, so use a different kind here.
        if self.cancel.load(Ordering::Relaxed) {
            return Err(io::Error::new(io::ErrorKind::Other, "cancelled"));
        }
        self.inner.read(buf)
    }
}
