//! Independent native corruption fixtures: new policies never enter legacy numeric repair.
use std::io::{Cursor, Write};

use deckcraft_format::{MIMETYPE, load};
use deckcraft_model::text::LineHeight;
use serde_json::{Value, json};

fn package(presentation: &str, sequence_manifest: bool) -> Vec<u8> {
    let manifest = if sequence_manifest {
        format!(r#"["deckcraft",1,"independent review",{presentation}]"#)
    } else {
        format!(r#"{{"format":"deckcraft","version":1,"generator":"independent review","presentation":{presentation}}}"#)
    };
    let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
    for (name, bytes) in [("mimetype", MIMETYPE), ("presentation.json", manifest.as_str())] {
        zip.start_file(name, zip::write::SimpleFileOptions::default()).unwrap();
        zip.write_all(bytes.as_bytes()).unwrap();
    }
    zip.finish().unwrap().into_inner()
}

fn owner(props: &str, reversed: bool) -> String {
    let xfrm = r#""xfrm":{"x":null,"y":0,"w":100,"h":100}"#;
    let text = format!(r#""text":{{"paragraphs":[{{"props":{props},"runs":[{{"text":"original review"}}]}}]}}"#);
    let shape = if reversed { format!("{text},{xfrm}") } else { format!("{xfrm},{text}") };
    format!(r#"{{"slides":[{{"id":7,"shapes":[{{"id":9,{shape}}}]}}]}}"#)
}

#[test]
fn malformed_new_policies_cannot_be_repaired_by_unrelated_legacy_numeric_nulls() {
    for policy in [
        json!({"kind":"minimum","points":null}),
        json!({"kind":"minimum"}),
        json!({"kind":"minimum","points":"12"}),
        json!({"kind":"minimum","points":false}),
        json!({"kind":"minimum","points":-1}),
        json!({"kind":"minimum","points":1_000_001}),
        json!({"kind":"minimum","points":2,"unknown":0}),
        json!({"kind":"normal","points":0}),
        json!({"kind":"unknown"}),
    ] {
        for positional_props in [false, true] {
            let props = if positional_props {
                let mut fields = vec![Value::Null; 20];
                fields.push(policy.clone());
                json!(fields)
            } else {
                json!({"lineHeight":policy})
            };
            for reversed in [false, true] {
                let wire = owner(&props.to_string(), reversed);
                for sequence_manifest in [false, true] {
                    assert!(load(&package(&wire, sequence_manifest)).is_err(), "{wire}");
                }
            }
        }
    }
}

#[test]
fn valid_new_policy_preserves_exact_points_while_legacy_coordinate_still_repairs() {
    for policy in [
        LineHeight::Normal,
        LineHeight::Minimum { points: 0.0 },
        LineHeight::Minimum { points: 1_000_000.0 },
        LineHeight::Minimum { points: 42.123_456_789 },
    ] {
        for positional_props in [false, true] {
            let encoded = serde_json::to_value(policy).unwrap();
            let props = if positional_props {
                let mut fields = vec![Value::Null; 20];
                fields.push(encoded);
                json!(fields)
            } else {
                json!({"lineHeight":encoded})
            };
            for reversed in [false, true] {
                for sequence_manifest in [false, true] {
                    let restored = load(&package(&owner(&props.to_string(), reversed), sequence_manifest)).unwrap();
                    let shape = &restored.slides[0].shapes[0];
                    assert_eq!(shape.xfrm.unwrap().x, 0.0);
                    assert_eq!(shape.text.as_ref().unwrap().paragraphs[0].props.line_height, Some(policy));
                    assert_eq!(shape.text.as_ref().unwrap().text(), "original review");
                }
            }
        }
    }
}
