//! Bounded binary readers used by the public MTX format decoder.
use super::Result;

pub(super) struct Reader<'a> {
    pub bytes: &'a [u8],
    pub pos: usize,
}
impl<'a> Reader<'a> {
    pub fn new(bytes: &'a [u8]) -> Self {
        Self { bytes, pos: 0 }
    }
    pub fn take(&mut self, n: usize) -> Result<&'a [u8]> {
        let end = self.pos.checked_add(n).ok_or("MTX offset overflow")?;
        let b = self.bytes.get(self.pos..end).ok_or("truncated MTX data")?;
        self.pos = end;
        Ok(b)
    }
    pub fn byte(&mut self) -> Result<u8> {
        self.take(1)?.first().copied().ok_or("truncated byte")
    }
    pub fn u16(&mut self) -> Result<u16> {
        Ok(u16::from_be_bytes(self.take(2)?.try_into().map_err(|_| "short u16")?))
    }
    pub fn i16(&mut self) -> Result<i16> {
        Ok(self.u16()? as i16)
    }
    pub fn u32(&mut self) -> Result<u32> {
        Ok(u32::from_be_bytes(self.take(4)?.try_into().map_err(|_| "short u32")?))
    }
    pub fn u24(&mut self) -> Result<usize> {
        let mut n = 0usize;
        for _ in 0..3 {
            n = n * 256 + self.byte()? as usize;
        }
        Ok(n)
    }
    pub fn u255(&mut self) -> Result<u16> {
        Ok(match self.byte()? {
            253 => self.u16()?,
            254 => 506 + self.byte()? as u16,
            255 => 253 + self.byte()? as u16,
            n => n as u16,
        })
    }
    pub fn s255(&mut self) -> Result<i16> {
        let mut b = self.byte()?;
        if b == 253 {
            return self.i16();
        }
        let sign = if b == 250 {
            b = self.byte()?;
            -1
        } else {
            1
        };
        let n = match b {
            254 => 500 + self.byte()? as i16,
            255 => 250 + self.byte()? as i16,
            0..=249 => b as i16,
            _ => return Err("invalid MTX signed value"),
        };
        Ok(sign * n)
    }
}

pub(super) struct Bits<'a> {
    bytes: &'a [u8],
    pos: usize,
    lsb: bool,
}
impl<'a> Bits<'a> {
    pub fn new(bytes: &'a [u8], lsb: bool) -> Self {
        Self { bytes, pos: 0, lsb }
    }
    pub fn bit(&mut self) -> Result<usize> {
        let b = self.bytes.get(self.pos / 8).ok_or("truncated MTX bit stream")?;
        let shift = if self.lsb { self.pos % 8 } else { 7 - self.pos % 8 };
        self.pos += 1;
        Ok(((b >> shift) & 1) as usize)
    }
    pub fn read(&mut self, n: usize) -> Result<usize> {
        if n > 32 {
            return Err("oversized MTX integer");
        }
        let mut v = 0;
        for _ in 0..n {
            v = v * 2 + self.bit()?;
        }
        Ok(v)
    }
    pub fn magnitude(&mut self) -> Result<i32> {
        let mut n = 0i32;
        while self.bit()? != 0 {
            n += 1;
            if n > 65535 {
                return Err("oversized MTX metric delta");
            }
        }
        Ok(if n != 0 && self.bit()? != 0 { -n } else { n })
    }
    pub fn consumed(&self) -> usize {
        self.pos.div_ceil(8)
    }
}

pub(super) fn at_u16(b: &[u8], offset: usize) -> Result<u16> {
    Reader::new(b.get(offset..).ok_or("short font table")?).u16()
}
pub(super) fn put16(b: &mut [u8], offset: usize, n: u16) -> Result<()> {
    b.get_mut(offset..offset.checked_add(2).ok_or("font offset overflow")?).ok_or("short font table")?.copy_from_slice(&n.to_be_bytes());
    Ok(())
}
pub(super) fn put32(b: &mut [u8], offset: usize, n: u32) -> Result<()> {
    b.get_mut(offset..offset.checked_add(4).ok_or("font offset overflow")?).ok_or("short font table")?.copy_from_slice(&n.to_be_bytes());
    Ok(())
}
