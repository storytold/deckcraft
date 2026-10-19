//! Equations through PPTX: typed ones are saved as OMML with their run properties, OMML as
//! PowerPoint writes it comes back with its font and size, and table cells keep theirs.

use std::sync::Arc;

use deckcraft_geom::Xfrm;
use deckcraft_model::table::Table;
use deckcraft_model::text::{Paragraph, Run, RunKind, RunProps, TextBody};
use deckcraft_model::{Presentation, Shape, ShapeId, ShapeKind, defaults};

const NS_M: &str = r#"xmlns:m="http://schemas.openxmlformats.org/officeDocument/2006/math""#;

fn equation(text: &str, omml: &str, props: RunProps) -> TextBody {
    TextBody {
        paragraphs: vec![Paragraph { runs: vec![Run { text: text.into(), props, kind: RunKind::Math { omml: omml.into() } }], ..Default::default() }],
        ..Default::default()
    }
}

fn deck(kind: ShapeKind, text: Option<TextBody>) -> Presentation {
    let mut p = defaults::new_presentation(None);
    let id = ShapeId(p.alloc_id().expect("id"));
    let sh = Shape { id, name: "Equation".into(), xfrm: Some(Xfrm::new(10.0, 10.0, 300.0, 80.0)), kind, text, ..Default::default() };
    if let Some(s) = p.slides.first_mut() {
        Arc::make_mut(s).shapes = vec![sh];
    }
    p
}

fn round(p: &Presentation) -> (Presentation, String) {
    let bytes = deckcraft_pptx::export(p).expect("export");
    let mut z = zip::ZipArchive::new(std::io::Cursor::new(&bytes)).expect("zip");
    let slide = std::io::read_to_string(z.by_name("ppt/slides/slide1.xml").expect("slide1")).expect("utf8");
    (deckcraft_pptx::import(&bytes).expect("import"), slide)
}

fn first_run(p: &Presentation) -> Run {
    let sh = p.slides.first().and_then(|s| s.shapes.first()).expect("shape");
    let body = match &sh.kind {
        ShapeKind::Table(t) => t.cell(0, 0).map(|c| c.text.clone()),
        _ => sh.text.clone(),
    };
    body.and_then(|b| b.paragraphs.into_iter().next()).and_then(|p| p.runs.into_iter().next()).expect("run")
}

#[test]
fn a_typed_equation_is_saved_as_omml_with_its_font_and_size() {
    let props = RunProps { font: Some("Latin Modern Math".into()), size: Some(28.0), ..Default::default() };
    let (q, xml) = round(&deck(ShapeKind::Shape, Some(equation("x^2/2", "", props))));
    assert!(xml.contains("<m:oMathPara") && xml.contains("<m:f>") && xml.contains("<m:sSup>"), "{xml}");
    assert!(xml.contains(r#"<a:latin typeface="Latin Modern Math"/>"#), "{xml}");
    let r = first_run(&q);
    assert!(matches!(&r.kind, RunKind::Math { omml } if omml.contains("<m:f>")), "{r:?}");
    assert_eq!(r.text, "(x^2)/2", "its linear form as read back");
    assert_eq!((r.props.font.as_deref(), r.props.size), (Some("Latin Modern Math"), Some(28.0)));
    // Saved again, it is the same.
    let (q2, _) = round(&q);
    assert_eq!(first_run(&q2), r);
}

#[test]
fn powerpoint_omml_comes_back_with_its_run_properties_and_linear_text() {
    let omml = format!(
        r#"<m:oMathPara {NS_M}><m:oMath><m:f><m:num><m:r><a:rPr lang="en-US" sz="2400" i="1"><a:latin typeface="Cambria Math"/></a:rPr><m:t>a</m:t></m:r></m:num><m:den><m:r><a:rPr lang="en-US" sz="2400" i="1"><a:latin typeface="Cambria Math"/></a:rPr><m:t>b</m:t></m:r></m:den></m:f></m:oMath></m:oMathPara>"#
    );
    let (q, xml) = round(&deck(ShapeKind::Shape, Some(equation("ab", &omml, RunProps::default()))));
    assert!(xml.contains(&omml), "imported OMML is written back unchanged");
    let r = first_run(&q);
    assert_eq!(r.text, "a/b");
    assert_eq!((r.props.font.as_deref(), r.props.size, r.props.italic), (Some("Cambria Math"), Some(24.0), None));
}

#[test]
fn an_equation_in_a_table_cell_round_trips() {
    let mut t = Table::new(1, 2, 300.0, 80.0);
    if let Some(c) = t.cell_mut(0, 0) {
        c.text = equation("sqrt(x)", "", RunProps::default());
    }
    let (q, xml) = round(&deck(ShapeKind::Table(t), None));
    assert!(xml.contains("<mc:Choice") && xml.contains("<m:rad>"), "{xml}");
    let r = first_run(&q);
    assert!(matches!(&r.kind, RunKind::Math { omml } if omml.contains("<m:rad>")), "{r:?}");
    assert_eq!(r.text, "√(x)");
}
