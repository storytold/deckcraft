//! Interface localization only; never translate document content or command IDs.
//! UI call sites must opt into translation explicitly.

use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Language {
    #[serde(rename = "ru")]
    Russian,
    #[default]
    #[serde(rename = "en", other)]
    English,
}

const RUSSIAN_CATALOG: &[(&str, &str)] = &[
    ("New", "Создать"),
    ("Open", "Открыть"),
    ("Info", "Сведения"),
    ("Save As", "Сохранить как"),
    ("Print", "Печать"),
    ("Account", "Учётная запись"),
    ("Draw", "Рисование"),
    ("Design", "Дизайн"),
    ("Transitions", "Переходы"),
    ("Animations", "Анимация"),
    ("Record", "Запись"),
    ("Review", "Рецензирование"),
    ("Shape Format", "Формат фигуры"),
    ("Picture Format", "Формат рисунка"),
    ("Table Design", "Конструктор таблиц"),
    ("Layout", "Макет"),
    ("Chart Design", "Конструктор диаграмм"),
    ("SmartArt Design", "Конструктор SmartArt"),
    ("Playback", "Воспроизведение"),
    ("Options", "параметры"),
    ("Search commands…", "Поиск команд…"),
    ("File (Alt+F)", "Файл (Alt+F)"),
    ("Scroll ribbon left", "Прокрутить ленту влево"),
    ("Scroll ribbon right", "Прокрутить ленту вправо"),
    ("File", "Файл"),
    ("Edit", "Правка"),
    ("View", "Вид"),
    ("Insert", "Вставка"),
    ("Format", "Формат"),
    ("Arrange", "Упорядочить"),
    ("Help", "Справка"),
    ("New Presentation", "Новая презентация"),
    ("Open…", "Открыть…"),
    ("Close", "Закрыть"),
    ("Save", "Сохранить"),
    ("Save As…", "Сохранить как…"),
    ("Export…", "Экспорт…"),
    ("Print…", "Печать…"),
    ("Properties", "Свойства"),
    ("Undo", "Отменить"),
    ("Redo", "Повторить"),
    ("Cut", "Вырезать"),
    ("Copy", "Копировать"),
    ("Paste", "Вставить"),
    ("Paste and Match Formatting", "Вставить и сохранить форматирование"),
    ("Paste Special…", "Специальная вставка…"),
    ("Select All", "Выбрать всё"),
    ("Duplicate", "Дублировать"),
    ("Delete Slide", "Удалить слайд"),
    ("Find…", "Найти…"),
    ("Replace…", "Заменить…"),
    ("Normal", "Обычный режим"),
    ("Slide Sorter", "Сортировщик слайдов"),
    ("Notes Page", "Страница заметок"),
    ("Outline View", "Режим структуры"),
    ("Reading View", "Режим чтения"),
    ("Slide Show", "Показ слайдов"),
    ("Handout Master", "Образец выдач"),
    ("Notes Master", "Образец заметок"),
    ("Ruler", "Линейка"),
    ("Gridlines", "Линии сетки"),
    ("Guides", "Направляющие"),
    ("Grid Options…", "Параметры сетки…"),
    ("Zoom…", "Масштаб…"),
    ("Zoom In", "Увеличить масштаб"),
    ("Zoom Out", "Уменьшить масштаб"),
    ("Fit to Window", "Вписать в окно"),
    ("New Window", "Новое окно"),
    ("New Slide", "Создать слайд"),
    ("Duplicate Slide", "Дублировать слайд"),
    ("Section", "Раздел"),
    ("Text Box", "Текстовое поле"),
    ("WordArt", "Объект WordArt"),
    ("Date and Time", "Дата и время"),
    ("Slide Number", "Номер слайда"),
    ("Table…", "Таблица…"),
    ("Chart", "Диаграмма"),
    ("Picture from File…", "Изображение из файла…"),
    ("Photo Album…", "Фотоальбом…"),
    ("Edit Photo Album…", "Изменить фотоальбом…"),
    ("Audio from File…", "Аудио из файла…"),
    ("Video from File…", "Видео из файла…"),
    ("Font…", "Шрифт…"),
    ("Paragraph…", "Абзац…"),
    ("Bold", "Полужирный"),
    ("Italic", "Курсив"),
    ("Underline", "Подчёркнутый"),
    ("Align Left", "По левому краю"),
    ("Center", "По центру"),
    ("Align Right", "По правому краю"),
    ("Justify", "По ширине"),
    ("Size and Position…", "Размер и положение…"),
    ("Right-to-Left Text Direction", "Направление текста справа налево"),
    ("Bring to Front", "На передний план"),
    ("Send to Back", "На задний план"),
    ("Bring Forward", "Переместить вперёд"),
    ("Send Backward", "Переместить назад"),
    ("Group", "Группировать"),
    ("Ungroup", "Разгруппировать"),
    ("Regroup", "Повторить группировку"),
    ("Rotate Left 90°", "Повернуть влево на 90°"),
    ("Rotate Right 90°", "Повернуть вправо на 90°"),
    ("Flip Horizontal", "Отразить по горизонтали"),
    ("Flip Vertical", "Отразить по вертикали"),
    ("Play from Start", "Воспроизвести с начала"),
    ("Play from Current Slide", "Воспроизвести с текущего слайда"),
    ("Hide Slide", "Скрыть слайд"),
    ("Set Up Show…", "Настроить показ…"),
    ("About DeckCraft", "О DeckCraft"),
    ("Check for Updates…", "Проверить обновления…"),
    ("Command Palette", "Палитра команд"),
    ("Interface language", "Язык интерфейса"),
    ("Slide Master", "Образец слайдов"),
    (" (not checked)", " (проверка отключена)"),
    ("Language", "Язык"),
    ("Fit slide to current window", "Вписать слайд в окно"),
    ("Zoom level", "Уровень масштаба"),
    ("Zoom", "Масштаб"),
    ("Comments", "Комментарии"),
    ("Notes", "Заметки"),
    ("Click to add notes", "Нажмите, чтобы добавить заметки"),
    ("Delete Repeated Word", "Удалить повторяющееся слово"),
    ("(no spelling suggestions)", "(нет вариантов исправления)"),
    ("Ignore All", "Пропустить все"),
    ("Add to Dictionary", "Добавить в словарь"),
    ("Browse", "Обзор"),
    ("More themes", "Другие темы"),
    ("Blank Presentation", "Пустая презентация"),
    ("Sample deck: Northern Lights", "Пример презентации: Северное сияние"),
    (
        "Presentations you open show up here. Use Open to browse for one.",
        "Здесь будут отображаться открытые презентации. Нажмите «Открыть», чтобы выбрать файл.",
    ),
    ("No recent presentations match “{query}”.", "Нет недавних презентаций по запросу «{query}»."),
    ("Remove from Recent", "Удалить из списка недавних"),
    ("Join the ArtCraft community on Discord", "Присоединиться к сообществу ArtCraft в Discord"),
    ("Templates", "Шаблоны"),
    ("More templates", "Другие шаблоны"),
    ("Search for a file", "Поиск файла"),
    ("More presentations", "Другие презентации"),
    ("Search for themes and templates", "Поиск тем и шаблонов"),
    ("No themes or templates match “{query}”.", "Нет тем или шаблонов по запросу «{query}»."),
    ("Open a presentation from a folder (Ctrl+O)", "Открыть презентацию из папки (Ctrl+O)"),
    ("Recover Unsaved Presentations", "Восстановить несохранённые презентации"),
    ("There are no unsaved presentations to recover", "Нет несохранённых презентаций для восстановления"),
    ("Not saved yet", "Ещё не сохранено"),
    ("Copy path", "Копировать путь"),
    ("Copied the path", "Путь скопирован"),
    ("Open file location", "Открыть расположение файла"),
    ("Check for Issues", "Проверить наличие проблем"),
    ("Inspect Presentation", "Проверка презентации"),
    (
        "Before you share it, check the presentation for things people with disabilities might find hard to read.",
        "Перед публикацией проверьте, нет ли в презентации элементов, которые могут быть трудны для восприятия.",
    ),
    ("There are no unsaved changes.", "Несохранённых изменений нет."),
    (
        "This presentation has unsaved changes. Revert goes back to the saved file.",
        "В презентации есть несохранённые изменения. Возврат восстановит сохранённую версию.",
    ),
    ("This presentation has never been saved.", "Эта презентация ещё не сохранялась."),
    ("Revert", "Вернуть сохранённую версию"),
    ("Manage Presentation", "Управление презентацией"),
    ("Title", "Название"),
    ("Add a title", "Добавить название"),
    ("Subject", "Тема"),
    ("Specify the subject", "Указать тему"),
    ("Tags", "Теги"),
    ("Add a tag", "Добавить тег"),
    ("Categories", "Категории"),
    ("Add a category", "Добавить категорию"),
    ("Company", "Организация"),
    ("Specify the company", "Указать организацию"),
    ("Add comments", "Добавить комментарии"),
    ("Author", "Автор"),
    ("Add an author", "Добавить автора"),
    ("Size", "Размер"),
    ("Hidden slides", "Скрытые слайды"),
    ("Location", "Расположение"),
    ("Save as type", "Тип файла"),
    ("Download", "Скачать"),
    ("Download the file", "Скачать файл"),
    ("Choose where to save", "Выбрать место сохранения"),
    ("PowerPoint Presentation (*.pptx)", "Презентация PowerPoint (*.pptx)"),
    ("Opens in PowerPoint and other presentation apps", "Открывается в PowerPoint и других приложениях для презентаций"),
    ("DeckCraft Presentation (*.deckcraft)", "Презентация DeckCraft (*.deckcraft)"),
    ("DeckCraft's own format: everything is kept", "Формат DeckCraft: сохраняются все данные"),
    ("PDF (*.pdf)", "PDF (*.pdf)"),
    ("A fixed copy that keeps its layout on any device", "Фиксированная копия с неизменной компоновкой на любом устройстве"),
    ("Outline (*.txt)", "Структура (*.txt)"),
    ("The slide titles and bullet text as plain text", "Заголовки слайдов и маркированный текст в виде обычного текста"),
    ("Outline/RTF (*.rtf)", "Структура/RTF (*.rtf)"),
    ("The slide titles and bullet text for word processors", "Заголовки слайдов и маркированный текст для текстовых редакторов"),
    ("PNG Picture (*.png)", "Изображение PNG (*.png)"),
    ("The current slide as a picture", "Текущий слайд как изображение"),
    ("JPEG Picture (*.jpg)", "Изображение JPEG (*.jpg)"),
    ("The current slide as a compressed picture", "Текущий слайд как сжатое изображение"),
    ("Print All Slides", "Печатать все слайды"),
    ("Print Current Slide", "Печатать текущий слайд"),
    ("Custom Range", "Выбранный диапазон"),
    ("Print range", "Диапазон печати"),
    ("Slides:", "Слайды:"),
    ("e.g. 1-3, 5", "Например: 1–3, 5"),
    ("Full Page Slides", "Слайды на всю страницу"),
    ("Notes Pages", "Страницы заметок"),
    ("Handouts (1 slide per page)", "Выдачи (1 слайд на странице)"),
    ("Handouts (2 slides per page)", "Выдачи (2 слайда на странице)"),
    ("Handouts (3 slides per page)", "Выдачи (3 слайда на странице)"),
    ("Handouts (4 slides per page)", "Выдачи (4 слайда на странице)"),
    ("Handouts (6 slides per page)", "Выдачи (6 слайдов на странице)"),
    ("Handouts (9 slides per page)", "Выдачи (9 слайдов на странице)"),
    ("Paper size", "Размер бумаги"),
    ("Portrait Orientation", "Книжная ориентация"),
    ("Landscape Orientation", "Альбомная ориентация"),
    ("Print Hidden Slides", "Печатать скрытые слайды"),
    ("Frame Slides", "Обрамлять слайды"),
    ("Edit Header & Footer", "Изменить колонтитулы"),
    ("Nothing to print: every slide in the range is hidden.", "Нечего печатать: все слайды в диапазоне скрыты."),
    ("Previous Page", "Предыдущая страница"),
    ("Next Page", "Следующая страница"),
    ("Appearance", "Внешний вид"),
    ("Dark", "Тёмная"),
    ("Light", "Светлая"),
    ("User Information", "Сведения о пользователе"),
    (
        "DeckCraft has no accounts: there is nothing to sign in to, and your presentations stay on your device.",
        "В DeckCraft нет учётных записей: вход не требуется, презентации остаются на вашем устройстве.",
    ),
    ("Product Information", "Сведения о продукте"),
    (
        "Open source under MIT OR Apache-2.0. Not affiliated with Microsoft.",
        "Открытый исходный код по лицензии MIT OR Apache-2.0. Не связан с Microsoft.",
    ),
    (
        "The version, the people and models who built it, and where to find us.",
        "Версия, участники и модели, участвовавшие в разработке, и наши контакты.",
    ),
    ("Join our community on Discord", "Присоединиться к сообществу в Discord"),
    ("Source on GitHub", "Исходный код на GitHub"),
    ("Insert Table", "Вставка таблицы"),
    ("Slide Size", "Размер слайда"),
    ("Size and Position", "Размер и положение"),
    ("Header and Footer", "Колонтитулы"),
    ("Insert Hyperlink", "Вставка гиперссылки"),
    ("Set Up Slide Show", "Настройка показа слайдов"),
    ("New Comment", "Новый комментарий"),
    ("Alt Text", "Замещающий текст"),
    ("Slides from Outline", "Слайды из структуры"),
    ("Rename Section", "Переименование раздела"),
    ("Reuse Slides", "Повторное использование слайдов"),
    ("Rename", "Переименовать"),
    ("Rename Layout", "Переименовать макет"),
    ("Paragraph", "Абзац"),
    ("Tabs", "Табуляция"),
    ("Font", "Шрифт"),
    ("Preferences", "Настройки"),
    ("AutoCorrect", "Автозамена"),
    ("Grid Options", "Параметры сетки"),
    ("Columns", "Столбцы"),
    ("Export", "Экспорт"),
    ("Symbol", "Символ"),
    ("Photo Album", "Фотоальбом"),
    ("Replace Font", "Замена шрифта"),
    ("Equation", "Формула"),
    ("Spelling", "Правописание"),
    ("Paste Special", "Специальная вставка"),
    ("Find and Replace", "Поиск и замена"),
    ("Accessibility Checker", "Проверка доступности"),
    ("Custom Shows", "Произвольные показы"),
    ("Chart Data", "Данные диаграммы"),
    ("Trim Media", "Обрезка мультимедиа"),
    ("Choose a SmartArt Graphic", "Выбор рисунка SmartArt"),
    ("Bullets and Numbering", "Маркеры и нумерация"),
    ("Action Settings", "Параметры действия"),
    ("Create New Theme Colors", "Создание цветов темы"),
    ("Create New Theme Fonts", "Создание шрифтов темы"),
    ("OK", "ОК"),
    ("Cancel", "Отмена"),
    ("Apply", "Применить"),
    ("Apply to All", "Применить ко всем"),
    ("Post", "Опубликовать"),
    ("Slides sized for:", "Размер слайдов:"),
    ("Custom", "Другой"),
    ("Orientation:", "Ориентация:"),
    ("Portrait", "Книжная"),
    ("Landscape", "Альбомная"),
    ("Maximize content (else ensure fit)", "Максимально увеличить содержимое (иначе вписать)"),
    ("Date and time", "Дата и время"),
    ("Slide number", "Номер слайда"),
    ("Footer", "Нижний колонтитул"),
    ("Don't show on title slide", "Не показывать на титульном слайде"),
    ("Web Page or File", "Веб-страница или файл"),
    ("This Document", "Этот документ"),
    ("Email Address", "Адрес электронной почты"),
    ("Email address:", "Адрес электронной почты:"),
    ("Address:", "Адрес:"),
    ("ScreenTip:", "Подсказка:"),
    ("Show type", "Тип показа"),
    ("Presented by a speaker (full screen)", "Под управлением докладчика (во весь экран)"),
    ("Browsed by an individual (window)", "Просмотр отдельным пользователем (в окне)"),
    ("Browsed at a kiosk (full screen)", "Просмотр в режиме киоска (во весь экран)"),
    ("Loop continuously until 'Esc'", "Непрерывный показ до нажатия Esc"),
    ("Show without narration", "Показывать без речевого сопровождения"),
    ("Show without animation", "Показывать без анимации"),
    ("Use timings, if present", "Использовать заданное время показа"),
    ("Keep mouse cursor visible", "Не скрывать указатель мыши"),
    ("Slides", "Слайды"),
    ("All slides", "Все слайды"),
    ("Don't Save", "Не сохранять"),
    ("Comment", "Комментарий"),
    ("Bulleted", "Маркированный список"),
    ("Numbered", "Нумерованный список"),
    ("Back", "Назад"),
    ("Do you want to save the changes you made to {what}?", "Сохранить изменения в {what}?"),
    ("Do you want to save the changes you made to “{name}”?", "Сохранить изменения в «{name}»?"),
    ("Your changes will be lost if you don't save them.", "Несохранённые изменения будут потеряны."),
    ("Slide {current} of {total}", "Слайд {current} из {total}"),
];

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
        RUSSIAN_CATALOG.iter().find_map(|(key, value)| (*key == english).then_some(*value)).unwrap_or(english)
    }

    pub fn localized(self, english: &str) -> String {
        if self == Self::English {
            return english.to_string();
        }
        let translated = self.text(english);
        if translated != english {
            return translated.to_string();
        }
        if let Some(name) = english.strip_suffix(" options") {
            let translated_name = self.text(name);
            if translated_name != name {
                return format!("{translated_name} {}", self.text("Options"));
            }
        }
        if let Some((name, shortcut)) = english.split_once(" (")
            && shortcut.ends_with(')')
        {
            let translated_name = self.text(name);
            if translated_name != name {
                return format!("{translated_name} ({shortcut}");
            }
        }
        english.to_string()
    }

    pub fn format_message(self, english: &str, values: &[(&str, &str)]) -> String {
        let mut message = self.text(english).to_string();
        for (key, value) in values {
            message = message.replace(&format!("{{{key}}}"), value);
        }
        message
    }

    pub fn presentation_word(self, count: u64) -> &'static str {
        match self {
            Self::English if count == 1 => "presentation",
            Self::English => "presentations",
            Self::Russian => ["презентация", "презентации", "презентаций"][russian_integer_form(count)],
        }
    }

    /// `current` and `total` are supplied by the caller's existing slide selection logic.
    pub fn slide_position(self, current: usize, total: usize) -> String {
        self.text("Slide {current} of {total}").replace("{current}", &current.to_string()).replace("{total}", &total.to_string())
    }
}

const LANGUAGE_CONTEXT_ID: &str = "deckcraft.interface-language";

pub fn set_context(ctx: &egui::Context, language: Language) {
    ctx.data_mut(|data| data.insert_temp(egui::Id::unique(LANGUAGE_CONTEXT_ID), language));
}

pub fn context_language(ctx: &egui::Context) -> Language {
    ctx.data(|data| data.get_temp(egui::Id::unique(LANGUAGE_CONTEXT_ID))).unwrap_or_default()
}

pub fn context_text(ctx: &egui::Context, english: &str) -> String {
    context_language(ctx).localized(english)
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

    fn placeholders(text: &str) -> Vec<&str> {
        let mut found = Vec::new();
        let mut rest = text;
        while let Some(open) = rest.find('{') {
            let after = &rest[open + 1..];
            let Some(close) = after.find('}') else { break };
            found.push(&after[..close]);
            rest = &after[close + 1..];
        }
        found
    }

    #[test]
    fn language_codes_round_trip_and_unknown_codes_use_english() {
        assert_eq!(serde_json::to_string(&Language::Russian).ok().as_deref(), Some("\"ru\""));
        assert_eq!(serde_json::to_string(&Language::English).ok().as_deref(), Some("\"en\""));
        assert_eq!(serde_json::from_str::<Language>("\"ru\"").ok(), Some(Language::Russian));
        assert_eq!(serde_json::from_str::<Language>("\"en\"").ok(), Some(Language::English));
        assert_eq!(serde_json::from_str::<Language>("\"unknown\"").ok(), Some(Language::English));
        assert_eq!(Language::default(), Language::English);
    }

    #[test]
    fn russian_catalog_has_unique_keys_and_preserves_placeholders() {
        let mut keys = std::collections::HashSet::new();
        for (english, russian) in RUSSIAN_CATALOG {
            assert!(keys.insert(english), "duplicate catalog key: {english}");
            assert!(!russian.trim().is_empty(), "empty translation for {english}");
            assert_eq!(placeholders(english), placeholders(russian), "placeholders for {english}");
        }
    }

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
    fn localized_suffixes_keep_shortcuts_and_translate_options() {
        assert_eq!(Language::Russian.localized("Bold (Cmd+B)"), "Полужирный (Cmd+B)");
        assert_eq!(Language::Russian.localized("Bold options"), "Полужирный параметры");
        assert_eq!(Language::English.localized("Bold (Cmd+B)"), "Bold (Cmd+B)");
    }

    #[test]
    fn named_substitutions_preserve_document_text_and_russian_plural_forms() {
        let name = "Quarterly Plan.pptx";
        assert_eq!(
            Language::Russian.format_message("Do you want to save the changes you made to “{name}”?", &[("name", name)]),
            "Сохранить изменения в «Quarterly Plan.pptx»?"
        );
        for (count, expected) in [(1, "презентация"), (2, "презентации"), (5, "презентаций"), (21, "презентация"), (11, "презентаций")]
        {
            assert_eq!(Language::Russian.presentation_word(count), expected);
        }
    }

    #[test]
    fn russian_integer_forms_cover_teens_and_large_counts() {
        for (count, expected) in [
            (0, 2),
            (1, 0),
            (2, 1),
            (4, 1),
            (5, 2),
            (11, 2),
            (12, 2),
            (14, 2),
            (21, 0),
            (22, 1),
            (25, 2),
            (101, 0),
            (111, 2),
            (112, 2),
            (114, 2),
            (121, 0),
            (122, 1),
            (u64::MAX, 2),
        ] {
            assert_eq!(russian_integer_form(count), expected, "count={count}");
        }
    }
}
