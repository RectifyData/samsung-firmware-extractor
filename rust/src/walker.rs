use anyhow::{bail, Context, Result};
use std::io::{Read, Seek, SeekFrom};
use std::sync::atomic::{AtomicBool, Ordering};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImageInfo {
    /// Path inside the firmware, e.g. `AP_X.tar.md5/system.img.lz4`.
    pub path: String,
    /// Compressed size when the container knows it.
    pub size: Option<u64>,
}

/// Receives every `.lz4` entry the walker finds.
pub trait Visitor {
    /// Return false to skip the entry without reading it.
    fn wants(&self, path: &str) -> bool;
    /// `data` is the compressed image; reading it is optional.
    fn visit(&mut self, path: &str, size: Option<u64>, data: &mut dyn Read) -> Result<()>;
}

/// Lists all `.lz4` images in a firmware without decompressing them.
pub fn list_images<R: Read + Seek>(input: R, name: &str, cancel: &AtomicBool) -> Result<Vec<ImageInfo>> {
    let mut lister = Lister::default();
    walk(input, name, &mut lister, cancel)?;
    Ok(lister.found)
}

#[derive(Default)]
struct Lister {
    found: Vec<ImageInfo>,
}

impl Visitor for Lister {
    fn wants(&self, _path: &str) -> bool {
        true
    }

    fn visit(&mut self, path: &str, size: Option<u64>, _data: &mut dyn Read) -> Result<()> {
        self.found.push(ImageInfo { path: path.to_owned(), size });
        Ok(())
    }
}

/// Walks a firmware file. `name` decides the container type (.zip, .lz4, otherwise tar).
pub fn walk<R: Read + Seek>(mut input: R, name: &str, v: &mut dyn Visitor, cancel: &AtomicBool) -> Result<()> {
    if name.to_ascii_lowercase().ends_with(".zip") {
        // Zip needs random access for its central directory, so it is handled here.
        let mut zip = zip::ZipArchive::new(input).context("not a valid zip archive")?;
        for i in 0..zip.len() {
            check_cancel(cancel)?;
            let mut entry = zip.by_index(i)?;
            if entry.is_dir() {
                continue;
            }
            let entry_name = entry.name().to_owned();
            let size = Some(entry.size());
            walk_entry(&entry_name, size, &mut entry, v, cancel)?;
        }
        Ok(())
    } else if is_lz4(name) {
        if v.wants(name) {
            let size = input.seek(SeekFrom::End(0))?;
            input.seek(SeekFrom::Start(0))?;
            v.visit(name, Some(size), &mut input)?;
        }
        Ok(())
    } else {
        walk_tar(&mut input, "", v, cancel)
    }
}

fn walk_entry(name: &str, size: Option<u64>, data: &mut dyn Read, v: &mut dyn Visitor, cancel: &AtomicBool) -> Result<()> {
    if is_lz4(name) {
        if v.wants(name) {
            v.visit(name, size, data)?;
        }
    } else if is_tar(name) {
        walk_tar(data, &format!("{name}/"), v, cancel)?;
    }
    Ok(())
}

/// Tar readers stop at the end-of-archive marker, so the trailing "MD5" of *.tar.md5 is never read.
fn walk_tar(reader: &mut dyn Read, prefix: &str, v: &mut dyn Visitor, cancel: &AtomicBool) -> Result<()> {
    let mut archive = tar::Archive::new(reader);
    for entry in archive.entries()? {
        check_cancel(cancel)?;
        let mut entry = entry?;
        if !entry.header().entry_type().is_file() {
            continue;
        }
        let inner = entry.path()?.to_string_lossy().into_owned();
        let full = format!("{prefix}{inner}");
        let size = entry.header().size().ok();
        if is_lz4(&inner) {
            if v.wants(&full) {
                v.visit(&full, size, &mut entry)?;
            }
        } else if is_tar(&inner) {
            walk_tar(&mut entry, &format!("{full}/"), v, cancel)?;
        }
    }
    Ok(())
}

pub(crate) fn is_lz4(name: &str) -> bool {
    name.to_ascii_lowercase().ends_with(".lz4")
}

pub(crate) fn is_tar(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    lower.ends_with(".tar") || lower.ends_with(".tar.md5")
}

pub(crate) fn check_cancel(cancel: &AtomicBool) -> Result<()> {
    if cancel.load(Ordering::Relaxed) {
        bail!("cancelled");
    }
    Ok(())
}
