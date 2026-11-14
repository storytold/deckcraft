//! `text.fit`: how text bodies fit their shapes as the slide draws them. An agent filling a
//! placeholder sees `autofit: null` in `shape.inspect` while the layout's shrink-on-overflow draws
//! the text at a fraction of its size; these tests pin what the query reports for that case, for
//! text that fits, for text that overflows a shape that does not shrink, for a slide number field,
//! for members of resized groups, for a body of empty paragraphs, for vertical text, for text set
//! in columns, for text nested deeper than the slide draws and for bad params.

use serde_json::{Value, json};

use crate::Session;

/// Twenty-five bullets, each long enough to wrap, as an agent pasting an outline writes them.
fn long_outline() -> String {
    (1..=25)
        .map(|i| format!("Point {i}: a sentence long enough to wrap across the full width of the content placeholder"))
        .collect::<Vec<_>>()
        .join("\n")
}

fn fit(s: &mut Session, p: Value) -> Value {
    s.execute("text.fit", &p).unwrap()
}

fn body_named<'a>(r: &'a Value, name: &str) -> &'a Value {
    r["bodies"].as_array().unwrap().iter().find(|b| b["name"] == name).unwrap_or_else(|| panic!("no body {name} in {r}"))
}

fn num(b: &Value, key: &str) -> f64 {
    b[key].as_f64().unwrap_or_else(|| panic!("{key} in {b}"))
}

#[test]
fn an_overfull_placeholder_reports_the_shrink_its_layout_applies() {
    let mut s = Session::with_new();
    s.execute("slide.new", &json!({"layout": "titleAndContent", "title": "Agenda", "body": long_outline()})).unwrap();
    let r = fit(&mut s, json!({}));
    assert_eq!(r["index"], 1);
    let b = body_named(&r, "Content Placeholder 2");
    // The shape sets no autofit of its own: the layout's placeholder shrinks the text.
    assert_eq!(b["autofit"], "shrink", "{b}");
    assert_eq!(b["autofitFrom"], "layout", "{b}");
    assert!(num(b, "fontScale") < 0.7, "{b}");
    assert!(num(b, "lineReduction") > 0.0, "{b}");
    assert!(num(b, "minSize") < 20.0, "{b}");
    assert!(num(b, "minSize") <= num(b, "maxSize"), "{b}");
    assert!(b["lines"].as_u64().unwrap() >= 25, "{b}");
    // Shrunk until the drawing's own overflow test passes, which allows half a point.
    assert_eq!(b["overflows"], false, "{b}");
    assert!(num(b, "overflowPt") <= 0.5, "{b}");
    // The default layout's content placeholder is 343 pt tall, less its top and bottom insets.
    assert!((330.0..=343.0).contains(&num(b, "frameHeight")), "{b}");
    // shape.inspect shows no autofit on the shape itself.
    let own = s.execute("shape.inspect", &json!({"id": b["id"]})).unwrap()["shape"]["text"]["body"].clone();
    assert!(own["autofit"].is_null(), "{own}");
}

#[test]
fn text_that_fits_reports_full_size_and_no_overflow() {
    let mut s = Session::with_new();
    s.execute("slide.new", &json!({"layout": "titleAndContent", "title": "Agenda", "body": "One\nTwo"})).unwrap();
    let r = fit(&mut s, json!({}));
    let b = body_named(&r, "Content Placeholder 2");
    assert_eq!(b["autofit"], "shrink", "{b}");
    assert_eq!(b["fontScale"], 1.0, "{b}");
    assert_eq!(b["lineReduction"], 0.0, "{b}");
    assert_eq!(b["lines"], 2, "{b}");
    assert_eq!(b["overflows"], false, "{b}");
    assert_eq!(b["overflowPt"], 0.0, "{b}");
    assert!(num(b, "minSize") >= 20.0, "{b}");
    assert!(num(b, "contentHeight") < num(b, "frameHeight"), "{b}");
    assert_eq!(body_named(&r, "Title 1")["autofitFrom"], "layout");
}

#[test]
fn text_that_cannot_shrink_reports_how_far_it_overflows() {
    let mut s = Session::with_new();
    s.execute("slide.new", &json!({"layout": "blank"})).unwrap();
    let long = "A sentence that will not fit in a short box. ".repeat(12);
    let id = s.execute("shape.insert", &json!({"preset": "rect", "rect": [100, 100, 200, 40], "text": long})).unwrap()["id"].clone();
    let r = fit(&mut s, json!({"ids": [id]}));
    let bodies = r["bodies"].as_array().unwrap();
    assert_eq!(bodies.len(), 1, "{r}");
    let b = &bodies[0];
    assert_eq!(b["id"], id);
    assert_eq!(b["autofit"], "none", "{b}");
    assert_eq!(b["autofitFrom"], "default", "{b}");
    assert_eq!(b["fontScale"], 1.0, "{b}");
    assert_eq!(b["overflows"], true, "{b}");
    let (content, frame, over) = (num(b, "contentHeight"), num(b, "frameHeight"), num(b, "overflowPt"));
    assert!(content > frame + 20.0, "{b}");
    assert!(over > 20.0 && over <= content, "{b}");
}

#[test]
fn group_members_are_measured_and_groups_and_tables_left_out() {
    let mut s = Session::with_new();
    s.execute("slide.new", &json!({"layout": "blank"})).unwrap();
    let a = s.execute("shape.insert", &json!({"preset": "rect", "rect": [100, 100, 200, 80], "text": "A"})).unwrap()["id"].clone();
    let b = s.execute("shape.insert", &json!({"preset": "rect", "rect": [400, 100, 200, 80], "text": "B"})).unwrap()["id"].clone();
    s.execute("edit.select", &json!({"ids": [a, b]})).unwrap();
    let g = s.execute("arrange.group", &json!({})).unwrap()["id"].clone();
    let t = s.execute("insert.table", &json!({"rows": 2, "cols": 2, "rect": [100, 300, 400, 100]})).unwrap()["id"].clone();
    let r = fit(&mut s, json!({}));
    let ids: Vec<Value> = r["bodies"].as_array().unwrap().iter().map(|x| x["id"].clone()).collect();
    assert_eq!(ids, vec![a, b], "{r}");
    for (id, why) in [(g, "no text body"), (t, "no text body")] {
        let e = s.execute("text.fit", &json!({"ids": [id]})).unwrap_err().to_string();
        assert!(e.contains(why), "{e}");
    }
}

#[test]
fn fitting_reads_any_slide_and_changes_nothing() {
    let mut s = Session::with_new();
    s.execute("slide.new", &json!({"layout": "titleAndContent", "title": "One", "body": long_outline()})).unwrap();
    s.execute("slide.new", &json!({"layout": "blank"})).unwrap();
    let before = {
        let d = s.doc().unwrap();
        (d.history.undo.len(), d.revision, d.selection.slide)
    };
    let r = fit(&mut s, json!({"index": 1}));
    assert_eq!(r["index"], 1);
    assert!(num(body_named(&r, "Content Placeholder 2"), "fontScale") < 0.7, "{r}");
    // The current slide is the blank one: no text bodies.
    assert_eq!(fit(&mut s, json!({}))["bodies"], json!([]));
    let d = s.doc().unwrap();
    assert_eq!((d.history.undo.len(), d.revision, d.selection.slide), before);
}

#[test]
fn bad_slide_indexes_and_shape_ids_are_refused() {
    let mut s = Session::with_new();
    assert_eq!(fit(&mut s, json!({"index": 0}))["index"], 0);
    for p in [
        json!({"index": 1}),
        json!({"index": -1}),
        json!({"index": 0.5}),
        json!({"index": "0"}),
        json!({"ids": "2"}),
        json!({"ids": [-1]}),
        json!({"ids": [1e300]}),
        json!({"ids": [4_294_967_296_u64]}),
        json!({"ids": [99_999]}),
    ] {
        assert!(s.execute("text.fit", &p).is_err(), "{p}");
    }
    assert!(s.execute("text.fit", &json!({"ids": [99_999]})).unwrap_err().to_string().contains("no shape"));
}

/// A slide number field is measured with the number the slide draws. Numbered from 9999, slide 2
/// draws "10000", which wraps onto a line of its own where a one-digit number fits.
#[test]
fn a_slide_number_field_is_measured_with_the_number_the_slide_draws() {
    let mut s = Session::with_new();
    s.execute("slide.new", &json!({"layout": "blank"})).unwrap();
    let id = s.execute("shape.insert", &json!({"preset": "rect", "rect": [100, 100, 164, 120], "text": "Slide number"})).unwrap()["id"].clone();
    s.execute("text.edit", &json!({"id": id})).unwrap();
    s.execute("text.insert", &json!({"text": " "})).unwrap();
    s.execute("insert.slideNumber", &json!({})).unwrap();
    // text.fit's report and the layout the renderer places for the same body.
    let measure = |s: &mut Session| {
        let b = fit(s, json!({"ids": [id]}))["bodies"][0].clone();
        let placed = deckcraft_render::place_slide(&s.doc().unwrap().doc, 1);
        let drawn = placed.texts.into_iter().find(|t| t.body.text().starts_with("Slide number")).expect("the slide draws the body");
        (b, drawn.layout.lines.len(), (drawn.layout.content_height * 10.0).round() / 10.0)
    };
    let (b, lines, height) = measure(&mut s);
    assert_eq!(lines, 1, "slide 2 draws one line");
    assert_eq!(b["lines"], lines, "{b}");
    s.edit(|doc, _| {
        doc.first_slide_number = 9999;
        Ok(())
    })
    .unwrap();
    let (b, wrapped, wrapped_height) = measure(&mut s);
    assert_eq!(wrapped, 2, "slide 10000 draws its number on a second line");
    assert!(wrapped_height > height, "{wrapped_height} after {height}");
    assert_eq!(b["lines"], wrapped, "text.fit {b} against the drawn layout");
    assert_eq!(num(&b, "contentHeight"), wrapped_height, "{b}");
}

/// The renderer sets a group member's text at the member's size on the slide: a group's child
/// coordinates size and place the box, never the type. After `shape.resize` halves a group (its
/// members keep their child coordinates), a member must report what a plain shape of its drawn
/// size reports, and so must a member of a group inside another resized group.
#[test]
fn members_of_resized_groups_are_measured_at_their_size_on_the_slide() {
    let mut s = Session::with_new();
    s.execute("slide.new", &json!({"layout": "blank"})).unwrap();
    let text = "Group members wrap as drawn on the slide";
    let insert = |s: &mut Session, rect: [i64; 4], text: &str| {
        s.execute("shape.insert", &json!({"preset": "rect", "rect": rect, "text": text})).unwrap()["id"].clone()
    };
    let same = |s: &mut Session, member: &Value, plain: &Value| {
        let r = fit(s, json!({"ids": [member, plain]}));
        let (m, p) = (&r["bodies"][0], &r["bodies"][1]);
        for k in ["fontScale", "lineReduction", "minSize", "maxSize", "lines", "contentHeight", "frameHeight", "overflows", "overflowPt"] {
            assert_eq!(m[k], p[k], "{k}: member {m}, plain shape {p}");
        }
        p["overflows"].clone()
    };
    let a = insert(&mut s, [100, 100, 200, 80], text);
    let b = insert(&mut s, [400, 100, 200, 80], "B");
    s.execute("edit.select", &json!({"ids": [a, b]})).unwrap();
    let g = s.execute("arrange.group", &json!({})).unwrap()["id"].clone();
    // A group at the size of its child coordinates draws its members at their own size.
    let own = insert(&mut s, [100, 300, 200, 80], text);
    same(&mut s, &a, &own);
    s.execute("shape.resize", &json!({"ids": [g], "w": 250})).unwrap();
    let half_wide = insert(&mut s, [400, 300, 100, 80], text);
    // At half the width the text needs more lines than the frame holds.
    assert_eq!(same(&mut s, &a, &half_wide), true);
    let c = insert(&mut s, [700, 100, 100, 80], "C");
    s.execute("edit.select", &json!({"ids": [g, c]})).unwrap();
    let outer = s.execute("arrange.group", &json!({})).unwrap()["id"].clone();
    s.execute("shape.resize", &json!({"ids": [outer], "h": 40})).unwrap();
    let half_high = insert(&mut s, [600, 300, 100, 40], text);
    assert_eq!(same(&mut s, &a, &half_high), true);
}

/// The renderer draws no text for a body whose paragraphs are all empty (`TextBody::is_empty`),
/// so `text.fit` reports one as it reports a body with no paragraphs: no lines and no overflow,
/// even in a frame shorter than one line.
#[test]
fn a_body_of_empty_paragraphs_reports_no_lines_and_no_overflow() {
    let mut s = Session::with_new();
    s.execute("slide.new", &json!({"layout": "blank"})).unwrap();
    let e = s.execute("shape.insert", &json!({"preset": "rect", "rect": [100, 450, 200, 10]})).unwrap()["id"].clone();
    let no_paragraphs = fit(&mut s, json!({"ids": [e]}));
    assert_eq!(no_paragraphs["bodies"][0]["lines"], 0, "{no_paragraphs}");
    s.execute("text.set", &json!({"id": e, "text": ""})).unwrap();
    let id = deckcraft_model::ShapeId(e.as_u64().unwrap() as u32);
    let st = s.doc().unwrap();
    let body = deckcraft_model::find_shape(&st.doc.slides[1].shapes, id).unwrap().text.clone().unwrap();
    assert!(body.paragraphs.len() == 1 && body.is_empty(), "{body:?}");
    let r = fit(&mut s, json!({"ids": [e]}));
    assert_eq!(r, no_paragraphs);
    assert_eq!(r["bodies"][0]["overflows"], false, "{r}");
}

/// Puts the slide's shape `id` inside `levels` groups, each holding only the next and sized to the
/// shape, as a saved file can nest them past the 32 levels `arrange.group` builds.
fn nest(s: &mut Session, id: &Value, levels: usize) {
    s.edit(|doc, sel| {
        let ids: Vec<u32> = (0..levels).map(|_| doc.alloc_id().unwrap()).collect();
        let list = crate::shapes_mut(doc, sel).unwrap();
        let at = list.iter().position(|sh| json!(sh.id) == *id).unwrap();
        let mut sh = list.remove(at);
        let x = sh.xfrm.unwrap();
        for g in ids {
            sh = deckcraft_model::Shape {
                id: deckcraft_model::ShapeId(g),
                name: format!("Group {g}"),
                xfrm: Some(x),
                kind: deckcraft_model::ShapeKind::Group { children: vec![sh], child: x },
                ..Default::default()
            };
        }
        list.insert(at, sh);
        Ok(())
    })
    .unwrap();
}

/// The renderer draws shapes nested at most 32 groups deep, and `text.fit` measures the same
/// shapes: a text body 32 groups down is reported, and one 33 groups down is left out of the slide
/// answer and refused by id.
#[test]
fn text_nested_deeper_than_the_slide_draws_is_left_out() {
    let mut s = Session::with_new();
    s.execute("slide.new", &json!({"layout": "blank"})).unwrap();
    let drawn = s.execute("shape.insert", &json!({"preset": "rect", "rect": [100, 100, 200, 80], "text": "Drawn"})).unwrap()["id"].clone();
    let deep = s.execute("shape.insert", &json!({"preset": "rect", "rect": [400, 100, 200, 80], "text": "Too deep"})).unwrap()["id"].clone();
    nest(&mut s, &drawn, 32);
    nest(&mut s, &deep, 33);
    // The slide's placement pass for exports stops where the renderer stops.
    let placed: Vec<Value> = deckcraft_render::place_slide(&s.doc().unwrap().doc, 1).shapes.iter().map(|p| json!(p.id)).collect();
    assert!(placed.contains(&drawn) && !placed.contains(&deep), "{placed:?}");
    let r = fit(&mut s, json!({}));
    let ids: Vec<Value> = r["bodies"].as_array().unwrap().iter().map(|x| x["id"].clone()).collect();
    assert_eq!(ids, vec![drawn.clone()], "{r}");
    assert_eq!(fit(&mut s, json!({"ids": [drawn.clone()]}))["bodies"][0]["id"], drawn);
    let e = s.execute("text.fit", &json!({"ids": [deep]})).unwrap_err().to_string();
    assert!(e.contains("33 groups deep"), "{e}");
}

/// `deckcraft_model::walk` stops 64 groups down, and `ids` found a shape's depth through it, so a
/// shape nested deeper was refused as "no shape". `ids` names the depth however deep a file nests
/// the shape.
#[test]
fn text_nested_past_64_groups_is_refused_with_its_depth() {
    let mut s = Session::with_new();
    s.execute("slide.new", &json!({"layout": "blank"})).unwrap();
    let past_walk =
        s.execute("shape.insert", &json!({"preset": "rect", "rect": [100, 100, 200, 80], "text": "Past the walk"})).unwrap()["id"].clone();
    let far = s.execute("shape.insert", &json!({"preset": "rect", "rect": [400, 100, 200, 80], "text": "Far down"})).unwrap()["id"].clone();
    nest(&mut s, &past_walk, 65);
    nest(&mut s, &far, 100);
    for (id, depth) in [(&past_walk, 65), (&far, 100)] {
        let e = s.execute("text.fit", &json!({"ids": [id]})).unwrap_err().to_string();
        assert!(e.contains(&format!("is nested {depth} groups deep")), "{e}");
    }
    assert_eq!(fit(&mut s, json!({}))["bodies"], json!([]));
    let e = s.execute("text.fit", &json!({"ids": [99_999]})).unwrap_err().to_string();
    assert!(e.contains("no shape 99999 on slide 1"), "{e}");
}

/// Paragraphs "Line 1" to "Line {n}". Each is one 18 pt line in a shape, and a line is 1.2 times
/// its size tall, so `n` of them are `21.6 * n` pt of text.
fn numbered_lines(n: usize) -> String {
    (1..=n).map(|i| format!("Line {i}")).collect::<Vec<_>>().join("\n")
}

/// Vertical text is set in its frame turned a quarter turn, so its lines stack from side to side:
/// `frameHeight` is the frame's width less its insets, and `overflowPt` how far the lines reach
/// past its left and right edges. A turned 60 x 300 pt box reports what a 300 x 60 pt horizontal
/// box with the same text reports.
#[test]
fn vertical_text_overflows_past_the_frames_left_and_right_edges() {
    let mut s = Session::with_new();
    s.execute("slide.new", &json!({"layout": "blank"})).unwrap();
    let text = numbered_lines(4);
    let mut insert = |rect: [i64; 4], dir: Option<&str>| {
        let id = s.execute("shape.insert", &json!({"preset": "rect", "rect": rect, "text": text})).unwrap()["id"].clone();
        if let Some(dir) = dir {
            s.execute("edit.select", &json!({"ids": [id]})).unwrap();
            s.execute("format.direction", &json!({"dir": dir})).unwrap();
        }
        id
    };
    let across = insert([100, 100, 300, 60], None);
    let turned = [insert([450, 20, 60, 300], Some("rotate90")), insert([550, 20, 60, 300], Some("rotate270"))];
    let stacked = insert([650, 20, 60, 300], Some("stacked"));
    let r = fit(&mut s, json!({"ids": [across, turned[0], turned[1], stacked]}));
    let b = &r["bodies"][0];
    // Four 21.6 pt lines (86.4 pt) in a frame of 60 pt less 3.6 pt insets at each side (52.8 pt).
    assert_eq!(b["lines"], 4, "{b}");
    assert_eq!(num(b, "contentHeight"), 86.4, "{b}");
    assert_eq!(num(b, "frameHeight"), 52.8, "{b}");
    assert_eq!(b["overflows"], true, "{b}");
    assert_eq!(num(b, "overflowPt"), 33.6, "{b}");
    for v in [&r["bodies"][1], &r["bodies"][2]] {
        for k in ["lines", "contentHeight", "frameHeight", "overflows", "overflowPt"] {
            assert_eq!(v[k], b[k], "{k}: turned {v}, horizontal {b}");
        }
    }
    // Stacked text stands each character upright, its lines placed from left to right: its frame
    // is the shape's width less the insets too, and the lines reach past it by as much as they
    // are taller than it.
    let st = &r["bodies"][3];
    assert_eq!(num(st, "frameHeight"), 52.8, "{st}");
    assert_eq!(st["overflows"], true, "{st}");
    let over = num(st, "contentHeight") - num(st, "frameHeight");
    assert!((num(st, "overflowPt") - over).abs() < 0.11, "{st}");
}

/// A body set in columns fills each column from the top, and the last column takes the lines the
/// others leave. `overflowPt` measures the lines where the columns draw them, so only the last
/// column overflows, while `contentHeight` is the text's height set in one column.
#[test]
fn text_in_columns_overflows_only_where_the_last_column_passes_the_frame() {
    let mut s = Session::with_new();
    s.execute("slide.new", &json!({"layout": "blank"})).unwrap();
    // Frames of 100 pt less 3.6 pt insets at top and bottom, 92.8 pt: room for four 21.6 pt lines.
    let fits = s.execute("shape.insert", &json!({"preset": "rect", "rect": [100, 100, 400, 100], "text": numbered_lines(6)})).unwrap()["id"].clone();
    let spills =
        s.execute("shape.insert", &json!({"preset": "rect", "rect": [100, 250, 400, 100], "text": numbered_lines(14)})).unwrap()["id"].clone();
    let one = fit(&mut s, json!({"ids": [fits, spills]}));
    // In one column, six lines (129.6 pt) and fourteen (302.4 pt) both overflow.
    assert_eq!(num(&one["bodies"][0], "overflowPt"), 36.8, "{one}");
    assert_eq!(num(&one["bodies"][1], "overflowPt"), 209.6, "{one}");
    s.execute("edit.select", &json!({"ids": [fits, spills]})).unwrap();
    s.execute("format.columns", &json!({"count": 2, "spacing": 18})).unwrap();
    let two = fit(&mut s, json!({"ids": [fits, spills]}));
    let (a, b) = (&two["bodies"][0], &two["bodies"][1]);
    // Six lines: four in the first column and two in the second, so every column fits.
    assert_eq!(a["lines"], 6, "{a}");
    assert_eq!(a["overflows"], false, "{a}");
    assert_eq!(a["overflowPt"], 0.0, "{a}");
    assert_eq!(a["contentHeight"], one["bodies"][0]["contentHeight"], "{a}");
    assert!(num(a, "contentHeight") > num(a, "frameHeight"), "{a}");
    // Fourteen lines: four in the first column and ten (216 pt) in the last, 123.2 pt more than
    // the frame holds.
    assert_eq!(b["lines"], 14, "{b}");
    assert_eq!(b["overflows"], true, "{b}");
    assert_eq!(num(b, "overflowPt"), 123.2, "{b}");
    assert_eq!(b["contentHeight"], one["bodies"][1]["contentHeight"], "{b}");
}
