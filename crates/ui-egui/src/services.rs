pub type ReadFn = Box<dyn Fn(&str) -> Result<Vec<u8>, String>>;

pub type WriteFn = Box<dyn FnMut(&str, &[u8]) -> Result<(), String>>;

pub type PickFn = Box<dyn FnMut(&str) -> Option<String>>;

/// `(purpose, request id)`. The host shows a picker and later pushes an [`InboxEvent`]
/// carrying that same id. Completing a cancelled or unknown id does nothing: the file is
/// not applied to whichever document happens to be active.
pub type OpenAsyncFn = Box<dyn FnMut(&str, u64)>;

pub type DownloadFn = Box<dyn FnMut(&str, &[u8])>;

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
    /// The screens the computer shows to, read when a slide show starts. Without it (the web,
    /// tests) the show stays where the window is.
    pub screens: Option<Box<dyn FnMut() -> Option<Screens>>>,
}

/// The screens connected, numbered as the windowing system lists them (the numbers a window is
/// sent to with `ViewportCommand::SetMonitor`). Wayland can list a screen more than once: `list`
/// holds one number per screen.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Screens {
    pub list: Vec<usize>,
    /// The screen DeckCraft's window is on, when known.
    pub current: Option<usize>,
    /// The system's main screen, when known (Wayland doesn't say).
    pub primary: Option<usize>,
}

impl Screens {
    /// Where a slide show with Presenter View goes, as in PowerPoint: the slides full screen on
    /// another screen and Presenter View on the presenter's (the one DeckCraft is on). `None`
    /// with one screen: there, PowerPoint shows only the slides. Returns (slides, presenter).
    pub fn for_presenter_view(&self) -> Option<(usize, usize)> {
        self.for_presenter_view_on(None)
    }

    /// As [`for_presenter_view`](Self::for_presenter_view), with the slides on `chosen` when that
    /// screen is connected. Presenter View then goes to the screen DeckCraft is on, or when that
    /// is `chosen` too, to the main screen or the first other one.
    pub fn for_presenter_view_on(&self, chosen: Option<usize>) -> Option<(usize, usize)> {
        if self.list.len() < 2 {
            return None;
        }
        let listed = |i: &usize| self.list.contains(i);
        let Some(slides) = chosen.filter(listed) else {
            let presenter = self.current.filter(listed).or(self.primary.filter(listed)).or_else(|| self.list.first().copied())?;
            let slides = self.list.iter().copied().find(|i| *i != presenter)?;
            return Some((slides, presenter));
        };
        let presenter = [self.current, self.primary].into_iter().flatten().find(|i| listed(i) && *i != slides);
        let presenter = presenter.or_else(|| self.list.iter().copied().find(|i| *i != slides))?;
        Some((slides, presenter))
    }
}
