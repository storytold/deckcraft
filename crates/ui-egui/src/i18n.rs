//! Interface localization only; never translate document content or command IDs.
//! This module must be wired into the frontend before its translations are visible.

use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Language {
    #[default]
    #[serde(rename = "en", other)]
    English,
    #[serde(rename = "ru")]
    Russian,
}

impl Language {
    pub fn display_name(self) -> &'static str {
        match self {
            Self::English => "English",
            Self::Russian => "Русский",
        }
    }

    /// Borrow the original English text when a translation is not available.
    pub fn text(self, english: &str) -> &str {
        if self == Self::English {
            return english;
        }
        match english {
            "Interface language" => "Язык интерфейса",
            "Slide Master" => "Образец слайдов",
            " (not checked)" => " (проверка отключена)",
            "Language" => "Язык",
            "Fit slide to current window" => "Вписать слайд в окно",
            "Zoom level" => "Уровень масштаба",
            "Zoom" => "Масштаб",
            "Zoom In" => "Увеличить масштаб",
            "Zoom Out" => "Уменьшить масштаб",
            "Slide Show" => "Показ слайдов",
            "Reading View" => "Режим чтения",
            "Slide Sorter" => "Сортировщик слайдов",
            "Normal" => "Обычный режим",
            "Comments" => "Комментарии",
            "Notes" => "Заметки",
            "Click to add notes" => "Нажмите, чтобы добавить заметки",
            "Delete Repeated Word" => "Удалить повторяющееся слово",
            "(no spelling suggestions)" => "(нет вариантов исправления)",
            "Ignore All" => "Пропустить все",
            "Add to Dictionary" => "Добавить в словарь",
            _ => english,
        }
    }

    /// `current` and `total` are supplied by the caller's existing slide selection logic.
    pub fn slide_position(self, current: usize, total: usize) -> String {
        match self {
            Self::English => format!("Slide {current} of {total}"),
            Self::Russian => format!("Слайд {current} из {total}"),
        }
    }
}

/// Russian cardinal forms for nonnegative integer counts: one, few, many.
/// Decimal quantities require a separate rule and must not use this helper.
pub fn russian_integer_form(count: u64) -> usize {
    if (11..=14).contains(&(count % 100)) {
        return 2;
    }
    match count % 10 {
        1 => 0,
        2..=4 => 1,
        _ => 2,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn english_and_missing_translations_preserve_input() {
        let input = String::from("A diagnostic not in the catalog");
        assert_eq!(Language::English.text("Notes"), "Notes");
        assert_eq!(Language::Russian.text(&input), input.as_str());
        assert_eq!(Language::Russian.text("Notes"), "Заметки");
        assert_eq!(Language::Russian.text("slide.new"), "slide.new");
    }

    #[test]
    fn slide_positions_keep_numbers() {
        assert_eq!(Language::English.slide_position(1, 12), "Slide 1 of 12");
        assert_eq!(Language::Russian.slide_position(1, 12), "Слайд 1 из 12");
        assert_eq!(Language::Russian.slide_position(0, 0), "Слайд 0 из 0");
    }

    #[test]
    fn russian_integer_forms_cover_teens_and_large_counts() {
        for (count, expected) in [
            (0, 2), (1, 0), (2, 1), (4, 1), (5, 2),
            (11, 2), (12, 2), (14, 2), (21, 0), (22, 1),
            (25, 2), (101, 0), (111, 2), (112, 2),
            (114, 2), (121, 0), (122, 1), (u64::MAX, 2),
        ] {
            assert_eq!(russian_integer_form(count), expected, "count={count}");
        }
    }
}
