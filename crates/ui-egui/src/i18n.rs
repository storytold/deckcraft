//! Internationalization (i18n) and localization support for DeckCraft UI.

use serde::{Deserialize, Serialize};

/// Supported interface languages.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum UiLanguage {
    #[default]
    English,
    Turkish,
}

impl UiLanguage {
    pub const ALL: [UiLanguage; 2] = [UiLanguage::English, UiLanguage::Turkish];

    /// Native name of the language.
    pub fn name(self) -> &'static str {
        match self {
            UiLanguage::English => "English",
            UiLanguage::Turkish => "Türkçe",
        }
    }

    /// Two-letter language code (ISO 639-1).
    pub fn code(self) -> &'static str {
        match self {
            UiLanguage::English => "en",
            UiLanguage::Turkish => "tr",
        }
    }

    /// Translate a generic UI key with optional fallback.
    pub fn t(self, key: &'static str) -> &'static str {
        match self {
            UiLanguage::English => match key {
                "ok" => "OK",
                "cancel" => "Cancel",
                "save" => "Save",
                "dont_save" => "Don't Save",
                "close" => "Close",
                "done" => "Done",
                "insert" => "Insert",
                "rename" => "Rename",
                "post" => "Post",
                "export" => "Export",
                "create" => "Create",
                "delete" => "Delete",
                "fit_window" => "Fit slide to current window",
                "zoom_in" => "Zoom In",
                "zoom_out" => "Zoom Out",
                "slide_show" => "Slide Show",
                "reading_view" => "Reading View",
                "slide_sorter" => "Slide Sorter",
                "normal" => "Normal",
                "comments" => "Comments",
                "notes" => "Notes",
                "search_commands" => "Search commands (⇧⌘P)",
                "present" => "Present",
                "undo" => "Undo (⌘Z)",
                "redo" => "Redo (⌘Y)",
                "play_from_start" => "Play from Start (F5)",
                "show_all" => "Show All",
                "hide_all" => "Hide All",
                "send_backward" => "Send Backward",
                "bring_forward" => "Bring Forward",
                "language" => "Language",
                _ => key,
            },
            UiLanguage::Turkish => match key {
                "ok" => "Tamam",
                "cancel" => "İptal",
                "save" => "Kaydet",
                "dont_save" => "Kaydetme",
                "close" => "Kapat",
                "done" => "Bitti",
                "insert" => "Ekle",
                "rename" => "Yeniden Adlandır",
                "post" => "Gönder",
                "export" => "Dışa Aktar",
                "create" => "Oluştur",
                "delete" => "Sil",
                "fit_window" => "Slaytı pencereye sığdır",
                "zoom_in" => "Yakınlaştır",
                "zoom_out" => "Uzaklaştır",
                "slide_show" => "Slayt Gösterisi",
                "reading_view" => "Okuma Görünümü",
                "slide_sorter" => "Slayt Sıralayıcısı",
                "normal" => "Normal",
                "comments" => "Açıklamalar",
                "notes" => "Notlar",
                "search_commands" => "Komut ara (⇧⌘P)",
                "present" => "Sunum Yap",
                "undo" => "Geri Al (⌘Z)",
                "redo" => "Yinele (⌘Y)",
                "play_from_start" => "Baştan Başlat (F5)",
                "show_all" => "Tümünü Göster",
                "hide_all" => "Tümünü Gizle",
                "send_backward" => "Arkaya Gönder",
                "bring_forward" => "Öne Getir",
                "language" => "Dil",
                _ => key,
            },
        }
    }

    /// Localized ribbon tab title.
    pub fn ribbon_tab(self, tab: &'static str) -> &'static str {
        match self {
            UiLanguage::English => tab,
            UiLanguage::Turkish => match tab {
                "File" => "Dosya",
                "Home" => "Giriş",
                "Insert" => "Ekle",
                "Draw" => "Çizim",
                "Design" => "Tasarım",
                "Transitions" => "Geçişler",
                "Animations" => "Animasyonlar",
                "Slide Show" => "Slayt Gösterisi",
                "Record" => "Kayıt",
                "Review" => "Gözden Geçir",
                "View" => "Görünüm",
                "Slide Master" => "Asıl Slayt",
                "Shape Format" => "Şekil Biçimi",
                "Picture Format" => "Resim Biçimi",
                "Table Design" => "Tablo Tasarımı",
                "Layout" => "Düzen",
                "Chart Design" => "Grafik Tasarımı",
                "Playback" => "Kayıttan Yürütme",
                _ => tab,
            },
        }
    }

    /// Localized backstage navigation/page title.
    pub fn backstage_page(self, id: &'static str) -> &'static str {
        match self {
            UiLanguage::English => match id {
                "home" | "Home" => "Home",
                "new" | "New" => "New",
                "open" | "Open" => "Open",
                "info" | "Info" => "Info",
                "saveAs" | "Save As" => "Save As",
                "print" | "Print" => "Print",
                "export" | "Export" => "Export",
                "account" | "Account" => "Account",
                "save" | "Save" => "Save",
                "close" | "Close" => "Close",
                "options" | "Options" => "Options",
                _ => id,
            },
            UiLanguage::Turkish => match id {
                "home" | "Home" => "Giriş",
                "new" | "New" => "Yeni",
                "open" | "Open" => "Aç",
                "info" | "Info" => "Bilgi",
                "saveAs" | "Save As" => "Farklı Kaydet",
                "print" | "Print" => "Yazdır",
                "export" | "Export" => "Dışa Aktar",
                "account" | "Account" => "Hesap",
                "save" | "Save" => "Kaydet",
                "close" | "Close" => "Kapat",
                "options" | "Options" => "Seçenekler",
                _ => id,
            },
        }
    }

    /// Localized dialog window title.
    pub fn dialog_title(self, id: &str) -> &'static str {
        match self {
            UiLanguage::English => match id {
                "table" => "Insert Table",
                "slideSize" => "Slide Size",
                "headerFooter" => "Header and Footer",
                "hyperlink" => "Insert Hyperlink",
                "setupShow" => "Set Up Slide Show",
                "zoom" => "Zoom",
                "comment" => "New Comment",
                "altText" => "Alt Text",
                "outline" => "Slides from Outline",
                "renameSection" => "Rename Section",
                "renameShape" => "Rename",
                "renameLayout" => "Rename Layout",
                "paragraph" => "Paragraph",
                "about" => "About DeckCraft",
                "preferences" => "Preferences",
                "export" => "Export",
                "symbol" => "Symbol",
                "equation" => "Equation",
                "spelling" => "Spelling",
                "accessibility" => "Accessibility Checker",
                "language" => "Language",
                "customShow" => "Custom Shows",
                "chartData" => "Chart Data",
                "trim" => "Trim Media",
                "smartart" => "Choose a SmartArt Graphic",
                "sizePosition" => "Size and Position",
                "tabs" => "Tabs",
                "font" => "Font",
                "autocorrect" => "AutoCorrect",
                "gridOptions" => "Grid Options",
                "photoAlbum" => "Photo Album",
                "replaceFonts" => "Replace Font",
                "dateTime" => "Date and Time",
                "pasteSpecial" => "Paste Special",
                "find" => "Find and Replace",
                "bullets" => "Bullets and Numbering",
                "action" => "Action Settings",
                "themeColors" => "Create New Theme Colors",
                "themeFonts" => "Create New Theme Fonts",
                "quit" => "DeckCraft",
                _ => "DeckCraft",
            },
            UiLanguage::Turkish => match id {
                "table" => "Tablo Ekle",
                "slideSize" => "Slayt Boyutu",
                "headerFooter" => "Üstbilgi ve Altbilgi",
                "hyperlink" => "Köprü Ekle",
                "setupShow" => "Slayt Gösterisini Ayarla",
                "zoom" => "Yakınlaştır",
                "comment" => "Yeni Açıklama",
                "altText" => "Alternatif Metin",
                "outline" => "Anahattan Slaytlar",
                "renameSection" => "Bölümü Yeniden Adlandır",
                "renameShape" => "Yeniden Adlandır",
                "renameLayout" => "Düzeni Yeniden Adlandır",
                "paragraph" => "Paragraf",
                "about" => "DeckCraft Hakkında",
                "preferences" => "Tercihler",
                "export" => "Dışa Aktar",
                "symbol" => "Simge",
                "equation" => "Denklem",
                "spelling" => "Yazım Denetimi",
                "accessibility" => "Erişilebilirlik Denetleyicisi",
                "language" => "Dil",
                "customShow" => "Özel Gösteriler",
                "chartData" => "Grafik Verisi",
                "trim" => "Medyayı Kırp",
                "smartart" => "SmartArt Grafiği Seç",
                "sizePosition" => "Boyut ve Konum",
                "tabs" => "Sekmeler",
                "font" => "Yazı Tipi",
                "autocorrect" => "Otomatik Düzelt",
                "gridOptions" => "Izgara Seçenekleri",
                "photoAlbum" => "Fotoğraf Albümü",
                "replaceFonts" => "Yazı Tiplerini Değiştir",
                "dateTime" => "Tarih ve Saat",
                "pasteSpecial" => "Özel Yapıştır",
                "find" => "Bul ve Değiştir",
                "bullets" => "Madde İşaretleri ve Numaralandırma",
                "action" => "Eylem Ayarları",
                "themeColors" => "Yeni Tema Renkleri Oluştur",
                "themeFonts" => "Yeni Tema Yazı Tipleri Oluştur",
                "quit" => "DeckCraft",
                _ => "DeckCraft",
            },
        }
    }

    /// Localized task pane title.
    pub fn pane_title(self, id: &str) -> &'static str {
        match self {
            UiLanguage::English => match id {
                "format" => "Format Shape",
                "background" => "Format Background",
                "animation" => "Animation Pane",
                "selection" => "Selection",
                "comments" => "Comments",
                "designer" => "Design Ideas",
                "smartart" => "Text Pane",
                "thesaurus" => "Thesaurus",
                "revisions" => "Revisions",
                _ => "Pane",
            },
            UiLanguage::Turkish => match id {
                "format" => "Şekli Biçimlendir",
                "background" => "Arka Planı Biçimlendir",
                "animation" => "Animasyon Bölmesi",
                "selection" => "Seçim",
                "comments" => "Açıklamalar",
                "designer" => "Tasarım Fikirleri",
                "smartart" => "Metin Bölmesi",
                "thesaurus" => "Eşanlamlılar Sözlüğü",
                "revisions" => "Düzeltmeler",
                _ => "Bölme",
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_language_defaults_and_switching() {
        let en = UiLanguage::English;
        let tr = UiLanguage::Turkish;

        assert_eq!(en.code(), "en");
        assert_eq!(tr.code(), "tr");

        assert_eq!(en.name(), "English");
        assert_eq!(tr.name(), "Türkçe");

        assert_eq!(en.t("ok"), "OK");
        assert_eq!(tr.t("ok"), "Tamam");

        assert_eq!(en.t("cancel"), "Cancel");
        assert_eq!(tr.t("cancel"), "İptal");

        assert_eq!(en.ribbon_tab("Home"), "Home");
        assert_eq!(tr.ribbon_tab("Home"), "Giriş");
        assert_eq!(tr.ribbon_tab("Insert"), "Ekle");
        assert_eq!(tr.ribbon_tab("Transitions"), "Geçişler");

        assert_eq!(en.backstage_page("home"), "Home");
        assert_eq!(tr.backstage_page("home"), "Giriş");
        assert_eq!(tr.backstage_page("saveAs"), "Farklı Kaydet");

        assert_eq!(en.dialog_title("preferences"), "Preferences");
        assert_eq!(tr.dialog_title("preferences"), "Tercihler");

        assert_eq!(en.pane_title("selection"), "Selection");
        assert_eq!(tr.pane_title("selection"), "Seçim");
    }
}
