//! Cell data dialogs keep drafts outside the presentation until Apply succeeds.
use crate::{SlideApp, dialogs::Dialog};
use deckcraft_model::{
    ShapeKind,
    table::{CellNumberKind, CellValue, DateFormatPart, NegativeDisplay, NumberFormat, NumberLocale, NumberStyle},
};
use egui::Ui;
use serde_json::{Value, json};

fn field(ui: &mut Ui, d: &mut Dialog, key: &str, label: &str, default: &str) {
    ui.horizontal(|ui| {
        ui.label(label);
        ui.text_edit_singleline(d.fields.entry(key.into()).or_insert_with(|| default.into()));
    });
}
fn choice(ui: &mut Ui, d: &mut Dialog, key: &str, label: &str, default: &str, choices: &[&str]) {
    let value = d.fields.entry(key.into()).or_insert_with(|| default.into());
    egui::ComboBox::from_id_salt(key).selected_text(value.as_str()).show_ui(ui, |ui| {
        for choice in choices {
            ui.selectable_value(value, (*choice).into(), *choice);
        }
    });
    ui.label(egui::RichText::new(label).small());
}
fn value(d: &Dialog, key: &str) -> String {
    d.fields.get(key).cloned().unwrap_or_default()
}
fn digits(d: &Dialog) -> Result<u8, String> {
    value(d, "decimals").parse::<u8>().ok().filter(|n| *n <= 28).ok_or_else(|| "Decimal places must be between 0 and 28.".into())
}
fn clock_parts(elapsed: bool, decimals: u8) -> Vec<DateFormatPart> {
    vec![
        DateFormatPart::Hours { long: true, elapsed },
        DateFormatPart::Literal { text: ":".into() },
        DateFormatPart::Minutes { long: true },
        DateFormatPart::Literal { text: ":".into() },
        DateFormatPart::Seconds { long: true, decimal_places: decimals },
    ]
}
fn format_program(d: &Dialog) -> Result<NumberFormat, String> {
    let decimals = digits(d)?;
    let grouping = value(d, "grouping") == "Yes";
    let locale = match value(d, "locale").as_str() {
        "French (Belgium)" => NumberLocale::FrBe,
        "German" => NumberLocale::DeDe,
        _ => NumberLocale::EnUs,
    };
    let style = match value(d, "format").as_str() {
        "Number" => NumberStyle::Number { decimal_places: decimals, min_decimal_places: decimals, min_integer_digits: 1, grouping },
        "Percentage" => NumberStyle::Percentage { decimal_places: decimals, min_decimal_places: decimals, grouping },
        "Currency" => NumberStyle::Currency {
            decimal_places: decimals,
            min_decimal_places: decimals,
            grouping,
            code: value(d, "currency"),
            symbol: value(d, "symbol"),
            symbol_after: value(d, "currencyPosition") == "After",
            space: true,
        },
        "Scientific" => NumberStyle::Scientific { decimal_places: decimals, min_exponent_digits: 2, exponent_sign: true },
        "Fraction" => NumberStyle::Fraction {
            denominator_digits: value(d, "fractionDigits")
                .parse::<u8>()
                .ok()
                .filter(|n| (1..=4).contains(n))
                .ok_or_else(|| "Fraction denominator digits must be between 1 and 4.".to_string())?,
            fixed_denominator: None,
        },
        "Date" => NumberStyle::DateTime {
            parts: vec![
                DateFormatPart::Year { long: true },
                DateFormatPart::Literal { text: "-".into() },
                DateFormatPart::Month { long: true, textual: false },
                DateFormatPart::Literal { text: "-".into() },
                DateFormatPart::Day { long: true },
            ],
        },
        "Time" => NumberStyle::DateTime { parts: clock_parts(false, decimals) },
        "Duration" => NumberStyle::Duration { parts: clock_parts(true, decimals) },
        "Boolean" => NumberStyle::Boolean { true_text: value(d, "trueLabel"), false_text: value(d, "falseLabel") },
        "Text" => NumberStyle::Text,
        _ => NumberStyle::General,
    };
    let format = NumberFormat {
        style,
        locale,
        prefix: value(d, "prefix"),
        suffix: value(d, "suffix"),
        negative: if value(d, "negative") == "Parentheses" { NegativeDisplay::Parentheses } else { NegativeDisplay::Minus },
        ..Default::default()
    };
    format.validate().map_err(|error| error.to_string())?;
    Ok(format)
}
fn format_summary(format: &NumberFormat) -> String {
    let style = match &format.style {
        NumberStyle::General => "General".into(),
        NumberStyle::Number { decimal_places, .. } => format!("Number, {decimal_places} decimal places"),
        NumberStyle::Percentage { decimal_places, .. } => format!("Percentage, {decimal_places} decimal places"),
        NumberStyle::Currency { code, decimal_places, .. } => format!("Currency {code}, {decimal_places} decimal places"),
        NumberStyle::Scientific { decimal_places, .. } => format!("Scientific, {decimal_places} decimal places"),
        NumberStyle::Fraction { .. } => "Fraction".into(),
        NumberStyle::DateTime { parts } => format!("Date/time, {} parts", parts.len()),
        NumberStyle::Duration { parts } => format!("Duration, {} parts", parts.len()),
        NumberStyle::Boolean { .. } => "Boolean".into(),
        NumberStyle::Text => "Text".into(),
    };
    let name = if format.name.is_empty() { String::new() } else { format!("{}: ", format.name) };
    format!("Current format: {name}{style}; {}-{}; {} conditional sections", format.locale.language(), format.locale.country(), format.sections.len())
}
fn draft_format(d: &Dialog) -> Result<Value, String> {
    if let Some(existing) = d.params.get("existingFormat") {
        match value(d, "formatMode").as_str() {
            "Keep" => return Ok(existing.clone()),
            "Affixes" => {
                let mut format: NumberFormat = serde_json::from_value(existing.clone()).map_err(|error| error.to_string())?;
                format.prefix = value(d, "prefix");
                format.suffix = value(d, "suffix");
                format.validate().map_err(|error| error.to_string())?;
                return serde_json::to_value(format).map_err(|error| error.to_string());
            }
            _ => {}
        }
    }
    serde_json::to_value(format_program(d)?).map_err(|error| error.to_string())
}
fn seed(app: &SlideApp, d: &mut Dialog) -> Result<(), String> {
    if d.params.get("target").is_some() {
        return Ok(());
    }
    let st = app.session.active().ok_or_else(|| "Open a presentation first.".to_string())?;
    let id = st
        .selection
        .text
        .as_ref()
        .filter(|t| t.cell.is_some() && !t.notes)
        .map(|t| t.shape)
        .or_else(|| st.selection.shapes.first().copied())
        .ok_or_else(|| "Select a table cell.".to_string())?;
    let shape = st.shape(id).ok_or_else(|| "The table is gone.".to_string())?;
    let ShapeKind::Table(table) = &shape.kind else { return Err("Select a table cell.".into()) };
    let path = if st.selection.table_owner == Some(id) { st.selection.table_path.clone() } else { Default::default() };
    let (row, col) = st
        .selection
        .text
        .as_ref()
        .filter(|t| t.shape == id)
        .and_then(|t| t.cell)
        .or(st.selection.cells.filter(|c| c.0 == id).map(|c| c.1))
        .unwrap_or((0, 0));
    let cell = table.at_path(&path).map_err(|error| error.to_string())?.cell(row, col).ok_or_else(|| "The cell is gone.".to_string())?;
    d.params = json!({"target":{"id":id,"row":row,"col":col,"path":path},"uid":st.uid,"revision":st.revision});
    d.fields.insert("expression".into(), cell.formula.as_ref().map(|f| f.expression().to_string()).unwrap_or_else(|| "=".into()));
    d.fields.insert("formulaMode".into(), if cell.formula.as_ref().is_some_and(|f| !f.is_v2()) { "Legacy" } else { "Extended" }.into());
    let scalar = cell.data.as_ref().map(|data| data.value());
    let (kind, text) = match scalar {
        Some(CellValue::Number { lexical, .. }) => ("Number", lexical.clone()),
        Some(CellValue::Boolean { value }) => ("Boolean", value.to_string()),
        Some(CellValue::Date { lexical }) => ("Date", lexical.clone()),
        Some(CellValue::Duration { lexical }) => ("Duration", lexical.clone()),
        Some(CellValue::String { value }) => ("Text", value.clone()),
        None => ("Text", cell.text.text()),
    };
    d.fields.insert("valueType".into(), kind.into());
    d.fields.insert("value".into(), text);
    d.fields.insert("display".into(), cell.text.text());
    if let Some(format) = &cell.number_format {
        // Untouched existing programs, including conditional sections and custom dates, stay exact.
        d.params["existingFormat"] = serde_json::to_value(format).map_err(|error| error.to_string())?;
        d.fields.insert("formatMode".into(), "Keep".into());
        d.fields.insert("formatSummary".into(), format_summary(format));
        d.fields.insert("prefix".into(), format.prefix.clone());
        d.fields.insert("suffix".into(), format.suffix.clone());
    }
    Ok(())
}
pub(crate) fn show(app: &mut SlideApp, ui: &mut Ui, d: &mut Dialog) -> bool {
    if let Err(error) = seed(app, d) {
        ui.label(error);
        return ui.button("Close").clicked();
    }
    let target = d.params["target"].clone();
    ui.label(format!("Cell {}, {}", target["row"].as_u64().unwrap_or(0) + 1, target["col"].as_u64().unwrap_or(0) + 1));
    match d.id.as_str() {
        "cellFormula" => {
            ui.label("Formula");
            ui.add(egui::TextEdit::multiline(d.fields.entry("expression".into()).or_default()).desired_rows(3).desired_width(360.0));
            choice(ui, d, "formulaMode", "Calculation rules", "Extended", &["Extended", "Legacy"]);
            ui.label("Example: =SUM([.A1:.B3]) / 2");
            ui.label("References stay in this table. Use semicolons between function arguments.");
        }
        "cellValue" => {
            choice(ui, d, "valueType", "Value type", "Text", &["Text", "Number", "Boolean", "Date", "Duration"]);
            field(ui, d, "value", "Value", "");
            field(ui, d, "display", "Displayed text", "");
            ui.label("Dates use YYYY-MM-DD or ISO date/time; durations use ISO P…T… syntax.");
        }
        "nestedTable" => {
            field(ui, d, "rows", "Rows", "2");
            field(ui, d, "cols", "Columns", "2");
            ui.label("The table is inserted at the text selection in this cell.");
        }
        _ => {
            let existing = d.params.get("existingFormat").is_some();
            if existing {
                if let Some(summary) = d.fields.get("formatSummary") {
                    ui.label(summary);
                }
                let mode = d.fields.entry("formatMode".into()).or_insert_with(|| "Keep".into());
                ui.radio_value(mode, "Keep".into(), "Keep current format");
                ui.radio_value(mode, "Affixes".into(), "Edit prefix and suffix");
                ui.radio_value(mode, "Replace".into(), "Replace current format");
            }
            let mode = value(d, "formatMode");
            if existing && mode == "Keep" {
                ui.label("Apply keeps the complete current format.");
            } else if existing && mode == "Affixes" {
                ui.label("Edit the default prefix and suffix. Conditional sections keep their own prefixes and suffixes.");
                field(ui, d, "prefix", "Prefix", "");
                field(ui, d, "suffix", "Suffix", "");
            } else {
                if existing {
                    ui.label("Applying replaces the current style and conditional sections.");
                }
                choice(
                    ui,
                    d,
                    "format",
                    "Format",
                    "Number",
                    &["General", "Number", "Percentage", "Currency", "Scientific", "Fraction", "Date", "Time", "Duration", "Boolean", "Text"],
                );
                choice(ui, d, "locale", "Separators and labels", "English (US)", &["English (US)", "French (Belgium)", "German"]);
                field(ui, d, "decimals", "Decimal places", "2");
                choice(ui, d, "grouping", "Group thousands", "Yes", &["Yes", "No"]);
                choice(ui, d, "negative", "Negative numbers", "Minus", &["Minus", "Parentheses"]);
                match value(d, "format").as_str() {
                    "Currency" => {
                        field(ui, d, "currency", "Currency code", "EUR");
                        field(ui, d, "symbol", "Symbol", "€");
                        choice(ui, d, "currencyPosition", "Symbol position", "After", &["Before", "After"]);
                    }
                    "Fraction" => field(ui, d, "fractionDigits", "Denominator digits", "2"),
                    "Boolean" => {
                        field(ui, d, "trueLabel", "True label", "TRUE");
                        field(ui, d, "falseLabel", "False label", "FALSE");
                    }
                    _ => {}
                }
                field(ui, d, "prefix", "Prefix", "");
                field(ui, d, "suffix", "Suffix", "");
            }
            if ui.button("Remove number format").clicked() {
                let mut p = target.clone();
                p["format"] = Value::Null;
                return apply(app, d, "table.setCellNumberFormat", p);
            }
        }
    }
    if let Some(error) = d.fields.get("error") {
        ui.colored_label(crate::theme::Tokens::get(ui.ctx()).text, error);
    }
    ui.separator();
    let mut close = false;
    ui.horizontal(|ui| {
        if ui.button("Cancel").clicked() {
            close = true;
        }
        if ui.button("Apply").clicked() {
            let result = (|| -> Result<(&str, Value), String> {
                let mut p = target.clone();
                let command = match d.id.as_str() {
                    "cellFormula" => {
                        p["expression"] = Value::String(value(d, "expression"));
                        p["profile"] =
                            Value::String(if value(d, "formulaMode") == "Legacy" { "exact-decimal-v1" } else { "presentation-decimal-v2" }.into());
                        "table.setCellFormula"
                    }
                    "cellValue" => {
                        let lexical = value(d, "value");
                        let scalar = match value(d, "valueType").as_str() {
                            "Number" => CellValue::Number { kind: CellNumberKind::Float, lexical },
                            "Boolean" => {
                                CellValue::Boolean { value: lexical.parse::<bool>().map_err(|_| "Boolean must be true or false.".to_string())? }
                            }
                            "Date" => CellValue::Date { lexical },
                            "Duration" => CellValue::Duration { lexical },
                            _ => CellValue::String { value: lexical },
                        };
                        scalar.validate().map_err(|error| error.to_string())?;
                        p["value"] = serde_json::to_value(scalar).map_err(|error| error.to_string())?;
                        p["display"] = Value::String(value(d, "display"));
                        "table.setCellValue"
                    }
                    "nestedTable" => {
                        for key in ["rows", "cols"] {
                            p[key] = Value::from(
                                value(d, key)
                                    .parse::<u64>()
                                    .ok()
                                    .filter(|n| (1..=100).contains(n))
                                    .ok_or_else(|| "Rows and columns must be between 1 and 100.".to_string())?,
                            );
                        }
                        "table.insertNested"
                    }
                    _ => {
                        p["format"] = draft_format(d)?;
                        "table.setCellNumberFormat"
                    }
                };
                Ok((command, p))
            })();
            match result {
                Ok((command, p)) => close = apply(app, d, command, p),
                Err(error) => {
                    d.fields.insert("error".into(), error);
                }
            }
        }
    });
    close
}
fn apply(app: &mut SlideApp, d: &mut Dialog, command: &str, params: Value) -> bool {
    if app.session.active().map(|st| Value::from(st.uid)) != Some(d.params["uid"].clone()) {
        d.fields.insert("error".into(), "The original presentation is no longer active.".into());
        return false;
    }
    if app.session.active().map(|st| Value::from(st.revision)) != Some(d.params["revision"].clone()) {
        d.fields.insert("error".into(), "The presentation changed while this dialog was open. Reopen the dialog to use the current cell.".into());
        return false;
    }
    match app.run(command, params) {
        Ok(_) => true,
        Err(error) => {
            d.fields.insert("error".into(), error.to_string());
            false
        }
    }
}
