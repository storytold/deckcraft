use super::*;

/// Read media bytes from `data` (base64) or `path`.
pub(crate) fn media_bytes(p: &Value, cmd: &str) -> Result<(String, Vec<u8>)> {
    if let Some(d) = str_param(p, "data") {
        let bytes = base64_decode(d).ok_or_else(|| bad(cmd, "invalid base64 `data`"))?;
        return Ok((str_param(p, "name").unwrap_or("media").to_string(), bytes));
    }
    if let Some(path) = str_param(p, "path") {
        let bytes = crate::io::read_file(path).map_err(EngineError::Other)?;
        let name = std::path::Path::new(path).file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_else(|| "media".into());
        return Ok((name, bytes));
    }
    Err(bad(cmd, "missing `path` or `data`"))
}

pub fn content_type(name: &str, bytes: &[u8]) -> &'static str {
    let l = name.to_ascii_lowercase();
    if bytes.starts_with(&[0x89, b'P', b'N', b'G']) {
        "image/png"
    } else if bytes.starts_with(&[0xFF, 0xD8]) {
        "image/jpeg"
    } else if bytes.starts_with(b"GIF8") {
        "image/gif"
    } else if bytes.starts_with(b"RIFF") && bytes.get(8..12) == Some(b"WEBP") {
        "image/webp"
    } else if bytes.starts_with(b"RIFF") && bytes.get(8..12) == Some(b"WAVE") {
        "audio/wav"
    } else if bytes.starts_with(b"BM") {
        "image/bmp"
    } else if bytes.starts_with(b"II*\0") || bytes.starts_with(b"MM\0*") {
        "image/tiff"
    } else if bytes.starts_with(b"fLaC") {
        "audio/flac"
    } else if bytes.starts_with(b"OggS") {
        if l.ends_with(".ogv") { "video/ogg" } else { "audio/ogg" }
    } else if bytes.len() > 1 && bytes[0] == 0xFF && bytes[1] & 0xF6 == 0xF0 {
        // ADTS (layer bits 00): AAC.
        "audio/aac"
    } else if bytes.starts_with(b"ID3") || (bytes.len() > 1 && bytes[0] == 0xFF && (bytes[1] & 0xE0) == 0xE0 && !l.ends_with(".aac")) {
        "audio/mpeg"
    } else if bytes.starts_with(b"FORM") {
        "audio/aiff"
    } else if bytes.get(4..8) == Some(b"ftyp") {
        if l.ends_with(".m4a") || l.ends_with(".m4b") || bytes.get(8..11) == Some(b"M4A") {
            "audio/mp4"
        } else if l.ends_with(".mov") || bytes.get(8..12) == Some(b"qt  ") {
            "video/quicktime"
        } else {
            "video/mp4"
        }
    } else if bytes.starts_with(&[0x1A, 0x45, 0xDF, 0xA3]) {
        if l.ends_with(".mka") || l.ends_with(".weba") {
            "audio/webm"
        } else if l.ends_with(".mkv") {
            "video/x-matroska"
        } else {
            "video/webm"
        }
    } else if bytes.starts_with(&[0x30, 0x26, 0xB2, 0x75, 0x8E, 0x66, 0xCF, 0x11]) {
        if l.ends_with(".wmv") || l.ends_with(".asf") { "video/x-ms-wmv" } else { "audio/x-ms-wma" }
    } else if bytes.starts_with(b"caff") {
        "audio/x-caf"
    } else if l.ends_with(".svg") || bytes.windows(4).take(512).any(|w| w == b"<svg") {
        "image/svg+xml"
    } else if l.ends_with(".aac") {
        "audio/aac"
    } else {
        "application/octet-stream"
    }
}

/// Natural size of an image in points (96 dpi), fitted inside the slide.
fn fitted(s: &Session, bytes: &[u8]) -> (f64, f64) {
    let (w, h) = image::ImageReader::new(std::io::Cursor::new(bytes))
        .with_guessed_format()
        .ok()
        .and_then(|r| r.into_dimensions().ok())
        .map(|(w, h)| (w as f64 * 0.75, h as f64 * 0.75))
        .unwrap_or((288.0, 216.0));
    let size = s.active().map(|d| d.doc.slide_size).unwrap_or(deckcraft_model::defaults::WIDE);
    let k = (size.width * 0.9 / w.max(1.0)).min(size.height * 0.9 / h.max(1.0)).min(1.0);
    (w * k, h * k)
}

pub(super) fn picture(s: &mut Session, p: &Value) -> Result<Value> {
    let (name, bytes) = media_bytes(p, "insert.picture")?;
    let ct = content_type(&name, &bytes);
    if !ct.starts_with("image/") {
        return Err(bad("insert.picture", format!("{name} is not a picture")));
    }
    if ct != "image/svg+xml" && deckcraft_render::decode_image(&bytes).is_none() {
        return Err(EngineError::Other(format!("{name}: the picture can't be read")));
    }
    let (w, h) = fitted(s, &bytes);
    // Into a selected empty picture/content placeholder, if there is one.
    let target_ph = s.active().and_then(|d| {
        d.selected_shapes()
            .into_iter()
            .find(|x| {
                x.ph_type()
                    .is_some_and(|k| matches!(k, deckcraft_model::PhType::Picture | deckcraft_model::PhType::Obj | deckcraft_model::PhType::Body))
                    && x.text.as_ref().is_none_or(|t| t.is_empty())
            })
            .map(|x| (x.id, xfrm_of(&d.doc, &d.selection, x)))
    });
    let rect = rect_param(p, "rect").unwrap_or_else(|| match target_ph {
        Some((_, b)) => {
            // Fill the placeholder, cropping the picture to its aspect.
            b
        }
        None => default_rect(s, w, h),
    });
    let crop = match (target_ph, rect_param(p, "rect")) {
        (Some(_), None) => {
            let (ia, ba) = (w / h.max(1e-9), rect.w / rect.h.max(1e-9));
            if ia > ba {
                let c = (1.0 - ba / ia) / 2.0;
                [c, 0.0, c, 0.0]
            } else {
                let c = (1.0 - ia / ba) / 2.0;
                [0.0, c, 0.0, c]
            }
        }
        _ => [0.0; 4],
    };
    let media = s.edit(|doc, _| doc.add_media(&name, ct, bytes).ok_or_else(|| bad("insert.picture", NO_ID_SPACE)))?;
    let shape = Shape {
        xfrm: Some(rect),
        kind: ShapeKind::Picture { fill: PictureFill { media, crop, ..Default::default() } },
        descr: name.clone(),
        ..Default::default()
    };
    if let Some((ph_id, _)) = target_ph {
        // Replace the placeholder with the picture (it keeps the placeholder link).
        let phv = s.doc()?.shape(ph_id).and_then(|x| x.ph.clone());
        s.edit(|doc, sel| {
            let list = crate::shapes_mut(doc, sel).ok_or_else(|| bad("insert.picture", "no slide"))?;
            if let Some(sh) = list.iter_mut().find(|x| x.id == ph_id) {
                sh.kind = shape.kind.clone();
                sh.xfrm = Some(rect);
                sh.ph = phv.map(|mut p| {
                    p.kind = deckcraft_model::PhType::Picture;
                    p
                });
                sh.text = None;
                sh.descr = name.clone();
            }
            sel.shapes = vec![ph_id];
            Ok(())
        })?;
        return Ok(json!({"id": ph_id}));
    }
    let id = add_shape(s, "insert.picture", shape, true)?;
    Ok(json!({"id": id}))
}

fn media(s: &mut Session, p: &Value, video: bool) -> Result<Value> {
    let cmd = if video { "insert.video" } else { "insert.audio" };
    let (name, bytes) = media_bytes(p, cmd)?;
    let ct = content_type(&name, &bytes);
    let bytes = std::sync::Arc::new(bytes);
    let info = deckcraft_media::probe(&bytes);
    let has_video = info.as_ref().is_ok_and(|i| i.video.is_some());
    let has_audio = info.as_ref().is_ok_and(|i| i.audio.is_some());
    match &info {
        Ok(_) if video && !has_video => return Err(bad(cmd, format!("{name} has no video"))),
        Ok(_) if !video && !has_audio && !has_video => return Err(bad(cmd, format!("{name} has no audio"))),
        // Not something we can read: embed it anyway when the type says it's media (it still
        // round-trips through PPTX), otherwise refuse.
        Err(_) if video && !ct.starts_with("video/") => return Err(bad(cmd, format!("{name} is not a supported video ({ct})"))),
        Err(_) if !video && !ct.starts_with("audio/") => return Err(bad(cmd, format!("{name} is not a supported audio file ({ct})"))),
        _ => {}
    }
    let (duration_ms, width, height, playable) = match &info {
        Ok(i) => {
            let (w, h) = i.video.as_ref().map(|v| (v.width, v.height)).unwrap_or((0, 0));
            (i.duration_ms.min(u32::MAX as u64) as u32, w, h, i.playable())
        }
        Err(_) => (0, 0, 0, false),
    };
    let decodable_video = info.as_ref().is_ok_and(|i| i.video.as_ref().is_some_and(|v| v.decodable));
    let warning = if !playable {
        Some(match &info {
            Ok(i) => {
                let codecs: Vec<&str> = i.audio.iter().map(|a| a.codec.as_str()).chain(i.video.iter().map(|v| v.codec.as_str())).collect();
                format!("{name} was inserted, but it can't be played ({} isn't supported yet)", codecs.join(" / "))
            }
            Err(e) => format!("{name} was inserted, but it can't be played ({e})"),
        })
    } else if video && !decodable_video {
        // A video codec without a decoder (VP8, say): the clip's audio still plays, but no frame
        // can ever show — say so instead of leaving an unexplained black box (#1237).
        info.as_ref()
            .ok()
            .and_then(|i| i.video.as_ref())
            .map(|v| format!("{name} was inserted, but its video can't be played ({} isn't supported yet); only the audio plays", v.codec))
    } else {
        None
    };
    let poster_png = if video && decodable_video { crate::cmd::media::poster_png(&bytes, 0) } else { None };
    let size = s.doc()?.doc.slide_size;
    let rect = rect_param(p, "rect").unwrap_or_else(|| {
        if video {
            // Natural size (96 dpi), fitted inside 90 % of the slide and centred.
            let (w, h) =
                if width > 0 && height > 0 { (width as f64 * 0.75, height as f64 * 0.75) } else { (size.width * 0.6, size.width * 0.6 * 9.0 / 16.0) };
            let k = (size.width * 0.9 / w).min(size.height * 0.9 / h).min(1.0);
            let (w, h) = (w * k, h * k);
            Xfrm::new((size.width - w) / 2.0, (size.height - h) / 2.0, w, h)
        } else {
            Xfrm::new((size.width - 48.0) / 2.0, (size.height - 48.0) / 2.0, 48.0, 48.0)
        }
    });
    let data = std::sync::Arc::try_unwrap(bytes).unwrap_or_else(|a| (*a).clone());
    let (media, poster) = s.edit(|doc, _| {
        let media = doc.add_media(&name, ct, data).ok_or_else(|| bad("insert.media", NO_ID_SPACE))?;
        let poster = match poster_png {
            Some(png) => Some(doc.add_media(&format!("{name}-poster.png"), "image/png", png).ok_or_else(|| bad("insert.media", NO_ID_SPACE))?),
            None => None,
        };
        Ok((media, poster))
    })?;
    let clip = MediaClip { media, video, poster, volume: 1.0, duration_ms, width, height, ..Default::default() };
    let id = add_shape(s, "insert.media", Shape { xfrm: Some(rect), kind: ShapeKind::Media(clip), descr: name, ..Default::default() }, true)?;
    if let Some(w) = &warning {
        s.ui_requests.push(crate::UiRequest::Message { text: w.clone() });
    }
    let mut r =
        json!({"id": id, "contentType": ct, "durationMs": duration_ms, "width": width, "height": height, "playable": playable, "warning": warning});
    if let Ok(i) = &info {
        r["probe"] = crate::cmd::media::probe_json(i);
    }
    Ok(r)
}

pub(super) fn audio(s: &mut Session, p: &Value) -> Result<Value> {
    media(s, p, false)
}

pub(super) fn video(s: &mut Session, p: &Value) -> Result<Value> {
    media(s, p, true)
}
