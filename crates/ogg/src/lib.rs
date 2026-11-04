//! Clean-room Ogg demuxer, written from RFC 3533 (the Ogg encapsulation format), RFC 7845 (Ogg
//! Opus) and the Vorbis I specification's Ogg mapping (section A).
//!
//! [`open`] reads every page (capture pattern, header, lacing values, CRC-32 checked; damaged
//! regions are skipped by searching for the next `OggS`), groups them by logical bitstream and
//! assembles packets — including packets that continue across pages — as lists of byte ranges,
//! with the granule position of the page each packet completes on. Codec headers (Opus:
//! `OpusHead` + `OpusTags`; Vorbis: identification, comment, setup) are collected separately from
//! the audio packets.
//!
//! [`OpusTiming`] implements RFC 7845 §4: packet start positions from granule positions and
//! TOC-derived durations, the pre-skip, and end trimming of the final page, so a player can seek
//! to any 48 kHz sample.
//!
//! Layer L0: no dependencies beyond `std`, no `unsafe`, builds for `wasm32-unknown-unknown`.

#![cfg_attr(not(test), deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable))]

use std::collections::HashMap;
use std::fmt;
use std::io;

/// Random-access, read-only byte source (same shape as the other FilmCraft demuxers').
pub trait ByteSource {
    fn len(&self) -> u64;
    fn read_at(&self, offset: u64, buf: &mut [u8]) -> io::Result<()>;
    fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

impl ByteSource for [u8] {
    fn len(&self) -> u64 {
        <[u8]>::len(self) as u64
    }
    fn read_at(&self, offset: u64, buf: &mut [u8]) -> io::Result<()> {
        let eof = || io::Error::new(io::ErrorKind::UnexpectedEof, "read past end");
        let a = usize::try_from(offset).map_err(|_| eof())?;
        buf.copy_from_slice(self.get(a..a.checked_add(buf.len()).ok_or_else(eof)?).ok_or_else(eof)?);
        Ok(())
    }
}

impl ByteSource for Vec<u8> {
    fn len(&self) -> u64 {
        self.as_slice().len() as u64
    }
    fn read_at(&self, offset: u64, buf: &mut [u8]) -> io::Result<()> {
        self.as_slice().read_at(offset, buf)
    }
}

impl<T: ByteSource + ?Sized> ByteSource for &T {
    fn len(&self) -> u64 {
        (**self).len()
    }
    fn read_at(&self, offset: u64, buf: &mut [u8]) -> io::Result<()> {
        (**self).read_at(offset, buf)
    }
}

#[derive(Debug)]
pub enum Error {
    Io(io::Error),
    /// No Ogg page found.
    NotOgg,
    Invalid(String),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Io(e) => write!(f, "I/O error: {e}"),
            Error::NotOgg => write!(f, "not an Ogg file"),
            Error::Invalid(s) => write!(f, "invalid Ogg: {s}"),
        }
    }
}

impl std::error::Error for Error {}

impl From<io::Error> for Error {
    fn from(e: io::Error) -> Self {
        Error::Io(e)
    }
}

pub type Result<T> = std::result::Result<T, Error>;

/// The capture pattern at the start of a file.
pub fn sniff(head: &[u8]) -> bool {
    head.len() >= 27 && &head[..4] == b"OggS" && head[4] == 0
}

/// CRC-32 of RFC 3533 §6: polynomial 0x04C11DB7, initial value 0, no reflection, no final XOR.
pub fn crc32(data: &[u8]) -> u32 {
    static TABLE: std::sync::OnceLock<[u32; 256]> = std::sync::OnceLock::new();
    let t = TABLE.get_or_init(|| {
        let mut t = [0u32; 256];
        for (i, e) in t.iter_mut().enumerate() {
            let mut r = (i as u32) << 24;
            for _ in 0..8 {
                r = if r & 0x8000_0000 != 0 { (r << 1) ^ 0x04C1_1DB7 } else { r << 1 };
            }
            *e = r;
        }
        t
    });
    data.iter().fold(0u32, |c, &b| (c << 8) ^ t[((c >> 24) as u8 ^ b) as usize])
}

/// The codec of a logical bitstream (from its first packet).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Codec {
    Opus,
    Vorbis,
    Flac,
    Theora,
    Speex,
    Unknown,
}

impl Codec {
    fn of(first: &[u8]) -> Codec {
        if first.starts_with(b"OpusHead") {
            Codec::Opus
        } else if first.starts_with(b"\x01vorbis") {
            Codec::Vorbis
        } else if first.starts_with(b"\x7fFLAC") {
            Codec::Flac
        } else if first.starts_with(b"\x80theora") {
            Codec::Theora
        } else if first.starts_with(b"Speex   ") {
            Codec::Speex
        } else {
            Codec::Unknown
        }
    }
    /// Header packets before the audio packets.
    fn header_count(self) -> usize {
        match self {
            Codec::Opus => 2,
            Codec::Vorbis | Codec::Theora => 3,
            Codec::Speex => 2,
            Codec::Flac | Codec::Unknown => 1,
        }
    }
    pub fn name(self) -> &'static str {
        match self {
            Codec::Opus => "Opus",
            Codec::Vorbis => "Vorbis",
            Codec::Flac => "FLAC",
            Codec::Theora => "Theora",
            Codec::Speex => "Speex",
            Codec::Unknown => "unknown",
        }
    }
}

/// A packet: its byte ranges (a packet may span pages) and the granule position of the page it
/// completes on when it is that page's last completed packet.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Packet {
    pub parts: Vec<(u64, u32)>,
    pub size: u32,
    /// Granule position of the completing page if this is the last packet completed on it.
    pub granule: Option<i64>,
    /// Index of the page (within the stream) the packet completes on.
    pub page: u32,
    /// The packet completes on the stream's end-of-stream page.
    pub eos: bool,
    /// The first bytes of the packet (TOC for Opus, packet type for Vorbis).
    pub prefix: [u8; 2],
}

/// A logical bitstream.
#[derive(Clone, Debug)]
pub struct Stream {
    pub serial: u32,
    pub codec: Codec,
    /// Codec header packets (contents).
    pub headers: Vec<Vec<u8>>,
    /// Data packets after the headers.
    pub packets: Vec<Packet>,
    pub pages: u32,
    /// Granule position of the last page with one.
    pub last_granule: Option<i64>,
    pub saw_eos: bool,
}

/// An opened Ogg file.
#[derive(Clone, Debug)]
pub struct OggFile {
    pub streams: Vec<Stream>,
    /// Pages dropped for a bad CRC, lost sync, truncation…
    pub warnings: Vec<String>,
}

/// Source bytes damage recovery may read per file: the windows the scan for the next capture
/// pattern fills and the pages read only to find their CRC wrong.
const RECOVERY_BUDGET: u64 = 32 << 20;
/// Warnings kept per file (not counting the one about the budget); the rest are counted in one last entry.
const MAX_WARNINGS: usize = 256;

/// Record a warning, keeping at most `MAX_WARNINGS` and one entry saying the rest were dropped.
fn warn(warnings: &mut Vec<String>, msg: String) {
    match warnings.len().cmp(&MAX_WARNINGS) {
        std::cmp::Ordering::Less => warnings.push(msg),
        std::cmp::Ordering::Equal => warnings.push("further warnings omitted".into()),
        std::cmp::Ordering::Greater => {}
    }
}

struct Partial {
    parts: Vec<(u64, u32)>,
    size: u32,
    prefix: Vec<u8>,
}

/// Read every page and assemble the packets of every logical bitstream.
pub fn open(src: &(impl ByteSource + ?Sized)) -> Result<OggFile> {
    let len = src.len();
    let mut streams: Vec<Stream> = Vec::new();
    let mut by_serial: HashMap<u32, usize> = HashMap::new();
    let mut partial: HashMap<u32, Partial> = HashMap::new();
    let mut warnings = Vec::new();
    let mut pos = 0u64;
    let mut found_any = false;
    let mut hdr = [0u8; 27 + 255];
    let mut scanner = CaptureScanner::new();
    while pos + 27 <= len {
        src.read_at(pos, &mut hdr[..27])?;
        if &hdr[..4] != b"OggS" || hdr[4] != 0 {
            match scanner.find(src, pos + 1, len)? {
                Some(p) => {
                    if found_any {
                        warn(&mut warnings, format!("lost sync at byte {pos}; resumed at {p}"));
                    }
                    pos = p;
                    continue;
                }
                None => break,
            }
        }
        let nseg = hdr[26] as usize;
        if pos + 27 + nseg as u64 > len {
            warn(&mut warnings, format!("truncated page header at byte {pos}"));
            break;
        }
        src.read_at(pos + 27, &mut hdr[27..27 + nseg])?;
        let body_len: u64 = hdr[27..27 + nseg].iter().map(|&b| b as u64).sum();
        let page_len = 27 + nseg as u64 + body_len;
        if pos + page_len > len {
            warn(&mut warnings, format!("truncated page at byte {pos}"));
            break;
        }
        let mut page = vec![0u8; page_len as usize];
        src.read_at(pos, &mut page)?;
        let stored_crc = u32::from_le_bytes([page[22], page[23], page[24], page[25]]);
        page[22..26].fill(0);
        if crc32(&page) != stored_crc {
            warn(&mut warnings, format!("bad CRC on page at byte {pos}"));
            // a page read only to find it damaged is recovery work, charged like the scan's windows
            if scanner.charge(page_len) {
                break;
            }
            pos += 1;
            continue;
        }
        found_any = true;
        let flags = page[5];
        let granule = i64::from_le_bytes(page[6..14].try_into().unwrap_or([0; 8]));
        let serial = u32::from_le_bytes([page[14], page[15], page[16], page[17]]);
        let lacing = page[27..27 + nseg].to_vec();
        let body_off = pos + 27 + nseg as u64;
        let si = *by_serial.entry(serial).or_insert_with(|| {
            streams.push(Stream {
                serial,
                codec: Codec::Unknown,
                headers: Vec::new(),
                packets: Vec::new(),
                pages: 0,
                last_granule: None,
                saw_eos: false,
            });
            streams.len() - 1
        });
        let st = &mut streams[si];
        let page_index = st.pages;
        st.pages += 1;
        if flags & 0x04 != 0 {
            st.saw_eos = true;
        }
        // a continued page without a pending packet (lost a page): drop the fragment
        let continued = flags & 0x01 != 0;
        if !continued && partial.remove(&serial).is_some() {
            warn(&mut warnings, format!("incomplete packet before page at byte {pos}"));
        }
        let mut completed: Vec<Packet> = Vec::new();
        let mut off = 0u64;
        let mut seg_start = 0u64;
        let mut skip_fragment = continued && !partial.contains_key(&serial);
        for (k, &l) in lacing.iter().enumerate() {
            off += l as u64;
            if l < 255 || k + 1 == nseg {
                let (a, n) = (seg_start, off - seg_start);
                seg_start = off;
                if skip_fragment {
                    if l < 255 {
                        skip_fragment = false;
                    }
                    continue;
                }
                let p = partial.entry(serial).or_insert_with(|| Partial { parts: Vec::new(), size: 0, prefix: Vec::new() });
                if n > 0 {
                    p.parts.push((body_off + a, n as u32));
                    p.size += n as u32;
                    let s = (body_off - pos + a) as usize;
                    let need = 2usize.saturating_sub(p.prefix.len()).min(n as usize);
                    p.prefix.extend_from_slice(&page[s..s + need]);
                }
                if l < 255
                    && let Some(p) = partial.remove(&serial)
                {
                    let mut prefix = [0u8; 2];
                    prefix[..p.prefix.len().min(2)].copy_from_slice(&p.prefix[..p.prefix.len().min(2)]);
                    completed.push(Packet { parts: p.parts, size: p.size, granule: None, page: page_index, eos: flags & 0x04 != 0, prefix });
                }
            }
        }
        if granule != -1 {
            st.last_granule = Some(granule);
            if let Some(last) = completed.last_mut() {
                last.granule = Some(granule);
            }
        }
        for pk in completed {
            if st.headers.is_empty() && st.packets.is_empty() {
                let data = read_parts(src, &pk.parts)?;
                st.codec = Codec::of(&data);
            }
            if st.headers.len() < st.codec.header_count() && st.packets.is_empty() {
                st.headers.push(read_parts(src, &pk.parts)?);
            } else {
                st.packets.push(pk);
            }
        }
        pos += page_len;
    }
    if !found_any {
        return Err(Error::NotOgg);
    }
    if !partial.is_empty() {
        warn(&mut warnings, "file ends inside a packet".into());
    }
    if scanner.spent() {
        // once per file and last, so it is not subject to the cap on the others
        warnings.push("recovery budget spent; the rest of the damaged data was not searched".into());
    }
    Ok(OggFile { streams, warnings })
}

struct CaptureScanner {
    buf: Vec<u8>,
    buf_start: u64,
    budget: u64,
}

impl CaptureScanner {
    fn new() -> Self {
        Self { buf: Vec::new(), buf_start: 0, budget: RECOVERY_BUDGET }
    }

    /// Charge `bytes` of recovery work; true once the budget is spent.
    fn charge(&mut self, bytes: u64) -> bool {
        self.budget = self.budget.saturating_sub(bytes);
        self.spent()
    }

    /// Nothing left to search with: fewer bytes than a capture pattern can still be charged for.
    fn spent(&self) -> bool {
        self.budget < 4
    }

    fn find(&mut self, src: &(impl ByteSource + ?Sized), from: u64, len: u64) -> Result<Option<u64>> {
        let mut at = from;
        while at + 4 <= len {
            let buf_end = self.buf_start + self.buf.len() as u64;
            if at >= self.buf_start && at + 4 <= buf_end {
                let off = (at - self.buf_start) as usize;
                if let Some(i) = self.buf[off..].windows(4).position(|w| w == b"OggS") {
                    return Ok(Some(at + i as u64));
                }
                at = buf_end.saturating_sub(3);
                continue;
            }
            if self.spent() {
                return Ok(None);
            }
            let n = ((len - at) as usize).min(64 * 1024).min(usize::try_from(self.budget).unwrap_or(usize::MAX));
            if n < 4 {
                break;
            }
            self.buf.resize(n, 0);
            src.read_at(at, &mut self.buf)?;
            self.buf_start = at;
            self.budget = self.budget.saturating_sub(n as u64);
            if let Some(i) = self.buf.windows(4).position(|w| w == b"OggS") {
                return Ok(Some(at + i as u64));
            }
            at += (n - 3) as u64;
        }
        Ok(None)
    }
}

fn read_parts(src: &(impl ByteSource + ?Sized), parts: &[(u64, u32)]) -> Result<Vec<u8>> {
    let mut v = Vec::with_capacity(parts.iter().map(|p| p.1 as usize).sum());
    for &(o, n) in parts {
        let a = v.len();
        v.resize(a + n as usize, 0);
        src.read_at(o, &mut v[a..])?;
    }
    Ok(v)
}

impl OggFile {
    /// The first stream of `codec`.
    pub fn stream_of(&self, codec: Codec) -> Option<usize> {
        self.streams.iter().position(|s| s.codec == codec)
    }
    /// The bytes of packet `i` of stream `s`.
    pub fn read_packet(&self, src: &(impl ByteSource + ?Sized), s: usize, i: usize) -> Result<Vec<u8>> {
        let p = self.streams.get(s).and_then(|st| st.packets.get(i)).ok_or_else(|| Error::Invalid(format!("no packet {i} in stream {s}")))?;
        read_parts(src, &p.parts)
    }
}

/// Duration of an Opus packet in 48 kHz samples from its TOC (RFC 6716 §3.1); `None` if invalid.
pub fn opus_packet_samples(prefix: [u8; 2], size: u32) -> Option<u32> {
    if size == 0 {
        return None;
    }
    let toc = prefix[0];
    let config = toc >> 3;
    let frame = match config {
        0..=11 => [480, 960, 1920, 2880][(config & 3) as usize],
        12..=15 => [480, 960][(config & 1) as usize],
        _ => [120, 240, 480, 960][(config & 3) as usize],
    };
    let frames = match toc & 3 {
        0 => 1,
        1 | 2 => 2,
        _ => {
            if size < 2 {
                return None;
            }
            (prefix[1] & 0x3F) as u32
        }
    };
    let n = frame * frames;
    (n > 0 && n <= 5760).then_some(n)
}

/// RFC 7845 §4 timing of an Opus stream.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OpusTiming {
    pub pre_skip: u32,
    /// Start of each packet's decoded output in output samples (48 kHz): 0 is the first sample
    /// after the pre-skip; the pre-skip samples have negative positions.
    pub starts: Vec<i64>,
    /// Decoded samples of each packet.
    pub durations: Vec<u32>,
    /// Playable samples (end trimming applied).
    pub total: i64,
}

fn timing_add(a: i64, b: i64) -> Result<i64> {
    a.checked_add(b).ok_or_else(|| Error::Invalid("opus timing overflow".into()))
}

fn timing_sub(a: i64, b: i64) -> Result<i64> {
    a.checked_sub(b).ok_or_else(|| Error::Invalid("opus timing overflow".into()))
}

impl OpusTiming {
    /// Timing of the Opus stream `s` (`pre_skip` from its `OpusHead`).
    ///
    /// A granule or a run of packet durations that does not fit in `i64` is an error. Ordinary
    /// files, including a truncated tail and end trimming, are unchanged.
    pub fn of(stream: &Stream, pre_skip: u32) -> Result<OpusTiming> {
        let durations: Vec<u32> = stream.packets.iter().map(|p| opus_packet_samples(p.prefix, p.size).unwrap_or(0)).collect();
        let n = durations.len();
        let mut ends: Vec<Option<i64>> = vec![None; n];
        let mut i = 0;
        let mut cursor: Option<i64> = None;
        let mut trimmed_end: Option<i64> = None;
        while i < n {
            // packets completing on the same page: i..j, the last carries the page granule
            let mut j = i;
            while j < n && stream.packets[j].granule.is_none() {
                j += 1;
            }
            if j == n {
                // trailing packets without a granule (truncated file): continue from the cursor
                let mut c = cursor.unwrap_or(0);
                for k in i..n {
                    c = timing_add(c, i64::from(durations[k]))?;
                    ends[k] = Some(c);
                }
                break;
            }
            let Some(g) = stream.packets[j].granule else { break };
            let sum = durations[i..=j].iter().try_fold(0i64, |a, &d| timing_add(a, i64::from(d)))?;
            let forward = match cursor {
                Some(c) => Some(timing_add(c, sum)?),
                None => None,
            };
            let end = match forward {
                // end trimming on the last page (RFC 7845 §4.5): keep the forward positions
                Some(f) if stream.packets[j].eos && g < f => {
                    trimmed_end = Some(g);
                    f
                }
                Some(f) if f == g => f,
                // first page, or a discontinuity: trust the granule
                _ => g,
            };
            let mut e = end;
            for k in (i..=j).rev() {
                ends[k] = Some(e);
                e = timing_sub(e, i64::from(durations[k]))?;
            }
            cursor = Some(end);
            i = j + 1;
        }
        let mut raw_starts = Vec::with_capacity(n);
        for (e, &d) in ends.iter().zip(&durations) {
            raw_starts.push(timing_sub(e.unwrap_or(0), i64::from(d))?);
        }
        let base = raw_starts.first().copied().unwrap_or(0).max(0);
        let shift = timing_add(base, i64::from(pre_skip))?;
        let mut starts = Vec::with_capacity(n);
        for s in &raw_starts {
            starts.push(timing_sub(*s, shift)?);
        }
        let end_all = trimmed_end.or(cursor).unwrap_or(0);
        let total = timing_sub(end_all, shift)?.max(0);
        Ok(OpusTiming { pre_skip, starts, durations, total })
    }
}

/// Vorbis identification header fields (Vorbis I §4.2.2).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct VorbisInfo {
    pub channels: u8,
    pub sample_rate: u32,
    pub bitrate_nominal: i32,
}

impl VorbisInfo {
    pub fn parse(h: &[u8]) -> Option<VorbisInfo> {
        if h.len() < 30 || !h.starts_with(b"\x01vorbis") {
            return None;
        }
        Some(VorbisInfo {
            channels: h[11],
            sample_rate: u32::from_le_bytes([h[12], h[13], h[14], h[15]]),
            bitrate_nominal: i32::from_le_bytes([h[20], h[21], h[22], h[23]]),
        })
    }
}

#[cfg(test)]
mod tests;
