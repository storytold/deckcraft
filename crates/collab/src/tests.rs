//! Replicas converge: random edits on two and three peers, updates delivered late, out of order
//! and twice, end with every peer showing the same presentation, and each peer's document reading
//! back as its model.

use std::sync::Arc;

use deckcraft_model::text::{RunProps, TextBody};
use deckcraft_model::{Fill, Geom, Presentation, Shape, ShapeId, ShapeKind, Slide, SlideId, Xfrm, edit};

use crate::{Replica, Rng, fingerprint};

struct Peer {
    rep: Replica,
    doc: Arc<Presentation>,
    inbox: Vec<Vec<u8>>,
}

impl Peer {
    fn refresh(&mut self) {
        match self.rep.refresh(&self.doc) {
            Ok(Some(r)) => self.doc = Arc::new(r.doc),
            Ok(None) => {}
            Err(e) => panic!("refresh: {e}"),
        }
    }
    fn commit(&mut self, next: Presentation) -> Option<Vec<u8>> {
        self.doc = Arc::new(next);
        self.rep.commit(&self.doc).expect("commit")
    }
}

fn start() -> Presentation {
    let mut p = Presentation::default();
    for i in 0..3 {
        let layout = p.masters[0].layouts[1].id;
        let mut s = deckcraft_model::defaults::new_slide(&mut p, layout).expect("slide");
        if let Some(t) = s.shapes.get_mut(0).and_then(|sh| sh.text.as_mut()) {
            *t = TextBody::from_text(&format!("Slide {i}"));
        }
        p.slides.push(Arc::new(s));
    }
    p
}

fn peers(n: usize, seed: u64) -> Vec<Peer> {
    let mut out = Vec::new();
    let mut a = Replica::new(seed * 10 + 1);
    let doc = Arc::new(start());
    let full = a.seed(doc.clone(), "Deck").expect("seed");
    let adoc = a.base().cloned().expect("base");
    out.push(Peer { rep: a, doc: adoc, inbox: vec![] });
    for i in 1..n {
        let mut r = Replica::new(seed * 10 + 1 + i as u64);
        r.apply_update(&full).expect("join");
        let mut p = Peer { rep: r, doc: Arc::new(Presentation::default()), inbox: vec![] };
        p.refresh();
        assert!(p.rep.is_ready());
        out.push(p);
    }
    out
}

fn new_id(p: &mut Presentation) -> u32 {
    p.alloc_id().expect("id")
}

/// A random edit, as a command would make it.
fn edit_once(p: &Presentation, rng: &mut Rng) -> Presentation {
    let mut p = p.clone();
    let ns = p.slides.len();
    let pick = |rng: &mut Rng, n: usize| rng.range(0, n.max(1) as u64) as usize;
    match rng.range(0, 12) {
        0 => {
            let layout = p.masters[0].layouts[0].id;
            if let Some(s) = deckcraft_model::defaults::new_slide(&mut p, layout) {
                let at = pick(rng, ns + 1).min(ns);
                p.slides.insert(at, Arc::new(s));
            }
        }
        1 if ns > 1 => {
            p.slides.remove(pick(rng, ns));
        }
        2 if ns > 1 => {
            let s = p.slides.remove(pick(rng, ns));
            let at = pick(rng, ns);
            p.slides.insert(at.min(p.slides.len()), s);
        }
        3 if ns > 0 => {
            let id = new_id(&mut p);
            let i = pick(rng, ns);
            let x = rng.range(0, 800) as f64;
            let s = Arc::make_mut(&mut p.slides[i]);
            s.shapes.push(Shape {
                id: ShapeId(id),
                name: format!("Box {id}"),
                xfrm: Some(Xfrm::new(x, 10.0, 100.0, 50.0)),
                geom: Geom::preset("rect"),
                text: Some(TextBody::from_text("box")),
                ..Default::default()
            });
        }
        4 if ns > 0 => {
            let i = pick(rng, ns);
            let s = Arc::make_mut(&mut p.slides[i]);
            if !s.shapes.is_empty() {
                let j = pick(rng, s.shapes.len());
                s.shapes.remove(j);
            }
        }
        5..=7 if ns > 0 => {
            // Typing somewhere in a shape's text.
            let i = pick(rng, ns);
            let s = Arc::make_mut(&mut p.slides[i]);
            let n = s.shapes.len();
            if let Some(sh) = s.shapes.get_mut(pick(rng, n))
                && let Some(t) = sh.text.as_mut()
            {
                if t.paragraphs.is_empty() {
                    t.paragraphs.push(Default::default());
                }
                let pi = pick(rng, t.paragraphs.len());
                let len = t.paragraphs[pi].char_len();
                let at = (pi, pick(rng, len + 1));
                let what = ["a", "é", "𝄞", "\n", "xy", "\u{b}"][pick(rng, 6)];
                edit::insert(t, at, what, None);
            }
        }
        8 if ns > 0 => {
            // Deleting a range, or formatting it.
            let i = pick(rng, ns);
            let s = Arc::make_mut(&mut p.slides[i]);
            let n = s.shapes.len();
            if let Some(sh) = s.shapes.get_mut(pick(rng, n))
                && let Some(t) = sh.text.as_mut()
                && !t.paragraphs.is_empty()
            {
                let end = edit::end(t);
                let a = (pick(rng, end.0 + 1), 0);
                let b = end;
                if rng.range(0, 2) == 0 {
                    edit::delete(t, a, (a.0, (a.1 + 2).min(t.paragraphs.get(a.0).map(|p| p.char_len()).unwrap_or(0))));
                } else {
                    let on = rng.range(0, 2) == 0;
                    edit::format(t, a, b, &|r: &mut RunProps| r.bold = Some(on));
                }
            }
        }
        9 if ns > 0 => {
            // Moving or filling a shape.
            let i = pick(rng, ns);
            let s = Arc::make_mut(&mut p.slides[i]);
            let n = s.shapes.len();
            if let Some(sh) = s.shapes.get_mut(pick(rng, n)) {
                if rng.range(0, 2) == 0 {
                    sh.xfrm = Some(Xfrm::new(rng.range(0, 900) as f64, rng.range(0, 500) as f64, 120.0, 60.0));
                } else {
                    sh.fill = Some(Fill::Solid { color: deckcraft_model::ColorRef::rgb(deckcraft_model::Rgba::rgb(rng.range(0, 255) as u8, 0, 0)) });
                }
            }
        }
        10 if ns > 0 => {
            // Notes.
            let i = pick(rng, ns);
            let s = Arc::make_mut(&mut p.slides[i]);
            let len = edit::end(&s.notes);
            edit::insert(&mut s.notes, len, "n", None);
        }
        11 if ns > 0 => {
            // Grouping the first two shapes of a slide, or reordering them.
            let i = pick(rng, ns);
            let gid = new_id(&mut p);
            let s = Arc::make_mut(&mut p.slides[i]);
            if s.shapes.len() >= 2 {
                if rng.range(0, 2) == 0 {
                    let kids: Vec<Shape> = s.shapes.drain(0..2).collect();
                    s.shapes.insert(
                        0,
                        Shape {
                            id: ShapeId(gid),
                            name: "Group".into(),
                            xfrm: Some(Xfrm::new(0.0, 0.0, 10.0, 10.0)),
                            kind: ShapeKind::Group { children: kids, child: Xfrm::new(0.0, 0.0, 10.0, 10.0) },
                            ..Default::default()
                        },
                    );
                } else {
                    let first = s.shapes.remove(0);
                    s.shapes.push(first);
                }
            }
        }
        _ => {}
    }
    p
}

fn converge(n: usize, seed: u64, steps: usize) {
    let mut rng = Rng::seeded(seed);
    let mut ps = peers(n, seed);
    for _ in 0..steps {
        let who = rng.range(0, n as u64) as usize;
        // Deliver some of what waits, possibly out of order, possibly twice.
        for (k, peer) in ps.iter_mut().enumerate() {
            if k == who || rng.range(0, 3) == 0 {
                let mut inbox = std::mem::take(&mut peer.inbox);
                if rng.range(0, 4) == 0 {
                    inbox.reverse();
                }
                for u in &inbox {
                    peer.rep.apply_update(u).expect("apply");
                    if rng.range(0, 8) == 0 {
                        peer.rep.apply_update(u).expect("apply twice");
                    }
                }
                peer.refresh();
            }
        }
        let next = edit_once(&ps[who].doc, &mut rng);
        if let Some(u) = ps[who].commit(next) {
            for (k, peer) in ps.iter_mut().enumerate() {
                if k != who {
                    peer.inbox.push(u.clone());
                }
            }
        }
    }
    // Everything arrives; held updates are asked for again with a sync.
    for _ in 0..3 {
        for p in ps.iter_mut() {
            let inbox = std::mem::take(&mut p.inbox);
            for u in inbox {
                p.rep.apply_update(&u).expect("apply");
            }
            p.refresh();
        }
        for k in 0..n {
            for j in 0..n {
                if j != k {
                    let sv = ps[j].rep.state_vector();
                    let diff = ps[k].rep.diff_for(&sv).expect("diff");
                    ps[j].rep.apply_update(&diff).expect("sync");
                    ps[j].refresh();
                }
            }
        }
    }
    let f0 = fingerprint(&ps[0].doc);
    for (k, p) in ps.iter().enumerate() {
        assert!(!p.rep.has_gaps(), "seed {seed}: peer {k} still waits for updates");
        let fp = fingerprint(&p.doc);
        if fp != f0 {
            let (a, b) = (f0.lines().next().unwrap_or(""), fp.lines().next().unwrap_or(""));
            let at = a.bytes().zip(b.bytes()).position(|(x, y)| x != y).unwrap_or(0);
            panic!(
                "seed {seed}: peer {k} differs near …{}… vs …{}…",
                &a[at.saturating_sub(200)..(at + 200).min(a.len())],
                &b[at.saturating_sub(200)..(at + 200).min(b.len())]
            );
        }
        assert_eq!(fingerprint(&p.rep.debug_build(&p.doc)), fp, "seed {seed}: peer {k}'s document reads back as its model");
    }
}

#[test]
fn two_peers_converge() {
    let seeds: u64 = std::env::var("DECKCRAFT_FUZZ_SEEDS").ok().and_then(|s| s.parse().ok()).unwrap_or(12);
    for seed in 0..seeds {
        converge(2, seed, 60);
    }
}

#[test]
fn three_peers_converge() {
    for seed in 100..106 {
        converge(3, seed, 60);
    }
}

#[test]
fn seeding_and_reading_back_keep_everything() {
    let mut a = Replica::new(7);
    let doc = Arc::new(start());
    let full = a.seed(doc.clone(), "Deck").expect("seed");
    let mut b = Replica::new(8);
    b.apply_update(&full).expect("apply");
    let got = b.refresh(&Arc::new(Presentation::default())).expect("refresh").expect("built").doc;
    let mut want = (*doc).clone();
    want.next_id = got.next_id;
    assert_eq!(fingerprint(&got), fingerprint(&want));
    assert_eq!(b.title().as_deref(), Some("Deck"));
    assert!(got.next_id >= 1 << 24, "a random id counter: {}", got.next_id);
}

#[test]
fn concurrent_typing_in_one_shape_merges_and_undo_is_per_user() {
    let mut ps = peers(2, 3);
    let slide = ps[0].doc.slides[1].id;
    let shape = ps[0].doc.slides[1].shapes[0].id;
    let typed = |doc: &Presentation, at: (usize, usize), what: &str| {
        let mut p = doc.clone();
        let s = p.slide_mut(slide).expect("slide");
        let t = s.shape_mut(shape).and_then(|sh| sh.text.as_mut()).expect("text");
        edit::insert(t, at, what, None);
        p
    };
    let ua = {
        let n = typed(&ps[0].doc, (0, 0), "A-");
        ps[0].commit(n)
    }
    .expect("update");
    let ub = {
        let n = typed(&ps[1].doc, (0, 7), "-B");
        ps[1].commit(n)
    }
    .expect("update");
    assert!(ua.len() < 200 && ub.len() < 200, "typing sends the characters, not the text: {} {}", ua.len(), ub.len());
    ps[0].rep.apply_update(&ub).expect("apply");
    ps[1].rep.apply_update(&ua).expect("apply");
    ps[0].refresh();
    ps[1].refresh();
    let text = |p: &Peer| p.doc.slide(slide).and_then(|s| s.shape(shape)).and_then(|s| s.text.as_ref()).map(|t| t.text()).unwrap_or_default();
    assert_eq!(text(&ps[0]), "A-Slide 0-B");
    assert_eq!(text(&ps[1]), "A-Slide 0-B");
    // A's undo takes back A's typing only.
    let undo = ps[0].rep.undo().expect("undo").expect("an update");
    ps[0].refresh();
    ps[1].rep.apply_update(&undo).expect("apply");
    ps[1].refresh();
    assert_eq!(text(&ps[0]), "Slide 0-B");
    assert_eq!(text(&ps[1]), "Slide 0-B");
    assert!(ps[0].rep.can_redo());
}

#[test]
fn concurrent_moves_of_different_shapes_both_stay() {
    let mut ps = peers(2, 4);
    let slide = ps[0].doc.slides[0].id;
    let (s0, s1) = (ps[0].doc.slides[0].shapes[0].id, ps[0].doc.slides[0].shapes[1].id);
    let moved = |doc: &Presentation, id: ShapeId, x: f64| {
        let mut p = doc.clone();
        p.slide_mut(slide).and_then(|s| s.shape_mut(id)).expect("shape").xfrm = Some(Xfrm::new(x, 1.0, 2.0, 3.0));
        p
    };
    let ua = {
        let n = moved(&ps[0].doc, s0, 11.0);
        ps[0].commit(n)
    }
    .expect("update");
    let ub = {
        let n = moved(&ps[1].doc, s1, 22.0);
        ps[1].commit(n)
    }
    .expect("update");
    ps[0].rep.apply_update(&ub).expect("apply");
    ps[1].rep.apply_update(&ua).expect("apply");
    ps[0].refresh();
    ps[1].refresh();
    for p in &ps {
        let s = p.doc.slide(slide).expect("slide");
        assert_eq!(s.shape(s0).and_then(|x| x.xfrm).map(|x| x.x), Some(11.0));
        assert_eq!(s.shape(s1).and_then(|x| x.xfrm).map(|x| x.x), Some(22.0));
    }
}

#[test]
fn text_positions_follow_others_edits() {
    let mut ps = peers(2, 5);
    let slide = ps[0].doc.slides[1].id.0;
    let shape = ps[0].doc.slides[1].shapes[0].id.0;
    let sticky = ps[1].rep.sticky(slide, Some(shape), (0, 5)).expect("sticky");
    let mut p = (*ps[0].doc).clone();
    let t = p.slide_mut(SlideId(slide)).and_then(|s| s.shape_mut(ShapeId(shape))).and_then(|s| s.text.as_mut()).expect("text");
    edit::insert(t, (0, 0), "123", None);
    let u = ps[0].commit(p).expect("update");
    ps[1].rep.apply_update(&u).expect("apply");
    ps[1].refresh();
    assert_eq!(ps[1].rep.resolve(slide, Some(shape), &sticky), Some((0, 8)));
    // A selection through awareness.
    let at = crate::TextAt { shape: Some(shape), notes: false, anchor: (0, 1), head: (0, 4) };
    let sel = ps[1].rep.selection_to_ids(slide, &[shape], Some(&at));
    let (index, shapes, text) = ps[0].rep.selection_from_ids(&sel).expect("selection");
    assert_eq!((index, shapes), (1, vec![shape]));
    assert_eq!(text, Some(at));
}

#[test]
fn hostile_updates_are_refused_not_fatal() {
    let mut a = Replica::new(1);
    a.seed(Arc::new(start()), "x").expect("seed");
    assert!(a.apply_update(&[0xff; 40]).is_err());
    assert!(a.apply_update(&[]).is_err());
    // Another app's meta.
    let other = yrs::Doc::with_client_id(99);
    let u = {
        use yrs::{Map, Transact, WriteTxn};
        let mut t = other.transact_mut();
        let m = t.get_or_insert_map("meta");
        m.insert(&mut t, "app", "wordcraft");
        m.insert(&mut t, "schema", 1);
        t.encode_update_v1()
    };
    assert!(matches!(a.apply_update(&u), Err(crate::CollabError::Mismatch { .. })));
    // Junk under the right names: a slide list with numbers, a slide root with garbage.
    let junk = yrs::Doc::with_client_id(98);
    let u = {
        use yrs::{Array, Map, Transact, WriteTxn};
        let mut t = junk.transact_mut();
        let arr = t.get_or_insert_array("slides");
        arr.push_back(&mut t, 5);
        arr.push_back(&mut t, "zzzzzzzz");
        arr.push_back(&mut t, "00000001");
        let s = t.get_or_insert_map("slide:00000001");
        s.insert(&mut t, "shapes", "not an array");
        s.insert(&mut t, "hidden", "{not json");
        s.insert(&mut t, "shape:00000002", 7);
        t.encode_update_v1()
    };
    let base = a.base().cloned().expect("base");
    a.apply_update(&u).expect("applies");
    let r = a.refresh(&base).expect("refresh").expect("changed");
    assert!(r.doc.slides.iter().any(|s| s.id == SlideId(1)));
}

#[test]
fn ids() {
    assert_eq!(crate::id_str(5), "00000005");
    assert_eq!(crate::id_str(u32::MAX), "01z141z3");
    assert_eq!(crate::parse_id("01z141z3"), Some(u32::MAX));
    assert_eq!(crate::parse_id("k3v9x0qa"), None, "beyond 32 bits");
    assert_eq!(crate::parse_id("00000000"), None);
    assert_eq!(crate::parse_id("0000000A"), None);
    let mut p = start();
    crate::reseed_ids(&mut p, &mut Rng::seeded(1));
    assert!(p.next_id >= 1 << 24 && p.next_id < 1 << 31);
    let _ = Slide::default();
}
