//! Complete typed rich-comment resource closure, shared by file and engine boundaries.
use super::{Result, bad};
use crate::{
    Comment, MediaId, SlideId,
    style::Fill,
    text::{Action, Bullet, Hyperlink, ParaProps, RunProps, TextBody},
};
use std::collections::BTreeMap;
macro_rules! media_walk {
 ($name:ident $(,$m:ident)?)=>{pub mod $name {use super::*;
 fn fill(value:&$($m)? Option<Fill>,f:&mut dyn FnMut(&$($m)? MediaId)){if let Some(Fill::Picture(p))=value{f(&$($m)? p.media);if let Some(a)=&$($m)? p.artistic{f(&$($m)? a.original);}}}
 fn link(value:&$($m)? Option<Hyperlink>,f:&mut dyn FnMut(&$($m)? MediaId)){if let Some(h)=value{if let Some(id)=&$($m)? h.sound{f(id);}}}
 fn run(value:&$($m)? RunProps,f:&mut dyn FnMut(&$($m)? MediaId)){fill(&$($m)? value.fill,f);if let Some(line)=&$($m)? value.outline{fill(&$($m)? line.fill,f);}link(&$($m)? value.link,f);link(&$($m)? value.hover,f);}
 fn para(value:&$($m)? ParaProps,f:&mut dyn FnMut(&$($m)? MediaId)){fill(&$($m)? value.background,f);if let Some(Bullet::Picture{media})=&$($m)? value.bullet{f(media);}}
 pub fn body(value:&$($m)? TextBody,f:&mut dyn FnMut(&$($m)? MediaId)){for level in (&$($m)? value.list_style.levels).into_iter().flatten(){para(&$($m)? level.para,f);run(&$($m)? level.run,f);}for p in &$($m)? value.paragraphs{para(&$($m)? p.props,f);run(&$($m)? p.def_props,f);run(&$($m)? p.end_props,f);for r in &$($m)? p.runs{run(&$($m)? r.props,f);}}}
 pub fn comments(items:&$($m)? [Comment],f:&mut dyn FnMut(&$($m)? MediaId)){for c in items{if let Some(value)=&$($m)? c.body{body(value,f);}for a in &$($m)? c.attachments{f(&$($m)? a.media);}comments(&$($m)? c.replies,f);}}
 }};
}
media_walk!(visit);
media_walk!(visit_mut, mut);
pub fn map_media(items: &mut [Comment], map: &BTreeMap<MediaId, MediaId>) -> Result<()> {
    let mut missing = false;
    visit_mut::comments(items, &mut |id| {
        if let Some(new) = map.get(id) {
            *id = *new;
        } else {
            missing = true;
        }
    });
    if missing {
        return Err(bad("rich comment media has no verified binding"));
    }
    Ok(())
}
pub fn map_slides(items: &mut [Comment], map: &BTreeMap<SlideId, SlideId>) -> Result<()> {
    fn link(h: &mut Option<Hyperlink>, map: &BTreeMap<SlideId, SlideId>) -> Result<()> {
        if let Some(h) = h {
            if let Action::Slide { slide } = &mut h.action {
                *slide = *map.get(slide).ok_or_else(|| bad("rich comment slide target has no verified binding"))?;
            }
            if let Some(slide) = &mut h.zoom_return {
                *slide = *map.get(slide).ok_or_else(|| bad("rich comment zoom target has no verified binding"))?;
            }
        }
        Ok(())
    }
    fn run(r: &mut RunProps, map: &BTreeMap<SlideId, SlideId>) -> Result<()> {
        link(&mut r.link, map)?;
        link(&mut r.hover, map)
    }
    for c in items {
        if let Some(body) = &mut c.body {
            for level in body.list_style.levels.iter_mut().flatten() {
                run(&mut level.run, map)?;
            }
            for p in &mut body.paragraphs {
                run(&mut p.def_props, map)?;
                run(&mut p.end_props, map)?;
                for r in &mut p.runs {
                    run(&mut r.props, map)?;
                }
            }
        }
        map_slides(&mut c.replies, map)?;
    }
    Ok(())
}
