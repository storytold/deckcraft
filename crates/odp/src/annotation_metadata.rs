//! Bounded annotation scalar/date profile from ODF 1.3 and XML Schema 1.0 §3.2.7.
pub(crate) const MAX_SCALAR: usize = 8192;
pub(crate) const MAX_DATE: usize = deckcraft_model::datetime::MAX_DATE_TIME_BYTES;

pub(crate) fn valid_xml_text(value: &str) -> bool {
    value.chars().all(|ch| matches!(ch, '\t' | '\n' | '\r' | '\u{20}'..='\u{d7ff}' | '\u{e000}'..='\u{fffd}' | '\u{10000}'..='\u{10ffff}'))
}

/// Shared XML Schema profile; scalar XML preservation and metadata caps remain local.
pub(crate) fn valid_date(value: &str) -> bool {
    deckcraft_model::datetime::valid_date_time(value)
}
