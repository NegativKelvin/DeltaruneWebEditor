//! xdelta3 patches (VCDIFF, RFC 3284), the usual format of Deltarune mods.
//!
//! Encode writes plain VCDIFF with Adler-32 window checksums (no secondary compression), so every
//! xdelta3 tool (xdelta3, Delta Patcher, xdelta UI) applies it. Decode reads plain patches and patches
//! with LZMA secondary compression (the xdelta3 default when it is built with liblzma; other builds
//! default to none). LZMA sections are unpacked here first, because they are xz streams.
//! DJW and FGK secondary compression (only with `xdelta3 -S djw|fgk`) are not supported.

use std::path::Path;

use oxidelta::compress::encoder::CompressOptions;

use crate::error::{Error, Result};

const MAGIC: [u8; 3] = [0xD6, 0xC3, 0xC4];
const VCD_DECOMPRESS: u8 = 0x01;
const VCD_CODETABLE: u8 = 0x02;
const VCD_APPHEADER: u8 = 0x04;
const VCD_SOURCE: u8 = 0x01;
const VCD_TARGET: u8 = 0x02;
const VCD_ADLER32: u8 = 0x04;
const LZMA_ID: u8 = 2;

pub fn is_vcdiff(bytes: &[u8]) -> bool {
    bytes.len() >= 4 && bytes[..3] == MAGIC
}

fn bad(e: impl std::fmt::Display) -> Error {
    Error::BadRequest(format!("not a valid xdelta patch: {e}"))
}

struct Reader<'a> {
    b: &'a [u8],
    pos: usize,
}

impl<'a> Reader<'a> {
    fn byte(&mut self) -> Result<u8> {
        let v = *self.b.get(self.pos).ok_or_else(|| bad("unexpected end"))?;
        self.pos += 1;
        Ok(v)
    }
    fn varint(&mut self) -> Result<u64> {
        let mut v: u64 = 0;
        for _ in 0..10 {
            let c = self.byte()?;
            v = v.checked_mul(128).ok_or_else(|| bad("integer too large"))? | u64::from(c & 0x7f);
            if c & 0x80 == 0 {
                return Ok(v);
            }
        }
        Err(bad("integer too long"))
    }
    fn take(&mut self, n: u64) -> Result<&'a [u8]> {
        let n = usize::try_from(n).map_err(bad)?;
        let end = self
            .pos
            .checked_add(n)
            .filter(|e| *e <= self.b.len())
            .ok_or_else(|| bad("unexpected end"))?;
        let s = &self.b[self.pos..end];
        self.pos = end;
        Ok(s)
    }
    fn done(&self) -> bool {
        self.pos >= self.b.len()
    }
}

fn put_varint(out: &mut Vec<u8>, mut v: u64) {
    let mut tmp = [0u8; 10];
    let mut i = tmp.len();
    loop {
        i -= 1;
        tmp[i] = (v & 0x7f) as u8 | if i == tmp.len() - 1 { 0 } else { 0x80 };
        v >>= 7;
        if v == 0 {
            break;
        }
    }
    out.extend_from_slice(&tmp[i..]);
}

/// What a patch says about itself.
#[derive(Debug, Default, Clone)]
pub struct Info {
    pub windows: usize,
    /// Every window has an Adler-32 checksum (a wrong source file is found).
    pub checksums: bool,
    pub target_size: u64,
    /// Largest source offset that a window reads, plus one.
    pub source_end: u64,
}

struct Window<'a> {
    win: u8,
    seg: Option<(u64, u64)>,
    target_len: u64,
    ind: u8,
    adler: Option<&'a [u8]>,
    sections: [&'a [u8]; 3],
}

/// xdelta3 LZMA secondary compression: one xz stream per section kind (data, inst, addr) for the whole
/// file, sync-flushed after each section. Each compressed section is `<size varint><stream part>`.
/// Joins the parts of one kind and unpacks them: returns the plain sections in order.
fn unpack_lzma(parts: &[&[u8]]) -> Result<Vec<Vec<u8>>> {
    let mut sizes = Vec::with_capacity(parts.len());
    let mut stream = Vec::new();
    for p in parts {
        let mut r = Reader { b: p, pos: 0 };
        sizes.push(usize::try_from(r.varint()?).map_err(bad)?);
        stream.extend_from_slice(&p[r.pos..]);
    }
    // xz stream header (12 bytes), then the block header ((first byte + 1) * 4 bytes), then LZMA2 chunks.
    if stream.len() < 13 || stream[..6] != [0xFD, b'7', b'z', b'X', b'Z', 0] {
        return Err(bad("LZMA section is not an xz stream"));
    }
    let block = 12 + (usize::from(stream[12]) + 1) * 4;
    let mut chunks = stream.get(block..).ok_or_else(|| bad("LZMA section: short block header"))?.to_vec();
    chunks.push(0); // end of LZMA2 data (xdelta3 does not finish the stream)
    let total: usize = sizes.iter().sum();
    let mut out = Vec::with_capacity(total.min(1 << 28));
    let res = lzma_rs::lzma2_decompress(&mut chunks.as_slice(), &mut out);
    if out.len() < total {
        return Err(bad(format!(
            "LZMA section: {}",
            res.err().map(|e| e.to_string()).unwrap_or_else(|| "too short".into())
        )));
    }
    let mut at = 0;
    Ok(sizes
        .into_iter()
        .map(|n| {
            let v = out[at..at + n].to_vec();
            at += n;
            v
        })
        .collect())
}

/// Reads the patch, and rewrites LZMA secondary sections as plain sections. Returns the patch (changed
/// only when it had LZMA sections) and its info.
fn normalize(patch: &[u8]) -> Result<(Vec<u8>, Info)> {
    if !is_vcdiff(patch) {
        return Err(bad("no VCDIFF header"));
    }
    let mut r = Reader { b: patch, pos: 4 };
    let hdr = r.byte()?;
    let secondary = if hdr & VCD_DECOMPRESS != 0 { Some(r.byte()?) } else { None };
    if hdr & VCD_CODETABLE != 0 {
        let n = r.varint()?;
        r.take(n)?;
    }
    if hdr & VCD_APPHEADER != 0 {
        let n = r.varint()?;
        r.take(n)?;
    }
    let header_end = r.pos;
    let mut info = Info {
        checksums: true,
        ..Info::default()
    };
    let mut windows = Vec::new();
    while !r.done() {
        let win = r.byte()?;
        let seg = if win & (VCD_SOURCE | VCD_TARGET) != 0 {
            Some((r.varint()?, r.varint()?))
        } else {
            None
        };
        if win & VCD_SOURCE != 0
            && let Some((len, pos)) = seg
        {
            info.source_end = info.source_end.max(pos + len);
        }
        let delta_len = r.varint()?;
        let mut d = Reader {
            b: r.take(delta_len)?,
            pos: 0,
        };
        let target_len = d.varint()?;
        let ind = d.byte()?;
        let lens = [d.varint()?, d.varint()?, d.varint()?];
        let adler = if win & VCD_ADLER32 != 0 { Some(d.take(4)?) } else { None };
        let sections = [d.take(lens[0])?, d.take(lens[1])?, d.take(lens[2])?];
        info.windows += 1;
        info.target_size += target_len;
        info.checksums &= adler.is_some();
        windows.push(Window {
            win,
            seg,
            target_len,
            ind,
            adler,
            sections,
        });
    }
    if info.windows == 0 {
        return Err(bad("no windows"));
    }
    match secondary {
        None => return Ok((patch.to_vec(), info)),
        Some(LZMA_ID) => {}
        Some(id) => {
            // A patch that compresses no section still decodes.
            if windows.iter().all(|w| w.ind & 7 == 0) {
                return Ok((patch.to_vec(), info));
            }
            let name = match id {
                1 => "DJW",
                16 => "FGK",
                _ => "unknown",
            };
            return Err(bad(format!(
                "{name} secondary compression is not supported; make the patch with xdelta3 -S lzma or -S none"
            )));
        }
    }
    // Plain sections per kind, in window order.
    let mut plain: Vec<std::vec::IntoIter<Vec<u8>>> = Vec::with_capacity(3);
    for k in 0..3 {
        let parts: Vec<&[u8]> = windows.iter().filter(|w| w.ind & (1 << k) != 0).map(|w| w.sections[k]).collect();
        plain.push(if parts.is_empty() { Vec::new() } else { unpack_lzma(&parts)? }.into_iter());
    }
    let mut out = Vec::with_capacity(patch.len() * 4);
    out.extend_from_slice(&patch[..4]);
    out.push(hdr & !VCD_DECOMPRESS);
    out.extend_from_slice(&patch[6..header_end]);
    for w in &windows {
        let sections: Vec<Vec<u8>> = (0..3)
            .map(|k| {
                if w.ind & (1 << k) != 0 {
                    plain[k].next().ok_or_else(|| bad("LZMA section missing"))
                } else {
                    Ok(w.sections[k].to_vec())
                }
            })
            .collect::<Result<_>>()?;
        let mut body = Vec::new();
        put_varint(&mut body, w.target_len);
        body.push(0);
        for s in &sections {
            put_varint(&mut body, s.len() as u64);
        }
        if let Some(a) = w.adler {
            body.extend_from_slice(a);
        }
        for s in &sections {
            body.extend_from_slice(s);
        }
        out.push(w.win);
        if let Some((len, pos)) = w.seg {
            put_varint(&mut out, len);
            put_varint(&mut out, pos);
        }
        put_varint(&mut out, body.len() as u64);
        out.extend_from_slice(&body);
    }
    Ok((out, info))
}

pub fn info(patch: &[u8]) -> Result<Info> {
    normalize(patch).map(|(_, i)| i)
}

/// Makes `out`: the patch from `source` to `target`.
pub fn encode(source: &Path, target: &Path, out: &Path) -> Result<()> {
    let opts = CompressOptions {
        checksum: true,
        ..CompressOptions::default()
    };
    oxidelta::io::encode_file(source, target, out, opts).map_err(|e| Error::Other(format!("xdelta encode: {e}")))?;
    Ok(())
}

/// Applies `patch` to `source`, writes `out`. Fails when a window checksum does not match (wrong source file).
pub fn decode(source: &Path, patch: &Path, out: &Path) -> Result<Info> {
    let (bytes, info) = normalize(&std::fs::read(patch)?)?;
    if let Ok(m) = std::fs::metadata(source)
        && info.source_end > m.len()
    {
        return Err(Error::BadRequest(
            "the patch needs a larger game file: it was made for another game version or chapter".into(),
        ));
    }
    let src = std::fs::read(source)?;
    let target = oxidelta::compress::decoder::decode_all(&src, &bytes).map_err(|e| {
        Error::BadRequest(format!(
            "the patch does not fit this game file ({e}); it was made for another game version or chapter"
        ))
    })?;
    std::fs::write(out, target)?;
    Ok(info)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn data(n: usize, seed: u32) -> Vec<u8> {
        let mut x = seed;
        (0..n)
            .map(|_| {
                x ^= x << 13;
                x ^= x >> 17;
                x ^= x << 5;
                x as u8
            })
            .collect()
    }

    #[test]
    fn roundtrip_and_wrong_source() {
        let tmp = tempfile::tempdir().unwrap();
        let [src, tgt, patch, out] = ["src", "tgt", "p.xdelta", "out"].map(|f| tmp.path().join(f));
        let s = data(3 << 20, 7);
        let mut t = s.clone();
        t[1000..1100].fill(b'Y');
        t.splice(2_000_000..2_000_000, b"INSERTED".repeat(100));
        std::fs::write(&src, &s).unwrap();
        std::fs::write(&tgt, &t).unwrap();
        encode(&src, &tgt, &patch).unwrap();
        let p = std::fs::read(&patch).unwrap();
        assert!(is_vcdiff(&p) && p.len() < 20_000);
        let i = decode(&src, &patch, &out).unwrap();
        assert!(i.checksums);
        assert_eq!(std::fs::read(&out).unwrap(), t);
        std::fs::write(&src, data(3 << 20, 9)).unwrap();
        assert!(decode(&src, &patch, &out).is_err());
        assert!(info(b"PK\x03\x04").is_err());
    }

    #[test]
    fn varints() {
        for v in [0u64, 1, 127, 128, 300, 1 << 35] {
            let mut b = Vec::new();
            put_varint(&mut b, v);
            assert_eq!(Reader { b: &b, pos: 0 }.varint().unwrap(), v);
        }
    }

    /// Patches made by the real xdelta3 (all secondary compressors), and ours applied by xdelta3.
    /// Runs when `xdelta3` is on PATH.
    #[test]
    fn xdelta3_interop() {
        if std::process::Command::new("xdelta3").arg("-V").output().is_err() {
            eprintln!("skipped: no xdelta3");
            return;
        }
        let tmp = tempfile::tempdir().unwrap();
        let [src, tgt, out] = ["src", "tgt", "out"].map(|f| tmp.path().join(f));
        let s = data(12 << 20, 3);
        let mut t = s.clone();
        t[5_000_000..5_000_500].fill(0);
        t.drain(9_000_000..9_100_000);
        let text: Vec<u8> = (0..60_000).flat_map(|i| format!("line {i} of new text\n").into_bytes()).collect();
        t.splice(3_000_000..3_000_000, text.iter().copied());
        t.splice(10_000_000..10_000_000, text);
        t.extend_from_slice(b"tail");
        std::fs::write(&src, &s).unwrap();
        std::fs::write(&tgt, &t).unwrap();
        for sec in [None, Some("lzma"), Some("djw"), Some("none")] {
            let p = tmp.path().join(format!("x-{}.xdelta", sec.unwrap_or("default")));
            let mut cmd = std::process::Command::new("xdelta3");
            cmd.args(["-e", "-f"]);
            if let Some(s) = sec {
                cmd.args(["-S", s]);
            }
            let st = cmd.arg("-s").arg(&src).arg(&tgt).arg(&p).status().unwrap();
            assert!(st.success());
            if sec == Some("djw") {
                assert!(decode(&src, &p, &out).unwrap_err().to_string().contains("DJW"));
                continue;
            }
            decode(&src, &p, &out).unwrap_or_else(|e| panic!("{sec:?}: {e}"));
            assert_eq!(std::fs::read(&out).unwrap(), t, "{sec:?}");
        }
        let ours = tmp.path().join("ours.xdelta");
        encode(&src, &tgt, &ours).unwrap();
        let st = std::process::Command::new("xdelta3")
            .args(["-d", "-f", "-s"])
            .arg(&src)
            .arg(&ours)
            .arg(&out)
            .status()
            .unwrap();
        assert!(st.success());
        assert_eq!(std::fs::read(&out).unwrap(), t);
    }
}
