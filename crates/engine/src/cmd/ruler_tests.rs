//! The ruler's data and tab-stop editing.

use serde_json::json;

use crate::Session;

fn editing() -> (Session, u64) {
    let mut s = Session::with_new();
    s.execute("slide.new", &json!({"layout": "blank"})).unwrap();
    let id = s.execute("insert.textBox", &json!({"rect": [100, 50, 400, 40]})).unwrap()["id"].as_u64().unwrap();
    s.execute("text.edit", &json!({"id": id})).unwrap();
    s.execute("text.insert", &json!({"text": "a\tb"})).unwrap();
    (s, id)
}

#[test]
fn ruler_reports_the_text_area_and_indents() {
    let (mut s, _) = editing();
    let r = s.execute("format.ruler", &json!({})).unwrap();
    assert!((r["x"].as_f64().unwrap() - 107.2).abs() < 0.01, "{r}");
    assert_eq!(r["rotated"], false);
    assert_eq!(r["tabs"].as_array().unwrap().len(), 0);
    s.execute("format.paragraph", &json!({"indentLeft": 36, "indentFirst": -18})).unwrap();
    let r = s.execute("format.ruler", &json!({})).unwrap();
    assert_eq!((r["marginLeft"].as_f64(), r["indent"].as_f64()), (Some(36.0), Some(-18.0)));
    s.execute("text.exit", &json!({})).unwrap();
    assert!(s.execute("format.ruler", &json!({})).is_err(), "only while editing text");
}

#[test]
fn tabs_add_move_remove_clear_and_undo() {
    let (mut s, _) = editing();
    let tabs = |s: &mut Session| -> Vec<(f64, String)> {
        s.execute("format.ruler", &json!({})).unwrap()["tabs"]
            .as_array()
            .unwrap()
            .iter()
            .map(|t| (t["pos"].as_f64().unwrap(), t["align"].as_str().unwrap().to_string()))
            .collect()
    };
    s.execute("format.tabs", &json!({"add": {"pos": 144, "align": "center"}})).unwrap();
    s.execute("format.tabs", &json!({"add": {"pos": 72}})).unwrap();
    assert_eq!(tabs(&mut s), vec![(72.0, "l".into()), (144.0, "ctr".into())], "sorted");
    s.execute("format.tabs", &json!({"add": {"pos": 72.2, "align": "r"}})).unwrap();
    assert_eq!(tabs(&mut s), vec![(72.2, "r".into()), (144.0, "ctr".into())], "a stop at the same place is replaced");
    s.execute("format.tabs", &json!({"move": {"from": 144, "to": 200}})).unwrap();
    s.execute("format.tabs", &json!({"remove": 72.2})).unwrap();
    assert_eq!(tabs(&mut s), vec![(200.0, "ctr".into())]);
    s.execute("edit.undo", &json!({})).unwrap();
    assert_eq!(tabs(&mut s).len(), 2);
    s.execute("format.tabs", &json!({"clear": true})).unwrap();
    assert!(tabs(&mut s).is_empty());
    s.ui_requests.clear();
    s.execute("format.tabs", &json!({})).unwrap();
    assert!(matches!(s.ui_requests.last(), Some(crate::UiRequest::Dialog { id, .. }) if id == "tabs"), "no fields opens the Tabs dialog");
    assert!(s.execute("format.tabs", &json!({"add": {"pos": f64::MAX}})).is_ok(), "clamped, not a crash");
    for i in 0..100 {
        s.execute("format.tabs", &json!({"add": {"pos": i as f64 * 3.0}})).unwrap();
    }
    assert!(tabs(&mut s).len() <= 32);
    s.execute("format.tabs", &json!({"set": [{"pos": 90, "align": "dec"}, {"pos": 30, "align": "right"}, {"pos": "x"}], "defaultTab": 36})).unwrap();
    assert_eq!(tabs(&mut s), vec![(30.0, "r".into()), (90.0, "dec".into())], "set replaces all, sorted, bad entries dropped");
    assert_eq!(s.execute("format.ruler", &json!({})).unwrap()["defaultTab"].as_f64(), Some(36.0));
}
