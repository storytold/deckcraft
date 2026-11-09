//! Interface language for the shell, dialogs, task panes and Home ribbon controls.
//!
//! The catalog changes display text only. Command ids, preference keys and presentation content
//! keep their original values. Japanese strings are written from the English meaning; they are
//! not copied from another product's localization. Galleries and engine diagnostics still have
//! English labels where no translation is supplied.

use serde::{Deserialize, Deserializer, Serialize, Serializer};

/// The choice saved in preferences. `System` is resolved when the interface is drawn.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum LanguageChoice {
    #[default]
    System,
    En,
    Ja,
}

impl LanguageChoice {
    pub const ALL: [LanguageChoice; 3] = [Self::System, Self::En, Self::Ja];

    /// Return this language's stable command or preference code.
    pub fn code(self) -> &'static str {
        match self {
            Self::System => "system",
            Self::En => "en",
            Self::Ja => "ja",
        }
    }

    /// `system`, `en` (and `en-*`), or `ja` / `ja-JP`.
    pub fn parse(raw: &str) -> Option<Self> {
        let tag = normalize(raw);
        if tag == "system" {
            return Some(Self::System);
        }
        if tag == "en" || tag.starts_with("en-") {
            return Some(Self::En);
        }
        if tag == "ja" || tag.starts_with("ja-") {
            return Some(Self::Ja);
        }
        None
    }

    /// Resolve an explicit choice or system locale to a supported display language.
    pub fn resolve(self, system: Option<&str>) -> Resolved {
        match self {
            Self::En => Resolved::En,
            Self::Ja => Resolved::Ja,
            Self::System => system.map(resolve_tag).unwrap_or(Resolved::En),
        }
    }
}

impl Serialize for LanguageChoice {
    /// Save the language choice as its stable code rather than a translated caption.
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(self.code())
    }
}

impl<'de> Deserialize<'de> for LanguageChoice {
    /// Read a saved language choice, falling back to System for unknown codes.
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let raw = String::deserialize(deserializer)?;
        Ok(Self::parse(&raw).unwrap_or(Self::System))
    }
}

/// The language actually used to draw.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Resolved {
    En,
    Ja,
}

impl Resolved {
    /// Return this language's stable command or preference code.
    pub fn code(self) -> &'static str {
        match self {
            Self::En => "en",
            Self::Ja => "ja",
        }
    }
}

/// Map an OS locale tag to a catalog. Unknown tags are English.
pub fn resolve_tag(tag: &str) -> Resolved {
    let tag = normalize(tag);
    if tag == "ja" || tag.starts_with("ja-") {
        return Resolved::Ja;
    }
    Resolved::En
}

/// Normalize locale separators, case and encoding suffixes for catalog selection.
fn normalize(raw: &str) -> String {
    let trimmed = raw.trim();
    let head = trimmed.split(['.', '@']).next().unwrap_or(trimmed);
    head.replace('_', "-").to_ascii_lowercase()
}

/// The operating system's locale tag, read once. The web build has no system tag here, so
/// `System` stays English until the user picks a language.
pub fn system_tag() -> Option<String> {
    static CACHED: std::sync::OnceLock<Option<String>> = std::sync::OnceLock::new();
    CACHED.get_or_init(read_system_tag).clone()
}

/// Read the platform locale when available; the web build has no native locale here.
fn read_system_tag() -> Option<String> {
    #[cfg(not(target_arch = "wasm32"))]
    let tag = sys_locale::get_locale();
    #[cfg(target_arch = "wasm32")]
    let tag: Option<String> = None;
    // Tests pin System to English. A machine whose OS language is Japanese would otherwise
    // rename the labels the UI tests look up. Tag mapping is covered on its own.
    #[cfg(test)]
    {
        let _ = tag;
        None
    }
    #[cfg(not(test))]
    tag
}

/// `(id, English, Japanese)`. Ids are sorted so lookup can binary-search.
const CATALOG: &[(&str, &str, &str)] = &[
    ("action.apply", "Apply", "適用"),
    ("action.apply_all", "Apply to All", "すべてに適用"),
    ("action.create", "Create", "作成"),
    ("action.delete", "Delete", "削除"),
    ("action.dont_save", "Don't Save", "保存しない"),
    ("action.go_to", "Go to", "移動"),
    ("action.post", "Post", "投稿"),
    ("dialog.about.community", "Join our community on Discord", "Discordのコミュニティに参加"),
    ("dialog.about.contributors", "Contributors", "貢献者"),
    (
        "dialog.about.description",
        "Presentations and slide shows, rebuilt from scratch in pure Rust.",
        "Rustでゼロから開発した、プレゼンテーションとスライドショーのアプリです。",
    ),
    ("dialog.about.models", "Models", "AIモデル"),
    (
        "dialog.about.project",
        "An open-source, clean-room project. Not affiliated with Microsoft.",
        "オープンソースのクリーンルーム開発プロジェクトです。Microsoftとは関係ありません。",
    ),
    ("dialog.about.source", "Source on GitHub", "GitHubのソースコード"),
    ("dialog.about.tab", "About", "このアプリについて"),
    ("dialog.about.title", "About DeckCraft", "DeckCraftについて"),
    ("dialog.accessibility.title", "Accessibility Checker", "アクセシビリティチェック"),
    (
        "dialog.alt_text.description",
        "How would you describe this object and its context to someone who is blind or has low vision?",
        "このオブジェクトとその前後の内容を、画面が見えない方や見えにくい方にどう説明しますか？",
    ),
    ("dialog.alt_text.title", "Alt Text", "代替テキスト"),
    ("dialog.chart.add_row", "+ Row", "+ 行"),
    ("dialog.chart.add_series", "+ Series", "+ 系列"),
    (
        "dialog.chart.description",
        "Edit the data: first row = series names, first column = categories.",
        "データを編集してください。先頭行は系列名、先頭列は項目です。",
    ),
    ("dialog.chart.select", "Select a chart first.", "先にグラフを選択してください。"),
    ("dialog.chart.title", "Chart Data", "グラフのデータ"),
    ("dialog.comment.hint", "Start a conversation", "コメントを入力"),
    ("dialog.comment.title", "New Comment", "新しいコメント"),
    ("dialog.custom_show.name", "New show name:", "新しいスライドショーの名前:"),
    ("dialog.custom_show.slides", "Slides (e.g. 1,3,5):", "スライド（例: 1,3,5）:"),
    ("dialog.custom_show.summary", "{name} ({n} slides)", "{name}（{n} スライド）"),
    ("dialog.custom_show.title", "Custom Shows", "目的別スライドショー"),
    (
        "dialog.equation.description",
        "Type an equation in linear form (e.g. a^2+b^2=c^2, x=(-b±√(b^2-4ac))/2a).",
        "数式を1行の形式で入力してください（例: a^2+b^2=c^2, x=(-b±√(b^2-4ac))/2a）。",
    ),
    ("dialog.equation.title", "Equation", "数式"),
    ("dialog.export.description", "Export the presentation as:", "プレゼンテーションの出力形式:"),
    ("dialog.export.full_page", "Full Page Slides", "ページ全体にスライドを配置"),
    ("dialog.export.handout_1", "Handouts (1 slide per page)", "配布資料（1ページに1スライド）"),
    ("dialog.export.handout_2", "Handouts (2 slides per page)", "配布資料（1ページに2スライド）"),
    ("dialog.export.handout_3", "Handouts (3 slides per page)", "配布資料（1ページに3スライド）"),
    ("dialog.export.handout_4", "Handouts (4 slides per page)", "配布資料（1ページに4スライド）"),
    ("dialog.export.handout_6", "Handouts (6 slides per page)", "配布資料（1ページに6スライド）"),
    ("dialog.export.handout_9", "Handouts (9 slides per page)", "配布資料（1ページに9スライド）"),
    ("dialog.export.hidden", "Include hidden slides", "非表示のスライドを含める"),
    ("dialog.export.jpeg", "JPEG image (current slide)", "JPEG画像（現在のスライド）"),
    ("dialog.export.layout", "Print layout:", "印刷レイアウト:"),
    ("dialog.export.notes", "Notes Pages", "ノート付きのページ"),
    ("dialog.export.outline", "Outline (.txt)", "アウトライン（.txt）"),
    ("dialog.export.pdf", "PDF document (.pdf)", "PDF文書（.pdf）"),
    ("dialog.export.png", "PNG images (current slide)", "PNG画像（現在のスライド）"),
    ("dialog.export.png_all", "PNG images (all slides)", "PNG画像（すべてのスライド）"),
    ("dialog.export.pptx", "PowerPoint Presentation (.pptx)", "PowerPointプレゼンテーション（.pptx）"),
    ("dialog.header_footer.auto_date", "  Fixed date (empty = update automatically):", "  固定の日付（空欄なら自動更新）:"),
    ("dialog.header_footer.date", "Date and time", "日付と時刻"),
    ("dialog.header_footer.footer", "Footer", "フッター"),
    ("dialog.header_footer.footer_text", "  Footer text:", "  フッターのテキスト:"),
    ("dialog.header_footer.hide_title", "Don't show on title slide", "タイトルスライドには表示しない"),
    ("dialog.header_footer.number", "Slide number", "スライド番号"),
    ("dialog.header_footer.title", "Header and Footer", "ヘッダーとフッター"),
    ("dialog.hyperlink.address", "Address:", "アドレス:"),
    ("dialog.hyperlink.document", "This Document", "このプレゼンテーション内"),
    ("dialog.hyperlink.email", "Email Address", "メールアドレス"),
    ("dialog.hyperlink.email_field", "Email address:", "メールアドレス:"),
    ("dialog.hyperlink.tip", "ScreenTip:", "ヒント:"),
    ("dialog.hyperlink.title", "Insert Hyperlink", "ハイパーリンクの挿入"),
    ("dialog.hyperlink.web", "Web Page or File", "Webページまたはファイル"),
    ("dialog.language.description", "Mark selected text as:", "選択したテキストの言語:"),
    ("dialog.language.title", "Language", "言語"),
    (
        "dialog.outline.description",
        "Paste or type an outline. Unindented lines become slide titles; tab-indented lines become bullets.",
        "アウトラインを貼り付けるか入力してください。字下げのない行はスライドのタイトル、タブで字下げした行は箇条書きになります。",
    ),
    ("dialog.outline.title", "Slides from Outline", "アウトラインからスライドを作成"),
    ("dialog.paragraph.after", "Spacing after (pt):", "段落後の間隔（pt）:"),
    ("dialog.paragraph.before", "Spacing before (pt):", "段落前の間隔（pt）:"),
    ("dialog.paragraph.hanging", "Special — hanging (in):", "ぶら下げ幅（in）:"),
    ("dialog.paragraph.left", "Before text (in):", "左インデント（in）:"),
    ("dialog.paragraph.lines", "Line spacing (lines):", "行間（行）:"),
    ("dialog.paragraph.title", "Paragraph", "段落"),
    ("dialog.rename.name", "Name:", "名前:"),
    ("dialog.rename.title", "Rename", "名前の変更"),
    ("dialog.rename_layout.title", "Rename Layout", "レイアウト名の変更"),
    ("dialog.rename_section.title", "Rename Section", "セクション名の変更"),
    (
        "dialog.review.accessibility_complete",
        "No accessibility issues found. People with disabilities should not have difficulty reading this document.",
        "アクセシビリティの問題は見つかりませんでした。",
    ),
    ("dialog.review.issue", "Slide {n}: {issue}{name}", "スライド {n}: {issue}{name}"),
    (
        "dialog.review.spelling_complete",
        "The spelling check is complete. No issues found.",
        "スペルチェックが完了しました。問題は見つかりませんでした。",
    ),
    ("dialog.review.word", "Slide {n}: “{word}”", "スライド {n}:「{word}」"),
    ("dialog.save.close_question", "Do you want to save the changes you made to “{name}”?", "「{name}」への変更を保存しますか？"),
    ("dialog.save.many", "{n} presentations", "{n} 件のプレゼンテーション"),
    ("dialog.save.question", "Do you want to save the changes you made to {what}?", "{what}への変更を保存しますか？"),
    ("dialog.save.warning", "Your changes will be lost if you don't save them.", "保存しない場合、変更内容は失われます。"),
    ("dialog.setup_show.animation", "Show without animation", "アニメーションなしで表示"),
    ("dialog.setup_show.browsed", "Browsed by an individual (window)", "各自で閲覧（ウィンドウ表示）"),
    ("dialog.setup_show.kiosk", "Browsed at a kiosk (full screen)", "自動プレゼンテーション（全画面表示）"),
    ("dialog.setup_show.loop", "Loop continuously until 'Esc'", "Escキーを押すまで繰り返す"),
    ("dialog.setup_show.narration", "Show without narration", "ナレーションなしで表示"),
    ("dialog.setup_show.speaker", "Presented by a speaker (full screen)", "発表者によるプレゼンテーション（全画面表示）"),
    ("dialog.setup_show.timings", "Use timings, if present", "設定済みのタイミングを使用"),
    ("dialog.setup_show.title", "Set Up Slide Show", "スライドショーの設定"),
    ("dialog.setup_show.type", "Show type", "スライドショーの種類"),
    ("dialog.slide_size.custom", "Custom", "ユーザー設定"),
    ("dialog.slide_size.height", "Height (in):", "高さ（in）:"),
    ("dialog.slide_size.maximize", "Maximize content (else ensure fit)", "内容を最大化する（オフならスライド内に収める）"),
    ("dialog.slide_size.preset", "Slides sized for:", "スライドのサイズ:"),
    ("dialog.slide_size.title", "Slide Size", "スライドのサイズ"),
    ("dialog.slide_size.width", "Width (in):", "幅（in）:"),
    ("dialog.smartart.cycle", "Basic Cycle", "基本の循環図"),
    ("dialog.smartart.description", "Pick a graphic; type one item per line.", "図を選び、1行に1つずつ項目を入力してください。"),
    ("dialog.smartart.hierarchy", "Hierarchy", "階層図"),
    ("dialog.smartart.list", "Basic List", "基本のリスト"),
    ("dialog.smartart.matrix", "Matrix", "マトリックス"),
    ("dialog.smartart.process", "Basic Process", "基本の手順図"),
    ("dialog.smartart.pyramid", "Pyramid", "ピラミッド"),
    ("dialog.smartart.title", "Choose a SmartArt Graphic", "SmartArtの選択"),
    ("dialog.spelling.title", "Spelling", "スペルチェック"),
    ("dialog.symbol.arrows", "Arrows", "矢印"),
    ("dialog.symbol.currency", "Currency", "通貨"),
    ("dialog.symbol.description", "Click a symbol to insert it at the insertion point.", "記号をクリックすると、カーソル位置に挿入します。"),
    ("dialog.symbol.greek", "Greek", "ギリシャ文字"),
    ("dialog.symbol.math", "Math", "数学記号"),
    ("dialog.symbol.punctuation", "Punctuation", "句読点"),
    ("dialog.symbol.shapes", "Shapes", "図形"),
    ("dialog.symbol.title", "Symbol", "記号"),
    ("dialog.table.columns", "Number of columns:", "列数:"),
    ("dialog.table.rows", "Number of rows:", "行数:"),
    ("dialog.table.title", "Insert Table", "表の挿入"),
    ("dialog.trim.end", "End trim (s):", "末尾のトリミング（秒）:"),
    ("dialog.trim.fade_in", "Fade in (s):", "フェードイン（秒）:"),
    ("dialog.trim.fade_out", "Fade out (s):", "フェードアウト（秒）:"),
    ("dialog.trim.start", "Start time (s):", "開始時刻（秒）:"),
    ("dialog.trim.title", "Trim Media", "メディアのトリミング"),
    ("dialog.unavailable", "“{id}” is not available yet.", "「{id}」はまだ利用できません。"),
    ("dialog.zoom.fit", "Fit", "ウィンドウに合わせる"),
    ("dialog.zoom.title", "Zoom", "ズーム"),
    ("nav.account", "Account", "アカウント"),
    ("nav.back", "Back", "戻る"),
    ("nav.close", "Close", "閉じる"),
    ("nav.export", "Export", "エクスポート"),
    ("nav.home", "Home", "ホーム"),
    ("nav.info", "Info", "情報"),
    ("nav.new", "New", "新規"),
    ("nav.open", "Open", "開く"),
    ("nav.options", "Options", "オプション"),
    ("nav.print", "Print", "印刷"),
    ("nav.save", "Save", "保存"),
    ("nav.save_as", "Save As", "名前を付けて保存"),
    ("pane.animation.after_previous", "Start After Previous", "直前の動作の後に開始"),
    (
        "pane.animation.description",
        "Select an object on the slide and add an animation from the Animations tab.",
        "スライド上のオブジェクトを選択し、アニメーションタブから効果を追加してください。",
    ),
    ("pane.animation.move_down", "Move Down", "下に移動"),
    ("pane.animation.move_up", "Move Up", "上に移動"),
    ("pane.animation.on_click", "Start On Click", "クリック時に開始"),
    ("pane.animation.play_all", "▶ Play All", "▶ すべて再生"),
    ("pane.animation.remove", "Remove", "削除"),
    ("pane.animation.shape", "Shape {id}", "図形 {id}"),
    ("pane.animation.title", "Animation Pane", "アニメーションウィンドウ"),
    ("pane.animation.with_previous", "Start With Previous", "直前の動作と同時に開始"),
    ("pane.background.follow_layout", "Follow the layout", "レイアウトに合わせる"),
    ("pane.background.hide_graphics", "Hide background graphics", "背景の図形を非表示"),
    ("pane.background.insert_picture", "Insert picture from file…", "ファイルから画像を挿入…"),
    ("pane.background.reset", "Reset Background", "背景をリセット"),
    ("pane.background.title", "Format Background", "背景の書式設定"),
    ("pane.comments.empty", "No comments.", "コメントはありません。"),
    ("pane.comments.new", "+ New Comment", "+ 新しいコメント"),
    ("pane.comments.reopen", "Reopen", "再度開く"),
    ("pane.comments.reply", "Reply…", "返信…"),
    ("pane.comments.resolve", "Resolve", "解決済みにする"),
    (
        "pane.designer.description",
        "Pick a look for your slides. Ideas are generated on your device.",
        "スライドの見た目を選んでください。デザイン候補はこの端末上で生成します。",
    ),
    ("pane.designer.title", "Design Ideas", "デザインの候補"),
    ("pane.format.alt_text", "Alt Text", "代替テキスト"),
    ("pane.format.automatic", "Automatic", "自動"),
    ("pane.format.background", "Background", "背景色"),
    ("pane.format.bottom", "Bottom", "下"),
    ("pane.format.brightness", "Brightness", "明るさ"),
    ("pane.format.center", "Center", "中央"),
    ("pane.format.color", "Color", "色"),
    ("pane.format.columns", "Columns", "段組み"),
    ("pane.format.contrast", "Contrast", "コントラスト"),
    ("pane.format.dash", "Dash", "破線"),
    ("pane.format.effects", "Effects", "効果"),
    ("pane.format.fill", "Fill", "塗りつぶし"),
    ("pane.format.fill_line", "Fill & Line", "塗りつぶしと線"),
    ("pane.format.foreground", "Foreground", "前景色"),
    ("pane.format.full", "Full", "全体"),
    ("pane.format.glow", "Glow", "光彩"),
    ("pane.format.gradient", "Gradient fill", "グラデーションの塗りつぶし"),
    ("pane.format.half", "Half", "半分"),
    ("pane.format.height", "Height", "高さ"),
    ("pane.format.horizontal", "Horizontal", "横位置"),
    ("pane.format.inner", "Inner", "内側"),
    ("pane.format.left", "Left", "左"),
    ("pane.format.line", "Line", "線"),
    ("pane.format.margins", "Margins (pt)", "余白（pt）"),
    ("pane.format.middle", "Middle", "中央"),
    ("pane.format.no_autofit", "Do not Autofit", "自動調整しない"),
    ("pane.format.no_fill", "No fill", "塗りつぶしなし"),
    ("pane.format.no_fill_color", "No Fill", "塗りつぶしなし"),
    ("pane.format.no_line", "No line", "線なし"),
    ("pane.format.no_outline", "No Outline", "枠線なし"),
    ("pane.format.none", "None", "なし"),
    ("pane.format.outer", "Outer", "外側"),
    ("pane.format.pattern", "Pattern fill", "パターンの塗りつぶし"),
    ("pane.format.perspective", "Perspective", "遠近"),
    ("pane.format.picture", "Picture", "画像"),
    ("pane.format.picture_fill", "Picture or texture fill", "画像またはテクスチャの塗りつぶし"),
    ("pane.format.position", "Position", "位置"),
    ("pane.format.reflection", "Reflection", "反射"),
    ("pane.format.reset_effects", "Reset Effects", "効果をリセット"),
    ("pane.format.reset_picture", "Reset Picture", "画像をリセット"),
    ("pane.format.resize_text", "Resize shape to fit text", "テキストに合わせて図形のサイズを調整"),
    ("pane.format.right", "Right", "右"),
    ("pane.format.rotate", "Rotate with shape", "図形と一緒に回転"),
    ("pane.format.rotation", "Rotation", "回転"),
    ("pane.format.saturation", "Saturation", "彩度"),
    ("pane.format.select_shape", "Select a shape to format it.", "書式を設定する図形を選択してください。"),
    ("pane.format.shrink", "Shrink text on overflow", "はみ出す場合はテキストを縮小"),
    ("pane.format.size", "Size", "サイズ"),
    ("pane.format.size_properties", "Size & Properties", "サイズとプロパティ"),
    ("pane.format.size_pt", "size (pt)", "サイズ（pt）"),
    ("pane.format.soft_edges", "Soft Edges", "ぼかし"),
    ("pane.format.solid_fill", "Solid fill", "単色の塗りつぶし"),
    ("pane.format.solid_line", "Solid line", "実線"),
    ("pane.format.text_box", "Text Box", "テキストボックス"),
    ("pane.format.text_options", "Text Options", "テキストの設定"),
    ("pane.format.tight", "Tight", "密接"),
    ("pane.format.title", "Format Shape", "図形の書式設定"),
    ("pane.format.top", "Top", "上"),
    ("pane.format.transparency", "Transparency", "透明度"),
    ("pane.format.vertical", "Vertical", "縦位置"),
    ("pane.format.vertical_alignment", "Vertical alignment", "垂直方向の配置"),
    ("pane.format.width", "Width", "幅"),
    ("pane.format.wrap", "Wrap text in shape", "図形内でテキストを折り返す"),
    ("pane.selection.bring_forward", "Bring Forward", "前面へ移動"),
    ("pane.selection.bring_front", "Bring to Front", "最前面へ移動"),
    ("pane.selection.collapse", "Collapse {name}", "{name}を折りたたむ"),
    ("pane.selection.expand", "Expand {name}", "{name}を展開"),
    ("pane.selection.hide", "Hide", "非表示"),
    ("pane.selection.hide_all", "Hide All", "すべて非表示"),
    ("pane.selection.hide_name", "Hide {name}", "{name}を非表示"),
    ("pane.selection.lock", "Lock", "ロック"),
    ("pane.selection.lock_name", "Lock {name}", "{name}をロック"),
    ("pane.selection.rename", "Rename", "名前の変更"),
    ("pane.selection.send_back", "Send to Back", "最背面へ移動"),
    ("pane.selection.send_backward", "Send Backward", "背面へ移動"),
    ("pane.selection.show", "Show", "表示"),
    ("pane.selection.show_all", "Show All", "すべて表示"),
    ("pane.selection.show_name", "Show {name}", "{name}を表示"),
    ("pane.selection.title", "Selection", "選択"),
    ("pane.selection.unlock", "Unlock", "ロック解除"),
    ("pane.selection.unlock_name", "Unlock {name}", "{name}のロックを解除"),
    ("pane.title", "Pane", "作業ウィンドウ"),
    ("pref.author", "Your name (comments):", "名前（コメント）:"),
    ("pref.autocorrect", "AutoCorrect as you type", "入力しながら自動修正する"),
    ("pref.cancel", "Cancel", "キャンセル"),
    ("pref.dark", "Dark appearance", "ダーク表示"),
    ("pref.done", "Done", "完了"),
    ("pref.editing", "Editing", "編集"),
    ("pref.english", "English", "English"),
    ("pref.general", "General", "全般"),
    ("pref.ja", "日本語", "日本語"),
    ("pref.language", "Interface language", "表示言語"),
    ("pref.quotes", "Replace straight quotes with smart quotes", "直線の引用符を曲がった引用符に置き換える"),
    ("pref.size", "Interface size", "表示の大きさ"),
    ("pref.smart_guides", "Show smart guides", "スマートガイドを表示する"),
    ("pref.snap", "Snap objects to grid", "オブジェクトをグリッドに合わせる"),
    ("pref.system", "System", "システムに合わせる"),
    ("pref.title", "Preferences", "環境設定"),
    ("ribbon.align.bottom", "Align Bottom", "下揃え"),
    ("ribbon.align.center", "Align Center", "左右中央揃え"),
    ("ribbon.align.left", "Align Left", "左揃え"),
    ("ribbon.align.middle", "Align Middle", "上下中央揃え"),
    ("ribbon.align.right", "Align Right", "右揃え"),
    ("ribbon.align.top", "Align Top", "上揃え"),
    ("ribbon.arrange.align", "Align", "配置を揃える"),
    ("ribbon.arrange.distribute_h", "Distribute Horizontally", "左右に等間隔で配置"),
    ("ribbon.arrange.distribute_v", "Distribute Vertically", "上下に等間隔で配置"),
    ("ribbon.arrange.flip_h", "Flip Horizontal", "左右反転"),
    ("ribbon.arrange.flip_v", "Flip Vertical", "上下反転"),
    ("ribbon.arrange.group", "Group", "グループ化"),
    ("ribbon.arrange.group_heading", "Group Objects", "オブジェクトのグループ化"),
    ("ribbon.arrange.order_heading", "Order Objects", "オブジェクトの重なり順"),
    ("ribbon.arrange.position_heading", "Position Objects", "オブジェクトの位置"),
    ("ribbon.arrange.regroup", "Regroup", "再グループ化"),
    ("ribbon.arrange.rotate", "Rotate", "回転"),
    ("ribbon.arrange.rotate_left", "Rotate Left 90°", "左に90度回転"),
    ("ribbon.arrange.rotate_right", "Rotate Right 90°", "右に90度回転"),
    ("ribbon.arrange.selection", "Selection Pane…", "選択ウィンドウ…"),
    ("ribbon.arrange.ungroup", "Ungroup", "グループ解除"),
    ("ribbon.clipboard.copy", "Copy", "コピー"),
    ("ribbon.clipboard.cut", "Cut", "切り取り"),
    ("ribbon.clipboard.format", "Format", "書式"),
    ("ribbon.clipboard.painter_tip", "Format Painter (double-click to keep it on)", "書式のコピー（ダブルクリックすると連続して使用）"),
    ("ribbon.clipboard.paste", "Paste", "貼り付け"),
    ("ribbon.clipboard.paste_format", "Paste Formatting", "書式を貼り付け"),
    ("ribbon.clipboard.paste_match", "Paste and Match Formatting", "貼り付け先の書式に合わせて貼り付け"),
    ("ribbon.clipboard.paste_tip", "Paste (⌘V)", "貼り付け（⌘V）"),
    ("ribbon.drawing.arrange", "Arrange", "配置"),
    ("ribbon.drawing.effects", "Shape Effects", "図形の効果"),
    ("ribbon.drawing.fill", "Shape Fill", "図形の塗りつぶし"),
    ("ribbon.drawing.outline", "Shape Outline", "図形の枠線"),
    ("ribbon.drawing.quick_styles", "Quick\nStyles", "クイック\nスタイル"),
    ("ribbon.drawing.shapes", "Shapes", "図形"),
    ("ribbon.font.body", "{font} (Body)", "{font}（本文）"),
    ("ribbon.font.bold", "Bold", "太字"),
    ("ribbon.font.case", "Change Case", "大文字と小文字の変更"),
    ("ribbon.font.case_lower", "lowercase", "小文字"),
    ("ribbon.font.case_sentence", "Sentence case.", "文の先頭を大文字にする"),
    ("ribbon.font.case_title", "Capitalize Each Word", "各単語の先頭を大文字にする"),
    ("ribbon.font.case_toggle", "tOGGLE cASE", "大文字と小文字を切り替える"),
    ("ribbon.font.case_upper", "UPPERCASE", "大文字"),
    ("ribbon.font.clear", "Clear All Formatting", "すべての書式をクリア"),
    ("ribbon.font.color", "Font Color", "文字の色"),
    ("ribbon.font.grow", "Increase Font Size", "文字を大きくする"),
    ("ribbon.font.headings", "{font} (Headings)", "{font}（見出し）"),
    ("ribbon.font.highlight", "Text Highlight Color", "文字の強調色"),
    ("ribbon.font.italic", "Italic", "斜体"),
    ("ribbon.font.no_color", "No Color", "色なし"),
    ("ribbon.font.shrink", "Decrease Font Size", "文字を小さくする"),
    ("ribbon.font.spacing", "Character Spacing", "文字間隔"),
    ("ribbon.font.spacing_loose", "Loose", "広く"),
    ("ribbon.font.spacing_normal", "Normal", "標準"),
    ("ribbon.font.spacing_tight", "Tight", "狭く"),
    ("ribbon.font.spacing_very_loose", "Very Loose", "非常に広く"),
    ("ribbon.font.spacing_very_tight", "Very Tight", "非常に狭く"),
    ("ribbon.font.strike", "Strikethrough", "取り消し線"),
    ("ribbon.font.subscript", "Subscript", "下付き"),
    ("ribbon.font.superscript", "Superscript", "上付き"),
    ("ribbon.font.underline", "Underline", "下線"),
    ("ribbon.home.design_ideas", "Design\nIdeas", "デザインの\n候補"),
    ("ribbon.paragraph.align", "Align Text", "テキストの配置"),
    ("ribbon.paragraph.bullet", "{symbol}  Bullet", "{symbol}  箇条書き"),
    ("ribbon.paragraph.bullets", "Bullets", "箇条書き"),
    ("ribbon.paragraph.center", "Center", "中央揃え"),
    ("ribbon.paragraph.columns", "Columns", "段組み"),
    ("ribbon.paragraph.columns_many", "{n} Columns", "{n} 段"),
    ("ribbon.paragraph.columns_one", "{n} Column", "{n} 段"),
    ("ribbon.paragraph.direction", "Text Direction", "文字列の方向"),
    ("ribbon.paragraph.horizontal", "Horizontal", "横書き"),
    ("ribbon.paragraph.indent", "Increase List Level", "リストのレベルを下げる"),
    ("ribbon.paragraph.justify", "Justify", "両端揃え"),
    ("ribbon.paragraph.line_spacing", "Line Spacing", "行間"),
    ("ribbon.paragraph.line_spacing_options", "Line Spacing Options…", "行間の設定…"),
    ("ribbon.paragraph.numbering", "Numbering", "段落番号"),
    ("ribbon.paragraph.outdent", "Decrease List Level", "リストのレベルを上げる"),
    ("ribbon.paragraph.rotate_270", "Rotate all text 270°", "文字列全体を270度回転"),
    ("ribbon.paragraph.rotate_90", "Rotate all text 90°", "文字列全体を90度回転"),
    ("ribbon.paragraph.stacked", "Stacked", "縦に並べる"),
    ("ribbon.slides.add_section", "Add Section", "セクションの追加"),
    ("ribbon.slides.duplicate", "Duplicate Selected Slides", "選択したスライドを複製"),
    ("ribbon.slides.layout", "Layout", "レイアウト"),
    ("ribbon.slides.new", "New\nSlide", "新しい\nスライド"),
    ("ribbon.slides.new_tip", "New Slide (⇧⌘N)", "新しいスライド（⇧⌘N）"),
    ("ribbon.slides.outline", "Slides from Outline…", "アウトラインからスライドを作成…"),
    ("ribbon.slides.remove_sections", "Remove All Sections", "すべてのセクションを削除"),
    ("ribbon.slides.reset", "Reset", "リセット"),
    ("ribbon.slides.section", "Section", "セクション"),
    ("slide_size.a3", "A3 Paper (297x420 mm)", "A3用紙（297x420 mm）"),
    ("slide_size.a4", "A4 Paper (210x297 mm)", "A4用紙（210x297 mm）"),
    ("slide_size.b4", "B4 (ISO) Paper (250x353 mm)", "B4（ISO）用紙（250x353 mm）"),
    ("slide_size.b5", "B5 (ISO) Paper (176x250 mm)", "B5（ISO）用紙（176x250 mm）"),
    ("slide_size.banner", "Banner", "横断幕"),
    ("slide_size.ledger", "Ledger Paper (11x17 in)", "レジャー用紙（11x17 in）"),
    ("slide_size.letter", "Letter Paper (8.5x11 in)", "レター用紙（8.5x11 in）"),
    ("slide_size.overhead", "Overhead", "OHP用"),
    ("slide_size.portrait", "Portrait (9:16)", "縦長（9:16）"),
    ("slide_size.screen_16_10", "On-screen Show (16:10)", "画面表示（16:10）"),
    ("slide_size.slides_35mm", "35mm Slides", "35mmスライド"),
    ("slide_size.square", "Square (1:1)", "正方形（1:1）"),
    ("slide_size.standard", "Standard (4:3)", "標準（4:3）"),
    ("slide_size.wide", "Widescreen (16:9)", "ワイド画面（16:9）"),
    ("slide_size.wide_inches", "Widescreen (13.333x7.5 in)", "ワイド画面（13.333x7.5 in）"),
    ("status.comments", "Comments", "コメント"),
    ("status.fit", "Fit slide to current window", "スライドをウィンドウに合わせる"),
    ("status.normal", "Normal", "標準"),
    ("status.notes", "Notes", "ノート"),
    ("status.reading", "Reading View", "閲覧表示"),
    ("status.slide_master", "Slide Master", "スライドマスター"),
    ("status.slide_of", "Slide {n} of {m}", "スライド {n}/{m}"),
    ("status.slideshow", "Slide Show", "スライドショー"),
    ("status.sorter", "Slide Sorter", "スライド一覧"),
    ("status.zoom_in", "Zoom In", "拡大"),
    ("status.zoom_out", "Zoom Out", "縮小"),
    ("tab.animations", "Animations", "アニメーション"),
    ("tab.chart_design", "Chart Design", "グラフのデザイン"),
    ("tab.design", "Design", "デザイン"),
    ("tab.draw", "Draw", "描画"),
    ("tab.file", "File", "ファイル"),
    ("tab.home", "Home", "ホーム"),
    ("tab.insert", "Insert", "挿入"),
    ("tab.layout", "Layout", "レイアウト"),
    ("tab.picture_format", "Picture Format", "画像の書式"),
    ("tab.playback", "Playback", "再生"),
    ("tab.record", "Record", "記録"),
    ("tab.review", "Review", "校閲"),
    ("tab.shape_format", "Shape Format", "図形の書式"),
    ("tab.slide_master", "Slide Master", "スライドマスター"),
    ("tab.slideshow", "Slide Show", "スライドショー"),
    ("tab.table_design", "Table Design", "テーブルのデザイン"),
    ("tab.transitions", "Transitions", "切り替え"),
    ("tab.view", "View", "表示"),
    ("title.appearance", "Light/Dark appearance", "ライト/ダーク表示"),
    ("title.comments", "Comments", "コメント"),
    ("title.edited", "Edited", "編集済み"),
    ("title.home", "Home", "ホーム"),
    ("title.play", "Play from Start", "最初から再生"),
    ("title.present", "Present", "発表"),
    ("title.redo", "Redo", "やり直し"),
    ("title.save", "Save", "保存"),
    ("title.search", "Search commands", "コマンドを検索"),
    ("title.undo", "Undo", "元に戻す"),
];

/// The shell string `id` in `lang`. An unknown id is returned unchanged so a missed call site
/// shows up as the id rather than a blank label.
pub fn text(lang: Resolved, id: &'static str) -> &'static str {
    match CATALOG.binary_search_by_key(&id, |row| row.0) {
        Ok(i) => CATALOG
            .get(i)
            .map(|row| match lang {
                Resolved::En => row.1,
                Resolved::Ja => row.2,
            })
            .unwrap_or(id),
        Err(_) => id,
    }
}

/// `"{name} ({shortcut})"`, for tooltips that keep the key chord in Latin.
pub fn with_shortcut(lang: Resolved, id: &'static str, shortcut: &str) -> String {
    format!("{} ({shortcut})", text(lang, id))
}

/// Replace `{name}` placeholders. A name that is not in `vars` is left as written.
pub fn fill(template: &str, vars: &[(&str, &str)]) -> String {
    let mut out = String::new();
    let mut rest = template;
    while let Some(start) = rest.find('{') {
        out.push_str(&rest[..start]);
        let after = rest.get(start + 1..).unwrap_or("");
        if let Some(end) = after.find('}') {
            let name = after.get(..end).unwrap_or("");
            match vars.iter().find(|(key, _)| *key == name) {
                Some((_, value)) => out.push_str(value),
                None => {
                    out.push('{');
                    out.push_str(name);
                    out.push('}');
                }
            }
            rest = after.get(end + 1..).unwrap_or("");
        } else {
            out.push_str(rest.get(start..).unwrap_or(""));
            return out;
        }
    }
    out.push_str(rest);
    out
}

/// Ribbon and contextual tab titles. `english_name` is the id stored in [`crate::UiState::tab`].
pub fn tab_label(lang: Resolved, english_name: &'static str) -> &'static str {
    let id = match english_name {
        "File" => "tab.file",
        "Home" => "tab.home",
        "Insert" => "tab.insert",
        "Draw" => "tab.draw",
        "Design" => "tab.design",
        "Transitions" => "tab.transitions",
        "Animations" => "tab.animations",
        "Slide Show" => "tab.slideshow",
        "Record" => "tab.record",
        "Review" => "tab.review",
        "View" => "tab.view",
        "Slide Master" => "tab.slide_master",
        "Shape Format" => "tab.shape_format",
        "Picture Format" => "tab.picture_format",
        "Table Design" => "tab.table_design",
        "Layout" => "tab.layout",
        "Chart Design" => "tab.chart_design",
        "Playback" => "tab.playback",
        _ => return english_name,
    };
    text(lang, id)
}

/// A slide-size preset's display label; the original model name remains its identity.
pub fn slide_size_label(lang: Resolved, name: &str) -> &str {
    let id = match name {
        "A3 Paper (297x420 mm)" => "slide_size.a3",
        "A4 Paper (210x297 mm)" => "slide_size.a4",
        "B4 (ISO) Paper (250x353 mm)" => "slide_size.b4",
        "B5 (ISO) Paper (176x250 mm)" => "slide_size.b5",
        "Banner" => "slide_size.banner",
        "Ledger Paper (11x17 in)" => "slide_size.ledger",
        "Letter Paper (8.5x11 in)" => "slide_size.letter",
        "Overhead" => "slide_size.overhead",
        "Portrait (9:16)" => "slide_size.portrait",
        "On-screen Show (16:10)" => "slide_size.screen_16_10",
        "35mm Slides" => "slide_size.slides_35mm",
        "Square (1:1)" => "slide_size.square",
        "Standard (4:3)" => "slide_size.standard",
        "Widescreen (16:9)" => "slide_size.wide",
        "Widescreen (13.333x7.5 in)" => "slide_size.wide_inches",
        _ => return name,
    };
    text(lang, id)
}

/// Return the localized caption for a stable language-preference choice.
pub fn choice_label(lang: Resolved, choice: LanguageChoice) -> &'static str {
    let id = match choice {
        LanguageChoice::System => "pref.system",
        LanguageChoice::En => "pref.english",
        LanguageChoice::Ja => "pref.ja",
    };
    text(lang, id)
}

/// Font-dependent UI tests use the same optional input as native builds; required builds cannot skip.
#[cfg(test)]
pub(crate) fn japanese_font_input_available() -> bool {
    if deckcraft_fonts::CRAFT_FONTS.iter().any(|f| f.family == "BIZ UDPGothic" && f.style == "Regular") {
        return true;
    }
    assert!(option_env!("CRAFT_FONTS_REQUIRED").is_none(), "required Japanese interface face is absent");
    eprintln!("optional Japanese font input absent: translated UI glyph coverage was not qualified");
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Collect placeholder names in order so translated messages can be compared.
    fn holes(s: &str) -> Vec<&str> {
        let mut out = Vec::new();
        let mut rest = s;
        while let Some(start) = rest.find('{') {
            rest = rest.get(start + 1..).unwrap_or("");
            let Some(end) = rest.find('}') else { break };
            out.push(rest.get(..end).unwrap_or(""));
            rest = rest.get(end + 1..).unwrap_or("");
        }
        out
    }

    #[test]
    /// Require unique sorted keys, nonempty translations and matching message placeholders.
    fn the_shell_catalog_is_sorted_and_complete() {
        let mut previous = "";
        for (id, en, ja) in CATALOG {
            assert!(*id > previous, "{id} is out of order after {previous}");
            previous = id;
            assert!(!en.is_empty() && !ja.is_empty(), "{id}");
            assert_eq!(holes(en), holes(ja), "{id}");
            let same = ["pref.english", "pref.ja"];
            if en == ja {
                assert!(same.contains(id), "{id} has no Japanese text");
            }
        }
        assert!(CATALOG.len() >= 60, "the shell catalog shrank");
    }

    #[test]
    /// Verify Japanese locale variants and English fallback for unsupported locales.
    fn japanese_tags_resolve_and_other_tags_do_not() {
        for tag in ["ja", "ja-JP", "ja_JP.UTF-8", "ja-JP-mac", "JA-jp"] {
            assert_eq!(resolve_tag(tag), Resolved::Ja, "{tag}");
        }
        for tag in ["en-BY", "en-US", "zh-CN", "zh-TW", "ru-BY", "C", ""] {
            assert_eq!(resolve_tag(tag), Resolved::En, "{tag}");
        }
        assert_eq!(LanguageChoice::System.resolve(Some("ja-JP")), Resolved::Ja);
        assert_eq!(LanguageChoice::System.resolve(Some("zh-CN")), Resolved::En);
        assert_eq!(LanguageChoice::System.resolve(None), Resolved::En);
        assert_eq!(LanguageChoice::En.resolve(Some("ja-JP")), Resolved::En);
        assert_eq!(LanguageChoice::Ja.resolve(None), Resolved::Ja);
    }

    #[test]
    /// Accept System, English and Japanese codes while rejecting unsupported choices.
    fn the_language_command_accepts_the_three_choices() {
        assert_eq!(LanguageChoice::parse("system"), Some(LanguageChoice::System));
        assert_eq!(LanguageChoice::parse("en"), Some(LanguageChoice::En));
        assert_eq!(LanguageChoice::parse("en-GB"), Some(LanguageChoice::En));
        assert_eq!(LanguageChoice::parse("ja"), Some(LanguageChoice::Ja));
        assert_eq!(LanguageChoice::parse("ja-JP"), Some(LanguageChoice::Ja));
        assert_eq!(LanguageChoice::parse("zh-Hans"), None);
        assert_eq!(LanguageChoice::parse("fr"), None);
    }

    #[test]
    /// Preserve saved Japanese choices and use System for unrecognized saved codes.
    fn a_saved_choice_round_trips_and_an_unknown_code_falls_back() {
        let raw = serde_json::to_string(&LanguageChoice::Ja).expect("serialize");
        assert_eq!(raw, "\"ja\"");
        let back: LanguageChoice = serde_json::from_str(&raw).expect("deserialize");
        assert_eq!(back, LanguageChoice::Ja);
        let unknown: LanguageChoice = serde_json::from_str("\"zh-Hans\"").expect("deserialize");
        assert_eq!(unknown, LanguageChoice::System);
    }

    #[test]
    /// Fill named message arguments without losing unresolved placeholders.
    fn placeholders_keep_their_names() {
        let ja = text(Resolved::Ja, "status.slide_of");
        assert_eq!(fill(ja, &[("n", "2"), ("m", "9")]), "スライド 2/9");
        let en = text(Resolved::En, "status.slide_of");
        assert_eq!(fill(en, &[("n", "2"), ("m", "9")]), "Slide 2 of 9");
        assert_eq!(fill("Slide {n} of {m}", &[("n", "1")]), "Slide 1 of {m}");
    }

    #[test]
    /// Translate tab captions while retaining English identifiers and unknown-tab fallback.
    fn tab_titles_follow_the_language_and_keep_the_english_id() {
        assert_eq!(tab_label(Resolved::Ja, "File"), "ファイル");
        assert_eq!(tab_label(Resolved::Ja, "Home"), "ホーム");
        assert_eq!(tab_label(Resolved::En, "Slide Show"), "Slide Show");
        assert_eq!(tab_label(Resolved::Ja, "Not a tab"), "Not a tab");
        assert_eq!(with_shortcut(Resolved::Ja, "tab.file", "Alt+F"), "ファイル (Alt+F)");
    }

    /// Translated controls keep using the field names accepted by the table command.
    #[test]
    fn japanese_table_dialog_inserts_the_requested_table() {
        if !japanese_font_input_available() {
            return;
        }
        use egui_kittest::kittest::Queryable;
        let mut app = crate::SlideApp::new(deckcraft_engine::Session::with_new(), crate::Services::default());
        app.run("app.language", serde_json::json!({"language": "ja"})).unwrap();
        let mut dialog = crate::dialogs::Dialog::new("table");
        dialog.fields.insert("cols".into(), "2".into());
        dialog.fields.insert("rows".into(), "3".into());
        app.dialog = Some(dialog);
        let mut h =
            egui_kittest::Harness::builder().with_size(egui::vec2(900.0, 600.0)).with_context_setup(crate::theme::install_fonts).build_ui_state(
                |ui, app: &mut crate::SlideApp| {
                    app.logic(ui.ctx());
                    crate::dialogs::show(app, ui.ctx());
                },
                app,
            );
        h.run_steps(3);
        assert!(h.query_by_label("列数:").is_some());
        assert!(h.query_by_label("行数:").is_some());
        assert!(h.query_by_label("キャンセル").is_some());
        crate::i18n_integration_tests::preview(&mut h, "ja-table");
        h.get_by_label("挿入").click();
        h.run_steps(2);
        assert!(h.state().dialog.is_none());
        let table = h
            .state()
            .session
            .active()
            .unwrap()
            .shapes()
            .iter()
            .find_map(|shape| if let deckcraft_model::ShapeKind::Table(table) = &shape.kind { Some(table) } else { None })
            .expect("the Insert button runs the table command");
        assert_eq!(table.cols.len(), 2);
        assert_eq!(table.rows.len(), 3);
    }

    /// A translated action operates on the same shape; language switches preserve user data.
    #[test]
    fn japanese_selection_actions_keep_shape_names_and_ids() {
        if !japanese_font_input_available() {
            return;
        }
        use egui_kittest::kittest::Queryable;
        let mut session = deckcraft_engine::Session::with_new();
        session.execute("slide.new", &serde_json::json!({"layout": "blank"})).unwrap();
        let id = session.execute("shape.insert", &serde_json::json!({"preset": "rect", "rect": [20, 20, 100, 80]})).unwrap()["id"].as_u64().unwrap();
        let name = "File {customer}";
        session.execute("shape.rename", &serde_json::json!({"id": id, "name": name})).unwrap();
        let mut app = crate::SlideApp::new(session, crate::Services::default());
        app.run("app.language", serde_json::json!({"language": "ja"})).unwrap();
        let mut h =
            egui_kittest::Harness::builder().with_size(egui::vec2(440.0, 700.0)).with_context_setup(crate::theme::install_fonts).build_ui_state(
                |ui, app: &mut crate::SlideApp| {
                    app.logic(ui.ctx());
                    crate::selpane::show(app, ui);
                },
                app,
            );
        h.run_steps(3);
        crate::i18n_integration_tests::preview(&mut h, "ja-selection");
        h.get_by_label("File {customer}をロック").click();
        h.run_steps(2);
        let shape = h.state().session.active().unwrap().shapes().iter().find(|s| u64::from(s.id.0) == id).unwrap();
        assert_eq!(shape.name, name);
        assert!(shape.locked);
        assert!(h.query_by_label("File {customer}のロックを解除").is_some());
        let before = serde_json::to_value(&h.state().session.active().unwrap().doc).unwrap();
        h.state_mut().run("app.language", serde_json::json!({"language": "en"})).unwrap();
        h.run_steps(2);
        assert!(h.query_by_label("Unlock File {customer}").is_some());
        assert_eq!(before, serde_json::to_value(&h.state().session.active().unwrap().doc).unwrap());
        assert_eq!(h.state().ui.language, LanguageChoice::En);
    }

    #[test]
    /// Keep preset identities and preserve braces inside user-provided message arguments.
    fn preset_labels_keep_model_identity_and_user_placeholders_are_literal() {
        for &(name, _, _) in deckcraft_model::defaults::SLIDE_SIZES {
            assert_eq!(slide_size_label(Resolved::En, name), name);
            assert_ne!(slide_size_label(Resolved::Ja, name), name, "{name}");
        }
        assert_eq!(slide_size_label(Resolved::Ja, "My {custom} size"), "My {custom} size");
        assert_eq!(
            fill(text(Resolved::Ja, "dialog.save.close_question"), &[("name", "File {customer}.pptx")]),
            "「File {customer}.pptx」への変更を保存しますか？"
        );
        assert_eq!(fill(text(Resolved::En, "dialog.custom_show.summary"), &[("name", "My {show}"), ("n", "2")]), "My {show} (2 slides)");
    }
}
