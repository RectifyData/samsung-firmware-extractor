//! Pure-Rust LZ4 frame decoder (the format written by the `lz4` tool, and used by Samsung's *.img.lz4).
//!
//! Handles linked and independent blocks, block and content checksums (read past, not verified),
//! content-size and dictionary-id fields, skippable frames and concatenated frames.
//! Legacy (pre-frame) LZ4 streams are not supported.

use std::io::{self, Read};

const MAGIC: u32 = 0x184D_2204;
const SKIPPABLE_BASE: u32 = 0x184D_2A50;
const SKIPPABLE_MASK: u32 = 0xFFFF_FFF0;
const WINDOW: usize = 64 * 1024;
const BLOCK_MAX: [usize; 8] = [0, 0, 0, 0, 64 << 10, 256 << 10, 1 << 20, 4 << 20];

pub struct FrameDecoder<R: Read> {
    inner: R,
    /// `[0, hist)` holds match history; the current block is decoded after it.
    buf: Vec<u8>,
    src: Vec<u8>,
    hist: usize,
    out_pos: usize,
    out_end: usize,
    max_block: usize,
    in_frame: bool,
    block_checksum: bool,
    content_checksum: bool,
}

impl<R: Read> FrameDecoder<R> {
    pub fn new(inner: R) -> Self {
        Self {
            inner,
            buf: Vec::new(),
            src: Vec::new(),
            hist: 0,
            out_pos: 0,
            out_end: 0,
            max_block: 0,
            in_frame: false,
            block_checksum: false,
            content_checksum: false,
        }
    }

    /// Decodes the next block that yields output. Returns Ok(false) at a clean end of input.
    fn next_block(&mut self) -> io::Result<bool> {
        // Keep the last WINDOW bytes already delivered so matches can reach back into them.
        let keep = self.out_end.min(WINDOW);
        if keep > 0 {
            self.buf.copy_within(self.out_end - keep..self.out_end, 0);
        }
        self.hist = keep;
        self.out_pos = keep;
        self.out_end = keep;

        loop {
            if !self.in_frame && !self.open_frame()? {
                return Ok(false);
            }
            let size_field = self.read_u32()?;
            if size_field == 0 {
                // EndMark, followed by the content checksum when present.
                if self.content_checksum {
                    self.skip(4)?;
                }
                self.in_frame = false;
                continue;
            }
            let stored = size_field & 0x8000_0000 != 0;
            let size = (size_field & 0x7FFF_FFFF) as usize;
            if size > self.max_block {
                return Err(corrupt("block exceeds frame maximum"));
            }

            let hist = self.hist;
            let produced = if stored {
                self.inner.read_exact(&mut self.buf[hist..hist + size])?;
                size
            } else {
                self.inner.read_exact(&mut self.src[..size])?;
                decode_block(&self.src[..size], &mut self.buf, hist)?
            };
            if self.block_checksum {
                self.skip(4)?;
            }

            if produced > 0 {
                self.out_pos = hist;
                self.out_end = hist + produced;
                return Ok(true);
            }
        }
    }

    fn open_frame(&mut self) -> io::Result<bool> {
        let mut magic = match self.read_magic()? {
            Some(m) => m,
            None => return Ok(false),
        };
        while magic & SKIPPABLE_MASK == SKIPPABLE_BASE {
            let n = self.read_u32()?;
            self.skip(n as u64)?;
            magic = match self.read_magic()? {
                Some(m) => m,
                None => return Ok(false),
            };
        }
        if magic != MAGIC {
            return Err(corrupt(&format!("not an LZ4 frame (magic 0x{magic:08X})")));
        }

        let flg = self.read_u8()?;
        if flg >> 6 != 1 {
            return Err(corrupt("unsupported frame version"));
        }
        self.block_checksum = flg & 0x10 != 0;
        self.content_checksum = flg & 0x04 != 0;
        let has_content_size = flg & 0x08 != 0;
        let has_dict_id = flg & 0x01 != 0;

        let bd = self.read_u8()?;
        let block_max = BLOCK_MAX[((bd >> 4) & 0x07) as usize];
        if block_max == 0 {
            return Err(corrupt("invalid block size"));
        }
        if has_content_size {
            self.skip(8)?;
        }
        if has_dict_id {
            self.skip(4)?;
        }
        self.read_u8()?; // header checksum, not verified

        self.max_block = block_max;
        if self.buf.len() < WINDOW + block_max {
            self.buf.resize(WINDOW + block_max, 0); // resize keeps the history prefix
        }
        if self.src.len() < block_max {
            self.src.resize(block_max, 0);
        }
        self.in_frame = true;
        Ok(true)
    }

    fn read_magic(&mut self) -> io::Result<Option<u32>> {
        let mut b = [0u8; 4];
        if self.inner.read(&mut b[..1])? == 0 {
            return Ok(None);
        }
        self.inner.read_exact(&mut b[1..])?;
        Ok(Some(u32::from_le_bytes(b)))
    }

    fn read_u8(&mut self) -> io::Result<u8> {
        let mut b = [0u8; 1];
        self.inner.read_exact(&mut b)?;
        Ok(b[0])
    }

    fn read_u32(&mut self) -> io::Result<u32> {
        let mut b = [0u8; 4];
        self.inner.read_exact(&mut b)?;
        Ok(u32::from_le_bytes(b))
    }

    fn skip(&mut self, n: u64) -> io::Result<()> {
        let got = io::copy(&mut (&mut self.inner).take(n), &mut io::sink())?;
        if got != n {
            return Err(io::Error::new(io::ErrorKind::UnexpectedEof, "truncated LZ4 frame"));
        }
        Ok(())
    }
}

impl<R: Read> Read for FrameDecoder<R> {
    fn read(&mut self, out: &mut [u8]) -> io::Result<usize> {
        if out.is_empty() {
            return Ok(0);
        }
        while self.out_pos >= self.out_end {
            if !self.next_block()? {
                return Ok(0);
            }
        }
        let n = out.len().min(self.out_end - self.out_pos);
        out[..n].copy_from_slice(&self.buf[self.out_pos..self.out_pos + n]);
        self.out_pos += n;
        Ok(n)
    }
}

fn corrupt(why: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, format!("corrupt LZ4 data: {why}"))
}

fn read_ext(src: &[u8], ip: &mut usize) -> io::Result<usize> {
    let mut total = 0usize;
    loop {
        let b = *src.get(*ip).ok_or_else(|| corrupt("truncated length"))?;
        *ip += 1;
        total += b as usize;
        if b != 255 {
            return Ok(total);
        }
    }
}

/// Decodes one LZ4 block from `src` into `dst[base..]`, using `dst[..base]` as match history.
fn decode_block(src: &[u8], dst: &mut [u8], base: usize) -> io::Result<usize> {
    let mut ip = 0usize;
    let mut op = base;
    loop {
        let token = *src.get(ip).ok_or_else(|| corrupt("truncated sequence"))?;
        ip += 1;

        let mut lit = (token >> 4) as usize;
        if lit == 15 {
            lit += read_ext(src, &mut ip)?;
        }
        if lit > src.len() - ip || lit > dst.len() - op {
            return Err(corrupt("literal overflow"));
        }
        dst[op..op + lit].copy_from_slice(&src[ip..ip + lit]);
        ip += lit;
        op += lit;

        if ip == src.len() {
            return Ok(op - base); // the last sequence carries literals only
        }
        if src.len() - ip < 2 {
            return Err(corrupt("truncated match offset"));
        }
        let offset = u16::from_le_bytes([src[ip], src[ip + 1]]) as usize;
        ip += 2;

        let mut mlen = (token & 0x0F) as usize;
        if mlen == 15 {
            mlen += read_ext(src, &mut ip)?;
        }
        mlen += 4;
        if offset == 0 || offset > op || mlen > dst.len() - op {
            return Err(corrupt("bad match"));
        }

        let start = op - offset;
        if offset >= mlen {
            dst.copy_within(start..start + mlen, op);
        } else {
            // Overlapping copy: must go byte by byte so repeated patterns expand correctly.
            for i in 0..mlen {
                dst[op + i] = dst[start + i];
            }
        }
        op += mlen;
    }
}

#[cfg(test)]
mod tests {
    use super::FrameDecoder;
    use std::io::Read;

    #[test]
    fn rejects_bad_magic() {
        let mut out = Vec::new();
        let err = FrameDecoder::new(&[0u8, 1, 2, 3, 4, 5, 6, 7][..]).read_to_end(&mut out).unwrap_err();
        assert_eq!(err.kind(), std::io::ErrorKind::InvalidData);
    }

    #[test]
    fn empty_input_is_empty_output() {
        let mut out = Vec::new();
        FrameDecoder::new(&[][..]).read_to_end(&mut out).unwrap();
        assert!(out.is_empty());
    }
}
