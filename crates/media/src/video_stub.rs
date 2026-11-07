//! Video API available when the optional codec feature is disabled.

use crate::{Bytes, MediaError, Result};

#[derive(Clone, Debug, PartialEq)]
pub struct Frame {
    pub time: f64,
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

pub struct VideoDecoder;

impl VideoDecoder {
    pub fn open(_: Bytes) -> Result<VideoDecoder> {
        Err(MediaError::Unsupported("video decoding (enable the `video` feature)".into()))
    }
    pub fn size(&self) -> (u32, u32) {
        (0, 0)
    }
    pub fn codec(&self) -> &str {
        ""
    }
    pub fn frame_count(&self) -> usize {
        0
    }
    pub fn next_frame(&mut self) -> Result<Option<Frame>> {
        Err(MediaError::Unsupported("video decoding (enable the `video` feature)".into()))
    }
    pub fn seek(&mut self, _: f64) {}
}

pub fn poster_frame(_: &Bytes, _: f64) -> Result<Frame> {
    Err(MediaError::Unsupported("video decoding (enable the `video` feature)".into()))
}

pub fn poster_png(_: &Bytes, _: f64) -> Result<Vec<u8>> {
    Err(MediaError::Unsupported("video decoding (enable the `video` feature)".into()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn video_entry_points_report_unsupported_without_the_feature() {
        let bytes = std::sync::Arc::new(Vec::new());
        assert!(matches!(VideoDecoder::open(bytes.clone()), Err(MediaError::Unsupported(_))));
        assert!(matches!(poster_frame(&bytes, 0.0), Err(MediaError::Unsupported(_))));
        assert!(matches!(poster_png(&bytes, 0.0), Err(MediaError::Unsupported(_))));
        assert!(matches!(crate::VideoFeed::new(bytes), Err(MediaError::Unsupported(_))));
    }
}
