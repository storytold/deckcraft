//! File > Print > a printer of this computer: the system spooler (CUPS, or Windows) through
//! craft-print. DeckCraft builds the print-ready PDF; craft-print lists the queues and hands the
//! PDF to the spooler. The listing and the job run on their own threads: the UI thread only starts
//! them and drains their channels.
//!
//! The printer list is read when the person first opens the destination list (not at start-up),
//! and again the first time it is opened after the Print page is shown anew.

use std::sync::mpsc::{self, Receiver, TryRecvError};

pub use craft_print::{Duplex, Scaling};

use crate::SlideApp;

/// The queues of this computer, in the spooler's order. Reads only.
pub type ListFn = fn() -> Vec<String>;
/// Hand the PDF to the spooler; progress is `(sheets sent, sheets)`. The answer is the spooler's message.
pub type SubmitFn = fn(&[u8], &craft_print::Job, &mut dyn FnMut(usize, usize)) -> Result<String, String>;

fn list_queues() -> Vec<String> {
    craft_print::printers().into_iter().map(|p| p.name).collect()
}

fn submit_to_spooler(pdf: &[u8], job: &craft_print::Job, progress: &mut dyn FnMut(usize, usize)) -> Result<String, String> {
    craft_print::submit_with_progress(pdf, job, progress).map_err(|e| e.to_string())
}

enum Msg {
    Progress(usize, usize),
    Done(Result<String, String>),
}

/// The settings of a job to a local printer.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Settings {
    pub copies: u32,
    pub duplex: Option<Duplex>,
    pub grayscale: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Self { copies: 1, duplex: None, grayscale: false }
    }
}

/// The local printers, the settings and the job in flight.
pub struct Local {
    /// `None` until the list has been asked for.
    pub printers: Option<Vec<String>>,
    pub settings: Settings,
    /// `(sheets sent, sheets)` while a job runs, once the spooler has reported any.
    pub progress: Option<(usize, usize)>,
    list: ListFn,
    submit: SubmitFn,
    list_rx: Option<Receiver<Vec<String>>>,
    job_rx: Option<Receiver<Msg>>,
    /// The printer the running (or last) job went to, for the words that follow it.
    printer: String,
    ctx: Option<egui::Context>,
}

impl Default for Local {
    fn default() -> Self {
        Self::with(list_queues, submit_to_spooler)
    }
}

impl Local {
    /// Local printing with the listing and the submission supplied (tests give fakes).
    pub fn with(list: ListFn, submit: SubmitFn) -> Self {
        Self {
            printers: None,
            settings: Settings::default(),
            progress: None,
            list,
            submit,
            list_rx: None,
            job_rx: None,
            printer: String::new(),
            ctx: None,
        }
    }

    pub fn loading(&self) -> bool {
        self.list_rx.is_some()
    }

    pub fn running(&self) -> bool {
        self.job_rx.is_some()
    }

    pub(crate) fn set_ctx(&mut self, ctx: &egui::Context) {
        self.ctx = Some(ctx.clone());
    }

    /// Read the printers off the UI thread, unless they are known or being read.
    pub fn load(&mut self) {
        if self.printers.is_some() || self.list_rx.is_some() {
            return;
        }
        let (tx, rx) = mpsc::channel();
        let (list, ctx) = (self.list, self.ctx.clone());
        std::thread::spawn(move || {
            let _ = tx.send(list());
            if let Some(ctx) = ctx {
                ctx.request_repaint();
            }
        });
        self.list_rx = Some(rx);
    }

    /// Forget the list so the next [`Self::load`] reads it again.
    pub fn refresh(&mut self) {
        if self.list_rx.is_none() {
            self.printers = None;
        }
    }

    /// Send `pdf` to `printer`, off the UI thread. `false` when a job is already running.
    /// `scaling` is [`Scaling::Exact`] when the person chose the paper (the PDF is on it, and a
    /// queue that lacks it is refused), [`Scaling::ShrinkToFit`] when the pages are slide-shaped:
    /// no paper was asked for, so the job goes on the queue's own paper and the slide is fitted.
    /// `media` is the chosen paper as the spooler names it, sent as the job's `media` option (CUPS
    /// takes it from there; `None` sends no paper).
    pub fn send(&mut self, printer: &str, title: &str, pdf: Vec<u8>, scaling: Scaling, media: Option<&str>) -> bool {
        if self.job_rx.is_some() {
            return false;
        }
        let job = craft_print::Job {
            printer: Some(printer.to_owned()),
            copies: self.settings.copies.clamp(1, 999),
            duplex: self.settings.duplex,
            grayscale: self.settings.grayscale,
            scaling,
            title: title.chars().filter(|c| !c.is_control()).take(100).collect(),
            options: media.map(|m| vec![("media".to_owned(), m.to_owned())]).unwrap_or_default(),
            ..craft_print::Job::default()
        };
        let (tx, rx) = mpsc::channel();
        let (submit, ctx) = (self.submit, self.ctx.clone());
        std::thread::spawn(move || {
            let progress_tx = tx.clone();
            let result = submit(&pdf, &job, &mut |sent, sheets| {
                let _ = progress_tx.send(Msg::Progress(sent, sheets));
                if let Some(ctx) = &ctx {
                    ctx.request_repaint();
                }
            });
            let _ = tx.send(Msg::Done(result));
            if let Some(ctx) = &ctx {
                ctx.request_repaint();
            }
        });
        self.job_rx = Some(rx);
        self.printer = printer.to_owned();
        self.progress = None;
        true
    }

    /// Collect finished work. The words for a finished job name the printer.
    pub fn poll(&mut self) -> Option<String> {
        if let Some(rx) = &self.list_rx {
            match rx.try_recv() {
                Ok(list) => {
                    self.printers = Some(list);
                    self.list_rx = None;
                }
                Err(TryRecvError::Disconnected) => {
                    self.printers = Some(Vec::new());
                    self.list_rx = None;
                }
                Err(TryRecvError::Empty) => {}
            }
        }
        let rx = self.job_rx.as_ref()?;
        let mut said = None;
        loop {
            match rx.try_recv() {
                Ok(Msg::Progress(sent, sheets)) => self.progress = Some((sent, sheets)),
                Ok(Msg::Done(result)) => {
                    said = Some(match result {
                        Ok(message) if message.trim().is_empty() => format!("Sent to {}", self.printer),
                        Ok(message) => format!("Sent to {}: {message}", self.printer),
                        Err(e) => format!("Couldn't print to {}: {e}", self.printer),
                    });
                    break;
                }
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => {
                    said = Some(format!("The print job to {} stopped unexpectedly", self.printer));
                    break;
                }
            }
        }
        if said.is_some() {
            self.job_rx = None;
            self.progress = None;
        }
        said
    }
}

impl SlideApp {
    /// Collect finished local-printer work (each frame): the person hears how a job went, naming the printer.
    pub(crate) fn poll_local_print(&mut self) {
        if let Some(message) = self.local_print.poll() {
            self.notify(message);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fake_list() -> Vec<String> {
        vec!["Office Laser".into(), "Front Desk".into()]
    }

    fn fake_submit(pdf: &[u8], job: &craft_print::Job, progress: &mut dyn FnMut(usize, usize)) -> Result<String, String> {
        assert!(pdf.starts_with(b"%PDF"));
        progress(0, 2);
        progress(2, 2);
        match job.printer.as_deref() {
            Some("Office Laser") => Ok(format!("spooled 2 sheet(s), {} copies", job.copies)),
            Some(other) => Err(format!("no such queue {other}")),
            None => Err("no printer".into()),
        }
    }

    fn wait(local: &mut Local) -> String {
        for _ in 0..400 {
            if let Some(m) = local.poll() {
                return m;
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        panic!("timed out");
    }

    #[test]
    fn the_printer_list_is_read_once_off_the_ui_thread() {
        let mut local = Local::with(fake_list, fake_submit);
        assert!(local.printers.is_none());
        local.load();
        assert!(local.loading());
        for _ in 0..400 {
            local.poll();
            if local.printers.is_some() {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        assert_eq!(local.printers.as_deref(), Some(&["Office Laser".to_owned(), "Front Desk".to_owned()][..]));
        assert!(!local.loading());
    }

    #[test]
    fn a_job_names_the_printer_when_it_works_and_when_it_does_not() {
        let mut local = Local::with(fake_list, fake_submit);
        local.settings.copies = 3;
        assert!(local.send("Office Laser", "Quarterly\nreview", b"%PDF-1.7 fake".to_vec(), Scaling::Exact, None));
        assert!(local.running());
        assert!(!local.send("Front Desk", "x", b"%PDF".to_vec(), Scaling::Exact, None), "one job at a time");
        assert_eq!(wait(&mut local), "Sent to Office Laser: spooled 2 sheet(s), 3 copies");
        assert!(!local.running());
        assert!(local.send("Front Desk", "x", b"%PDF".to_vec(), Scaling::Exact, None));
        assert_eq!(wait(&mut local), "Couldn't print to Front Desk: no such queue Front Desk");
    }

    /// Says what scaling and driver options the job asked for, so a test can tell what reached the spooler.
    fn scaling_submit(_: &[u8], job: &craft_print::Job, _: &mut dyn FnMut(usize, usize)) -> Result<String, String> {
        Ok(format!("{:?} {:?}", job.scaling, job.options))
    }

    #[test]
    fn a_slide_sized_job_is_fitted_to_the_queues_own_paper() {
        let mut local = Local::with(fake_list, scaling_submit);
        assert!(local.send("Office Laser", "Deck", b"%PDF".to_vec(), Scaling::ShrinkToFit, None));
        assert_eq!(wait(&mut local), "Sent to Office Laser: ShrinkToFit []", "no paper or other driver option is sent");
    }

    #[test]
    fn a_chosen_paper_is_still_demanded_exactly() {
        let mut local = Local::with(fake_list, scaling_submit);
        assert!(local.send("Office Laser", "Notes", b"%PDF".to_vec(), Scaling::Exact, Some("A4")));
        assert_eq!(wait(&mut local), r#"Sent to Office Laser: Exact [("media", "A4")]"#, "the paper is named, so CUPS doesn't use its default");
    }
}
