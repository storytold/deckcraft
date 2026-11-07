# 繁體中文介面（台灣）／Traditional Chinese shell (Taiwan)

> **狀態：** 僅應用程式外殼，共 87 個字串。尚未經母語審閱。
> **Status:** application shell only, 87 strings. Native review is pending.

## 範圍／Scope

這份字串目錄只涵蓋應用程式外殼：

- 功能區索引標籤（含內容相關索引標籤）
- 檔案（Backstage）側邊選單：首頁、新增、開啟、資訊、另存新檔、列印、匯出、關於、儲存、
  關閉、選項、返回
- 標題列：已編輯、放映、搜尋命令、註解、淺色/深色外觀
- 快速存取工具列：按鈕提示、自訂選單、加入／移除與重設
- 狀態列：第 n 張，共 m 張、投影片母片、備忘稿、註解、縮放與檢視標籤
- 偏好設定對話方塊：一般、介面語言、介面大小、量測單位、編輯選項、按鈕與標題

以下刻意保持英文：功能區群組、其他對話方塊、命令識別碼（例如 `app.language`）、產品名稱
（DeckCraft、SmartArt）、簡報內容與檔案名稱。

This catalog covers the application shell only: the ribbon tabs (including contextual tabs), the
File/Backstage side menu, the title bar, the quick access toolbar, the status bar, and the
Preferences dialog. Ribbon groups, the other dialogs, command ids, product names, presentation
text and file names stay in English.

## 選擇語言／Choosing the language

偏好設定 → 介面語言提供三個選項：

| 選項 | 儲存值 | 說明 |
|---|---|---|
| 跟隨系統／System | `system` | 依作業系統地區設定決定；找不到支援的地區時使用英文 |
| English | `en` | 一律英文 |
| 繁體中文 | `zh-Hant` | 一律台灣繁體中文 |

也可以從命令或控制通道設定：

```json
{"command": "app.language", "params": {"language": "zh-Hant"}}
```

`app.language` 接受 `system`、`en`、`zh-Hant`，以及 `zh-TW`、`zh-HK`、`zh-MO` 等地區別名。
`zh-Hans`、`zh-CN`、`zh-SG` 與單獨的 `zh` 會保持英文，因為目前沒有簡體中文目錄。無法辨識的
已儲存值會回到「跟隨系統」。選擇會寫入 `ui.json` 的 `language` 欄位。

Preferences › Interface language offers System, English and 繁體中文. The choice is saved as
`system`, `en` or `zh-Hant` in `ui.json`, and `app.language` accepts the same codes plus the
Traditional Chinese regional aliases `zh-TW`, `zh-HK` and `zh-MO`. Simplified Chinese codes select
English because no Simplified catalog exists; an unknown saved value falls back to System.

## 系統地區設定／System locale

桌面版會讀取作業系統的地區設定。`zh-Hant`、`zh-TW`、`zh-HK`、`zh-MO`（可帶地區或編碼後綴）
會選用繁體中文；其他地區設定一律使用英文。`System` 只影響外殼；功能區群組與其他對話方塊仍為
英文。

On desktop, System reads the operating system locale once. Traditional Chinese tags (`zh-Hant`,
`zh-TW`, `zh-HK`, `zh-MO`, with optional region or encoding suffixes) select the Traditional
Chinese shell; every other locale stays English. System affects the shell only.

## 字型／Fonts

外殼會依序偏好已安裝的下列字型，讓繁體中文使用台灣字形：

1. Microsoft JhengHei（微軟正黑體，Windows）
2. MingLiU／DFKai-SB（Windows）
3. PingFang TC／Heiti TC／Songti TC（macOS）
4. AR PL UMing TW／WenQuanYi Zen Hei（Linux、FreeBSD 常見的開放字型）

這些都是系統字型；DeckCraft 不內嵌、也不在候選清單中指定 Noto CJK／Source Han（資產政策
禁止）。找不到上述字型時，仍會使用既有的逐字型遞補機制，介面不會因此停止運作。

The shell prefers an installed face, in this order: Microsoft JhengHei, MingLiU/DFKai-SB,
PingFang TC, Heiti TC, Songti TC, and the open AR PL UMing TW/WenQuanYi faces on Linux and
FreeBSD. DeckCraft bundles no CJK font and does not name Noto CJK or Source Han in the candidate
list (the asset policy excludes them). If none is installed, the existing per-character fallback
still applies.

## 網頁版限制／Web limitation

瀏覽器版本沒有可讀取的系統字型，也不內嵌 CJK 字型，因此無法保證繁體中文字形。為了避免顯示
方塊，網頁版的「繁體中文」選項以英文顯示並停用；使用者可選 English 或 System，而 System 在
網頁版維持英文。這是沿用現有的網頁字型限制，並未新增字型下載。

The web build has no installed fonts and bundles no CJK face, so it cannot guarantee Traditional
Chinese glyphs. Its Traditional Chinese entry is shown in English and disabled; System stays
English on the web. This follows the repository's existing web font gating instead of adding a
font download.

## 詞彙／Terminology

| English | 台灣繁體中文 |
|---|---|
| File | 檔案 |
| Home | 常用（功能區）／首頁（檔案頁） |
| Insert | 插入 |
| Design | 設計 |
| Transitions | 轉場 |
| Animations | 動畫 |
| Slide Show | 投影片放映 |
| Review | 校閱 |
| View | 檢視 |
| Slide Master | 投影片母片 |
| Notes | 備忘稿 |
| Comments | 註解 |
| Save As | 另存新檔 |
| Options | 選項 |
| Preferences | 偏好設定 |
| Interface language | 介面語言 |

字串以英文原文為來源，目錄位於 `crates/ui-egui/src/i18n.rs`。鍵值採穩定、可搜尋的英文鍵；
英文與繁中皆為人工撰寫，未從其他產品或既有 PR 複製。

The catalog lives in `crates/ui-egui/src/i18n.rs`. Keys are stable English identifiers; English is
the source and the Traditional Chinese rows were written from the English meanings.
