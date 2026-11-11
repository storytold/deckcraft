pub type ReadFn = Box<dyn Fn(&str) -> Result<Vec<u8>, String>>;

pub type WriteFn = Box<dyn FnMut(&str, &[u8]) -> Result<(), String>>;

pub type PickFn = Box<dyn FnMut(&str) -> Option<String>>;

/// `(purpose, request id)`. The host shows a picker and later pushes an [`InboxEvent`]
/// carrying that same id. Completing a cancelled or unknown id does nothing: the file is
/// not applied to whichever document happens to be active.
pub type OpenAsyncFn = Box<dyn FnMut(&str, u64)>;

pub type DownloadFn = Box<dyn FnMut(&str, &[u8])>;

/// Download a model's files into a directory, reporting `(done, total)` bytes. The desktop app
/// supplies an HTTPS implementation (ureq); the browser has none and the dialog says so.
pub type ModelDownloadFn =
    std::sync::Arc<dyn Fn(&deckcraft_asr::ModelManifest, &std::path::Path, &mut dyn FnMut(u64, u64)) -> Result<(), String> + Send + Sync>;

/// [`Services::host_save`]: `(file name, bytes, save_as)`. An error means the page didn't get
/// the file, and the save failed.
pub type HostSaveFn = Box<dyn FnMut(&str, &[u8], bool) -> Result<(), String>>;

/// A file the host finished reading for the request `id` from [`SlideApp::begin_external_file`].
#[derive(Clone, Debug)]
pub struct InboxFile {
    /// Cancellation identity. A file whose id was cancelled, or never started, is ignored.
    pub id: u64,
    pub name: String,
    pub bytes: Vec<u8>,
}

/// What the host delivers through [`Inbox`] after an asynchronous read.
#[derive(Clone, Debug)]
pub enum InboxEvent {
    File(InboxFile),
    /// The picker was dismissed or the file could not be read. `id` will not be applied.
    Cancelled {
        id: u64,
    },
}

/// Files delivered asynchronously by the host (web file picker, dropped files).
pub type Inbox = std::sync::Arc<std::sync::Mutex<Vec<InboxEvent>>>;

/// Platform services injected by the host (desktop or web).
#[derive(Default)]
pub struct Services {
    /// Browser-owned presenter popup; absent on native hosts.
    pub presenter: Option<Box<dyn crate::presenter::Host>>,
    /// Open-file dialog for a purpose (`open`, `picture`, `audio`, `video`) → path.
    pub pick_open: Option<PickFn>,
    /// Save dialog with a suggested name → path.
    pub pick_save: Option<PickFn>,
    pub read: Option<ReadFn>,
    pub write: Option<WriteFn>,
    /// Asynchronous open dialog (web). The callback receives the purpose and the request id
    /// from [`SlideApp::begin_external_file`]; the chosen file arrives through `inbox`.
    pub open_async: Option<OpenAsyncFn>,
    /// Hand bytes to the user as a named file (browser download).
    pub download: Option<DownloadFn>,
    /// Web, framed by a page that keeps the files (`?host=parent`): Save and Save As hand the
    /// presentation to that page instead of downloading it, with whether it is a new file
    /// ([`HostSaveFn`]). Save writes a presentation the page sent back under its own name and
    /// format, as the desktop app writes back to the file it opened; a presentation the page
    /// never sent (an untitled one, a repaired copy) goes as a new `.deckcraft` file, as Save
    /// suggests on the desktop. Save As (.pptx or .deckcraft) is always a new file. Either way the
    /// presentation is then that file, and the next Save goes to it. Exports and Print still go
    /// through `download`, which the host also points at the page: they are copies.
    pub host_save: Option<HostSaveFn>,
    pub inbox: Option<Inbox>,
    /// Image on the system clipboard (PNG bytes), when the host can read one.
    pub clipboard_image: Option<Box<dyn FnMut() -> Option<Vec<u8>>>>,
    /// HTML on the system clipboard (Pages, Keynote, and other apps that write `public.html`).
    pub clipboard_html: Option<Box<dyn FnMut() -> Option<String>>>,
    /// Put HTML and its plain-text alternative on the system clipboard. `true` when it landed.
    pub clipboard_set_html: Option<Box<dyn FnMut(&str, &str) -> bool>>,
    /// Audio output for media playback (cpal on desktop); without one media plays silently.
    pub audio_out: Option<Box<dyn deckcraft_media::AudioOut>>,
    /// How many displays are connected, when the host can tell: Presenter View needs a second.
    pub displays: Option<Box<dyn FnMut() -> Option<usize>>>,
    /// Model download for live subtitles (native hosts only; see [`ModelDownloadFn`]).
    pub download_model: Option<ModelDownloadFn>,
}
