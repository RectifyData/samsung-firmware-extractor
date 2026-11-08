//! Android sparse image ("simg") to raw conversion.

use anyhow::{bail, Result};
use std::fs::{self, File};
use std::io::{self, BufReader, BufWriter, Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};

const MAGIC: u32 = 0xED26_FF3A;
const CHUNK_RAW: u16 = 0xCAC1;
const CHUNK_FILL: u16 = 0xCAC2;
const CHUNK_DONT_CARE: u16 = 0xCAC3;
const CHUNK_CRC: u16 = 0xCAC4;
const BUF: usize = 1 << 20; // multiple of 4, used for copies and fill patterns

/// Converts `path` in place when it is a sparse image. Returns true if it was converted.
pub fn convert_if_sparse(path: &Path) -> Result<bool> {
    let raw_path = PathBuf::from(format!("{}.raw", path.display()));
    if !write_raw(path, &raw_path)? {
        return Ok(false);
    }
    // The source is closed by now. Windows refuses to rename over an open file.
    fs::rename(&raw_path, path)?;
    Ok(true)
}

/// Writes the raw form of a sparse image to `raw_path`. Returns false (and writes nothing) if
/// `path` is not sparse.
fn write_raw(path: &Path, raw_path: &Path) -> Result<bool> {
    let mut src = BufReader::with_capacity(BUF, File::open(path)?);
    let mut hdr = [0u8; 28];
    if src.read_exact(&mut hdr).is_err() {
        return Ok(false);
    }
    if u32::from_le_bytes(hdr[0..4].try_into()?) != MAGIC {
        return Ok(false);
    }

    let file_hdr_sz = u16::from_le_bytes([hdr[8], hdr[9]]) as u64;
    let chunk_hdr_sz = u16::from_le_bytes([hdr[10], hdr[11]]) as u64;
    let blk_sz = u32::from_le_bytes(hdr[12..16].try_into()?) as u64;
    let total_blks = u32::from_le_bytes(hdr[16..20].try_into()?) as u64;
    let total_chunks = u32::from_le_bytes(hdr[20..24].try_into()?) as u64;
    if file_hdr_sz < 28 || chunk_hdr_sz < 12 {
        bail!("corrupt sparse image header");
    }
    skip(&mut src, file_hdr_sz - 28)?;

    let mut dst = BufWriter::with_capacity(BUF, File::create(raw_path)?);
    let mut buf = vec![0u8; BUF];

    for _ in 0..total_chunks {
        let mut ch = [0u8; 12];
        src.read_exact(&mut ch)?;
        let typ = u16::from_le_bytes([ch[0], ch[1]]);
        let chunk_sz = u32::from_le_bytes(ch[4..8].try_into()?) as u64;
        skip(&mut src, chunk_hdr_sz - 12)?;
        let out_len = chunk_sz * blk_sz;

        match typ {
            CHUNK_RAW => copy_exact(&mut src, &mut dst, out_len, &mut buf)?,
            CHUNK_FILL => {
                let mut pattern = [0u8; 4];
                src.read_exact(&mut pattern)?;
                fill(&mut dst, pattern, out_len, &mut buf)?;
            }
            CHUNK_DONT_CARE => {
                dst.seek(SeekFrom::Current(out_len as i64))?;
            }
            CHUNK_CRC => skip(&mut src, 4)?,
            other => bail!("unknown sparse chunk type 0x{other:04X}"),
        }
    }

    dst.flush()?;
    let file = dst.into_inner().map_err(|e| e.into_error())?;
    file.set_len(total_blks * blk_sz)?; // trailing don't-care blocks become zeros
    Ok(true)
}

fn skip<R: Read>(r: &mut R, n: u64) -> Result<()> {
    let got = io::copy(&mut r.by_ref().take(n), &mut io::sink())?;
    if got != n {
        bail!("truncated sparse image");
    }
    Ok(())
}

fn copy_exact<R: Read, W: Write>(src: &mut R, dst: &mut W, mut count: u64, buf: &mut [u8]) -> Result<()> {
    while count > 0 {
        let n = count.min(buf.len() as u64) as usize;
        src.read_exact(&mut buf[..n])?;
        dst.write_all(&buf[..n])?;
        count -= n as u64;
    }
    Ok(())
}

fn fill<W: Write>(dst: &mut W, pattern: [u8; 4], mut count: u64, buf: &mut [u8]) -> Result<()> {
    for chunk in buf.chunks_exact_mut(4) {
        chunk.copy_from_slice(&pattern);
    }
    while count > 0 {
        let n = count.min(buf.len() as u64) as usize;
        dst.write_all(&buf[..n])?;
        count -= n as u64;
    }
    Ok(())
}
