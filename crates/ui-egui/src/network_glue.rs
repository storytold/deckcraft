//! Print > Network printer (WS-Print and IPP): the app's part of `craft_ui::printing::network`. The
//! model, its rules and the widgets are craft-ui's; the printing is craft-print's WS-Print and IPP
//! backends through [`crate::netprint`]. This file supplies what a host supplies: the address
//! check, the worker that asks a printer what it lists, the worker that sends a job and follows
//! it, the mapping of craft-print's answers onto craft-ui's light types, and the wording.
//!
//! Built only with the `network-print` feature on a target with sockets (the `network_print` cfg).
//! All network work runs on its own threads; the UI thread only starts it and drains receivers.
//!
//! The document is the print-ready PDF that File > Print builds (slides, notes pages or handouts).
//! A printer that lists PDF gets it as it is; a printer that lists no PDF is reported as "accepts
//! none of the formats this document can be sent in" (DeckCraft registers no raster converter).
//! The printer the person queried is remembered, so the job does not read it again before the
//! registry decides.
//!
//! A job has two phases and so does its worker: it sends the document and reports **accepted**
//! as soon as the printer has it (`JobUpdate::Accepted`), then follows the job with craft-print's
//! `JobWatch`, reporting each new status (`JobUpdate::Status`) until it ends. A document is sent
//! once: a send whose outcome is unknown is shown with the request to check the printer before
//! sending again (`Failure::outcome_unknown`), and nothing retries it.

use std::sync::{Arc, Mutex, PoisonError, mpsc};
use std::time::Duration;

use crate::netprint::wsd;
use crate::netprint::{
    CancelToken, ColourMode, Document, Duplex, Elements, ErrorKind, Job, JobOptions, JobState, JobStatus, Orientation, Phase, PrintError, Printer,
    Quality, Resolution, Scaling, UserConfirmed, WaitPolicy, WatchOutcome, WatchStep,
};
use craft_ui::capability::Capability;
use craft_ui::printing::network::{
    self as net, Caps, Failure, FailureKind, JobHandle, JobUpdate, Labels, Outcome, PrinterSummary, Progress, QueryPrinter, Report, Request, SendJob,
    Stage,
};

use crate::SlideApp;

/// Any transport that can be shared with the worker threads (tests give an in-memory printer).
pub type Transport = crate::netprint::Transport;

/// The media types the Print page can produce: the print-ready PDF.
pub(crate) const FORMATS: &[&str] = &["application/pdf"];

/// What the last query of each address read, so a job to that printer does not read it again.
type Queried = Arc<Mutex<Option<(String, Elements)>>>;

fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(PoisonError::into_inner)
}
const MAX_JOB_NAME: usize = 100;
/// How long a job is followed after the printer accepted it; it carries on at the printer after that.
const FOLLOW_FOR: Duration = Duration::from_secs(300);

/// The destination's state and the closures its workers are made from.
#[derive(Default)]
pub(crate) struct NetTasks {
    transport: Option<Transport>,
    pub(crate) state: net::State,
    query: Option<QueryPrinter>,
    queried: Queried,
    /// The window's repaint handle, taken from the section as it is drawn (workers wake the UI with it).
    ctx: Option<egui::Context>,
}

impl NetTasks {
    pub(crate) fn set_transport(&mut self, transport: Transport) {
        self.transport = Some(transport);
        self.query = None;
    }

    pub(crate) fn transport(&self) -> Option<Transport> {
        self.transport.clone()
    }

    /// What the last query read of the printer at `address`, when it is the same printer.
    fn queried(&self, address: &str) -> Option<Elements> {
        lock(&self.queried).as_ref().filter(|(a, _)| a == address).map(|(_, e)| e.clone())
    }

    /// Draw the section; `sheets_ok`: the dialog's settings give something to print.
    pub(crate) fn show(&mut self, ui: &mut egui::Ui, sheets_ok: bool) -> net::Response {
        let (transport, ctx, queried) = (self.transport.clone(), ui.ctx().clone(), Arc::clone(&self.queried));
        self.ctx = Some(ctx.clone());
        let query = self.query.get_or_insert_with(|| query_printer(transport, Some(ctx), queried));
        let host = net::Host { validate: &validate, query, sheets_ok, formats: FORMATS };
        self.state.show(ui, egui::Id::unique("network-printer"), &host, &labels())
    }
}

/// The address checked against the network policy (a numeric loopback, link-local or private
/// address). The error is what the dialog shows under the field.
pub(crate) fn validate(text: &str) -> Result<(), Failure> {
    crate::netprint::canonical(text).map(|_| ()).map_err(|e| Failure::new(FailureKind::Policy, e))
}

/// What went wrong, as craft-ui words it. `open`: the failure is the printer's address being refused.
fn failure(e: &PrintError, open: bool) -> Failure {
    let kind = if e.native().is_some_and(|n| n.domain == "wsd-fault") {
        FailureKind::Fault
    } else if e.phase() == Some(Phase::Prepare) && e.kind() == ErrorKind::Failed {
        // The document could not be made into something the printer takes (the converter failed).
        FailureKind::Encode
    } else {
        match e.kind() {
            ErrorKind::InvalidRequest if open => FailureKind::Policy,
            ErrorKind::InvalidRequest | ErrorKind::ConsentRequired => FailureKind::Invalid,
            ErrorKind::UnsupportedFormat => FailureKind::NoCommonFormat,
            ErrorKind::Unsupported => FailureKind::Unsupported,
            ErrorKind::Unavailable | ErrorKind::Timeout | ErrorKind::Busy | ErrorKind::Io => FailureKind::Transport,
            ErrorKind::Failed => FailureKind::Protocol,
            ErrorKind::Rejected | ErrorKind::NotFound => FailureKind::Fault,
            ErrorKind::Cancelled => FailureKind::Stopped,
            _ => FailureKind::Other,
        }
    };
    Failure::new(kind, e.message()).with_outcome_unknown(e.outcome_unknown())
}

fn summary(elements: Elements) -> PrinterSummary {
    match elements {
        Elements::Wsd(elements) => {
            let caps = elements.capabilities.unwrap_or_default();
            let description = elements.description.unwrap_or_default();
            let status = elements.status.unwrap_or_default();
            PrinterSummary {
                name: description.name,
                color: description.color_supported,
                state: status.state,
                queued_jobs: status.queued_jobs,
                reasons: status.reasons,
                caps: caps_of(&caps, description.color_supported),
            }
        }
        Elements::Ipp(attrs) => ipp_summary(&attrs),
    }
}

/// An IPP printer's attributes as craft-ui's summary. Keywords and enums are translated into the
/// vocabulary of craft-ui's tokens; a list the printer did not publish is unknown, not unsupported.
fn ipp_summary(a: &crate::netprint::ipp::PrinterAttrs) -> PrinterSummary {
    let list = |items: Vec<String>| if items.is_empty() { Capability::Unknown } else { Capability::from_option_list(Some(&items)) };
    let sides: Vec<String> = a
        .sides
        .iter()
        .flatten()
        .filter_map(|k| {
            Some(match k.as_str() {
                "one-sided" => net::Sides::OneSided,
                "two-sided-long-edge" => net::Sides::LongEdge,
                "two-sided-short-edge" => net::Sides::ShortEdge,
                _ => return None,
            })
        })
        .map(|s| s.token().to_owned())
        .collect();
    // RFC 8011 enums: 3 portrait, 4 landscape, 5 reverse-landscape, 6 reverse-portrait.
    let orientations: Vec<String> = a
        .orientations
        .iter()
        .flatten()
        .filter_map(|n| match n {
            3 => Some(net::Orientation::Portrait),
            4 => Some(net::Orientation::Landscape),
            5 => Some(net::Orientation::ReverseLandscape),
            6 => Some(net::Orientation::ReversePortrait),
            _ => None,
        })
        .map(|o| o.token().to_owned())
        .collect();
    // 3 draft, 4 normal, 5 high.
    let qualities: Vec<String> = a
        .qualities
        .iter()
        .flatten()
        .filter_map(|n| match n {
            3 => Some(net::Quality::Draft),
            4 => Some(net::Quality::Normal),
            5 => Some(net::Quality::High),
            _ => None,
        })
        .map(|q| q.token().to_owned())
        .collect();
    let color_modes = match (&a.color_modes, a.color_supported) {
        (Some(modes), _) if !modes.is_empty() => {
            let mut out: Vec<String> = Vec::new();
            for m in modes {
                let token = match m.as_str() {
                    "color" => net::ColorMode::Color,
                    "monochrome" | "auto-monochrome" | "process-monochrome" => net::ColorMode::Monochrome,
                    _ => continue,
                };
                if !out.iter().any(|t| t == token.token()) {
                    out.push(token.token().to_owned());
                }
            }
            list(out)
        }
        (_, Some(true)) => list(vec![net::ColorMode::Color.token().to_owned(), net::ColorMode::Monochrome.token().to_owned()]),
        (_, Some(false)) => list(vec![net::ColorMode::Monochrome.token().to_owned()]),
        _ => Capability::Unknown,
    };
    let resolutions: Vec<net::Resolution> = a.resolutions.iter().flatten().map(|&(x, y)| net::Resolution::new(x, y)).collect();
    let state = crate::netprint::ipp::model::neutral_printer_state(a).map_or_else(|| "Unknown".to_owned(), |s| format!("{s:?}"));
    PrinterSummary {
        name: a.name.clone().or_else(|| a.make_and_model.clone()).unwrap_or_default(),
        color: a.color_supported,
        state,
        queued_jobs: None,
        reasons: a.state_reasons.iter().filter(|r| r.as_str() != "none").cloned().collect(),
        caps: Caps {
            formats: a.formats.clone(),
            sides: list(sides),
            orientations: list(orientations),
            qualities: list(qualities),
            color_modes,
            media_sizes: a.media.clone().map_or(Capability::Unknown, list),
            resolutions: if resolutions.is_empty() { Capability::Unknown } else { Capability::from_option_list(Some(&resolutions)) },
            copies: Capability::from_option_range(a.copies.and_then(|(lo, hi)| Some((u32::try_from(lo).ok()?, u32::try_from(hi).ok()?)))),
        },
    }
}

/// What the printer published, as craft-ui's three-state `Caps`. WS-Print cannot say "an empty
/// list": a list the printer left out is empty here, which reads as unknown (not unsupported), so the
/// control stays enabled with a note and the value is sent as a hint. WS-Print publishes no colour
/// list, only whether the printer prints in colour.
fn caps_of(caps: &wsd::Capabilities, color: Option<bool>) -> Caps {
    let tokens = |list: &[String]| if list.is_empty() { Capability::Unknown } else { Capability::from_option_list(Some(list)) };
    let resolutions: Vec<net::Resolution> = caps.resolutions.iter().map(|&(x, y)| net::Resolution::new(x, y)).collect();
    Caps {
        formats: caps.formats.clone(),
        sides: tokens(&caps.sides),
        orientations: tokens(&caps.orientations),
        qualities: tokens(&caps.qualities),
        color_modes: match color {
            None => Capability::Unknown,
            Some(true) => {
                Capability::from_option_list(Some(&[net::ColorMode::Color.token().to_owned(), net::ColorMode::Monochrome.token().to_owned()]))
            }
            Some(false) => Capability::from_option_list(Some(&[net::ColorMode::Monochrome.token().to_owned()])),
        },
        media_sizes: tokens(&caps.media_sizes),
        resolutions: if resolutions.is_empty() { Capability::Unknown } else { Capability::from_option_list(Some(&resolutions)) },
        copies: Capability::from_option_range(caps.copies),
    }
}

fn repaint(ctx: &Option<egui::Context>) {
    if let Some(ctx) = ctx {
        ctx.request_repaint();
    }
}

/// The worker that asks the printer at the (normalised) address what it is and what it lists. It
/// sends no job.
pub(crate) fn query_printer(transport: Option<Transport>, ctx: Option<egui::Context>, queried: Queried) -> QueryPrinter {
    Box::new(move |address: &str| {
        let (tx, rx) = mpsc::channel();
        let (address, transport, ctx, queried) = (address.to_owned(), transport.clone(), ctx.clone(), Arc::clone(&queried));
        std::thread::spawn(move || {
            let result = match Printer::open(&address, transport) {
                Ok(printer) => printer
                    .info()
                    .map(|elements| {
                        *lock(&queried) = Some((printer.address(), elements.clone()));
                        summary(elements)
                    })
                    .map_err(|e| failure(&e, false)),
                Err(e) => Err(failure(&e, true)),
            };
            let _ = tx.send(result);
            repaint(&ctx);
        });
        rx
    })
}

/// The `MediaSizeName` for a sheet size name of the Print dialog, for the sizes printers list.
pub(crate) fn media_for_paper(paper: &str) -> Option<&'static str> {
    Some(match paper {
        "US Letter" => "na_letter_8.5x11in",
        "US Legal" => "na_legal_8.5x14in",
        "Tabloid" => "na_ledger_11x17in",
        "A3" => "iso_a3_297x420mm",
        "A4" => "iso_a4_210x297mm",
        "A5" => "iso_a5_148x210mm",
        _ => return None,
    })
}

/// The job and its per-job options for the registry. `Photo` is craft-print's `Quality::Photo`: the
/// backend maps it to the printer's own quality when the printer lists one and says so when it does not.
/// `scaling` follows the Print page's layout ([`crate::print_dest`]): slide-shaped pages are fitted to
/// the printer's paper, notes pages and handouts are on the paper the person chose and print exactly.
fn options(request: &Request, job_name: &str, scaling: Scaling) -> (Job, JobOptions) {
    let name: String = job_name.chars().filter(|c| !c.is_control()).take(MAX_JOB_NAME).collect();
    let mut job = Job { scaling, ..Job::default() };
    if !name.trim().is_empty() {
        job.title = name;
    }
    job.copies = request.copies.unwrap_or(1);
    job.duplex = request.sides.map(|s| match s {
        net::Sides::OneSided => Duplex::Off,
        net::Sides::LongEdge => Duplex::LongEdge,
        net::Sides::ShortEdge => Duplex::ShortEdge,
    });
    let mut options = JobOptions::new();
    if let Some(o) = request.orientation {
        options = options.with_orientation(match o {
            net::Orientation::Portrait => Orientation::Portrait,
            net::Orientation::Landscape => Orientation::Landscape,
            net::Orientation::ReversePortrait => Orientation::ReversePortrait,
            net::Orientation::ReverseLandscape => Orientation::ReverseLandscape,
        });
    }
    match request.quality {
        Some(net::Quality::Draft) => options = options.with_quality(Quality::Draft),
        Some(net::Quality::Normal) => options = options.with_quality(Quality::Normal),
        Some(net::Quality::High) => options = options.with_quality(Quality::High),
        Some(net::Quality::Photo) => options = options.with_quality(Quality::Photo),
        None => {}
    }
    if let Some(color) = request.color {
        options = options.with_colour(match color {
            net::ColorMode::Color => ColourMode::Colour,
            net::ColorMode::Monochrome => ColourMode::Mono,
        });
    }
    if let Some(r) = request.resolution {
        options = options.with_resolution(Resolution::new(r.x_dpi, r.y_dpi));
    }
    if let Some(media) = &request.media_size {
        options = options.with_media(media.as_str());
    }
    if let Some(pages) = request.expected_pages {
        options = options.with_expected_pages(pages);
    }
    (job, options)
}

/// What the printer says about an accepted job, as craft-ui shows it.
fn progress(status: &JobStatus) -> Progress {
    Progress {
        stage: Stage::from_name(status.state.name()),
        sheets_done: status.sheets_done,
        sheets_total: status.sheets_total,
        notes: status.reasons.clone(),
    }
}

/// How following a job ended, as craft-ui's report. The document was accepted in every case, so
/// a watch that could not finish is "sent, outcome unknown", never a failure.
fn report(outcome: &WatchOutcome, printer: &str, warnings: &[String]) -> Report {
    let (kind, status) = match outcome {
        WatchOutcome::Terminal(s) => (
            match s.state {
                // craft-print models "completed with warnings" as Completed plus warnings on the status.
                JobState::Completed if !s.warnings.is_empty() => Outcome::CompletedWithWarnings,
                JobState::Completed => Outcome::Completed,
                JobState::CompletedWithErrors => Outcome::CompletedWithErrors,
                JobState::Cancelled => Outcome::Canceled,
                JobState::Aborted => Outcome::Aborted,
                _ => Outcome::AcceptedUnknown,
            },
            Some(s),
        ),
        WatchOutcome::Forgotten { last } => (Outcome::Forgotten, last.as_ref()),
        WatchOutcome::TimedOut { last } => (Outcome::StillRunning, last.as_ref()),
        _ => (Outcome::AcceptedUnknown, None),
    };
    let mut notes: Vec<String> = status.map(|s| s.reasons.clone()).unwrap_or_default();
    notes.extend(warnings.iter().cloned());
    if let WatchOutcome::Failed(e) = outcome {
        notes.push(e.message().to_owned());
    }
    Report { outcome: kind, printer: printer.to_owned(), sheets: status.and_then(|s| s.sheets_done).map_or(0, u64::from), notes }
}

/// The worker that sends the print-ready `pdf` and follows the job. `queried` is what the last query read of
/// the printer, when it is the one addressed. Each
/// call makes a new job, with the person's `confirmed` (never `interactive()`: it is the attestation
/// of the ticked box, taken when Print was pressed). `scaling`: see [`options`].
pub(crate) fn send_job(
    transport: Option<Transport>,
    ctx: Option<egui::Context>,
    name: String,
    pdf: Arc<Vec<u8>>,
    queried: Option<Elements>,
    confirmed: UserConfirmed,
    scaling: Scaling,
) -> SendJob {
    Box::new(move |address: &str, request: Request| {
        let (result_tx, result_rx) = mpsc::channel();
        let (update_tx, update_rx) = mpsc::channel();
        let cancel = CancelToken::new();
        let (address, transport, ctx) = (address.to_owned(), transport.clone(), ctx.clone());
        let (name, pdf, queried, worker_cancel) = (name.clone(), Arc::clone(&pdf), queried.clone(), cancel.clone());
        std::thread::spawn(move || {
            let (job, options) = options(&request, &name, scaling);
            let result = run_job(&address, transport, (job, options), &pdf, queried.as_ref(), confirmed, &worker_cancel, &update_tx, &ctx);
            let _ = result_tx.send(result);
            repaint(&ctx);
        });
        JobHandle::new(result_rx, Box::new(move || cancel.cancel())).with_updates(update_rx)
    })
}

#[allow(clippy::too_many_arguments)]
fn run_job(
    address: &str,
    transport: Option<Transport>,
    (job, options): (Job, JobOptions),
    pdf: &Arc<Vec<u8>>,
    queried: Option<&Elements>,
    confirmed: UserConfirmed,
    cancel: &CancelToken,
    updates: &mpsc::Sender<JobUpdate>,
    ctx: &Option<egui::Context>,
) -> Result<Report, Failure> {
    let printer = Printer::open(address, transport).map_err(|e| failure(&e, true))?;
    // What the person was shown is what the registry decides on: no second read of the printer.
    if let Some(elements) = queried {
        printer.remember(elements);
    }
    // Phase one: the printer accepts the document. A cancel before that sends nothing, or cancels the job just made.
    let sent = match printer.submit(Document::pdf(pdf.as_slice()), job, options, confirmed, cancel, &mut |_| {}) {
        Ok(sent) => sent,
        Err(e) if e.kind() == ErrorKind::Cancelled => {
            // The printer as the person was shown it: its name from the query, else its address.
            let name = queried.map(|e| summary(e.clone()).name).filter(|n| !n.trim().is_empty()).unwrap_or_else(|| printer.address());
            return Ok(Report { outcome: Outcome::Canceled, printer: name, sheets: 0, notes: Vec::new() });
        }
        Err(e) => return Err(failure(&e, false)),
    };
    let notes = sent.warnings.clone();
    let _ = updates.send(JobUpdate::Accepted(Progress { notes: notes.clone(), ..Progress::default() }));
    repaint(ctx);
    // Phase two: follow it. One cancel is sent if asked; the watch then carries on to the end state.
    let policy = WaitPolicy { interval: Some(Duration::from_millis(500)), ..WaitPolicy::default() }.with_max_wait(FOLLOW_FOR);
    let outcome = match printer.watch(&sent, policy) {
        Ok(mut watch) => {
            watch = watch.with_cancel(cancel.clone());
            loop {
                match watch.tick(printer.source()) {
                    WatchStep::Pending { next_in } => std::thread::sleep(next_in.min(Duration::from_millis(100))),
                    WatchStep::Update(status) => {
                        let _ = updates.send(JobUpdate::Status(progress(&status)));
                        repaint(ctx);
                    }
                    WatchStep::Done(outcome) => break outcome,
                    _ => std::thread::sleep(Duration::from_millis(100)),
                }
            }
        }
        Err(e) => WatchOutcome::Failed(e),
    };
    Ok(report(&outcome, &sent.printer_name, &notes))
}

/// The wording of the destination, in the interface language. Templates keep their `{...}` holes.
pub(crate) fn labels() -> Labels<'static> {
    Labels {
        address: "Printer address:",
        address_hint: "http://192.168.1.50:5357/service",
        address_note: "Only IP addresses on this network are accepted (private, link-local or loopback), not host names.",
        query_printer: "Query printer",
        querying: "Asking the printer…",
        query_first: "Query the printer first to see what it supports.",
        printer_status: "Printer status: {state}",
        reasons: "Reasons: {reasons}",
        no_common_format: "The printer accepts none of the formats this document can be sent in",
        copies: "Copies:",
        sides: "Two-sided:",
        sides_off: "Off",
        sides_long_edge: "Flip on long edge",
        sides_short_edge: "Flip on short edge",
        orientation: "Printer orientation:",
        portrait: "Portrait",
        landscape: "Landscape",
        reverse_portrait: "Reverse portrait",
        reverse_landscape: "Reverse landscape",
        quality: "Quality:",
        draft: "Draft",
        normal: "Normal",
        high: "High",
        photo: "Photo",
        color: "Color:",
        color_color: "Color",
        color_monochrome: "Grayscale",
        resolution: "Resolution:",
        resolution_dpi: "{dpi} dpi",
        resolution_dpi_xy: "{x} x {y} dpi",
        auto: "Auto",
        unsupported_copies: "This printer does not take a copies setting.",
        unsupported_sides: "This printer only prints one-sided.",
        unsupported_orientation: "This printer does not take an orientation setting.",
        unsupported_quality: "This printer does not take a quality setting.",
        unsupported_color: "This printer only prints in grayscale.",
        unsupported_resolution: "This printer does not take a resolution setting.",
        unconfirmed_note: "Not confirmed by the printer",
        confirm: "I confirm sending this document to this printer",
        sending: "Sending to {printer}…",
        canceling: "Canceling…",
        cancel: "Cancel",
        sent: "Sent to {printer}",
        accepted: "Accepted by {printer}",
        stage_waiting: "Waiting in the printer's queue",
        stage_held: "Held by the printer",
        stage_printing: "The printer is printing it",
        stage_stopped: "The printer paused the job; it needs attention",
        sheets: "Sheets printed: {n}",
        sheets_progress: "Sheets printed: {done} of {total}",
        outcome_warnings: "Printed with warnings",
        outcome_errors: "Printed with errors",
        outcome_canceled: "The job was canceled",
        outcome_aborted: "The printer stopped the job",
        outcome_forgotten: "The printer no longer lists the job; it most likely finished",
        outcome_still_running: "Still printing; the job continues on the printer",
        outcome_accepted_unknown: "Sent. The printer can't say whether it printed.",
        outcome_unknown_warning: "The printer may have received the document. Check the printer before sending again.",
        failure_policy: "The printer address was refused",
        failure_transport: "Couldn't reach the printer",
        failure_protocol: "The printer's reply couldn't be understood",
        failure_fault: "The printer refused the request",
        failure_no_common_format: "The printer accepts none of the formats this document can be sent in",
        failure_unsupported: "The printer doesn't support this setting",
        failure_encode: "The document couldn't be prepared",
        failure_invalid: "A setting isn't valid",
        failure_stopped: "The print job stopped unexpectedly",
        failure_other: "The print job stopped unexpectedly",
    }
}

impl SlideApp {
    /// Use `transport` for network printing instead of the built-in HTTP one (tests; embedding).
    pub fn set_wsd_transport(&mut self, transport: Transport) {
        self.net_print.set_transport(transport);
    }

    /// Whether the network printer's Color setting is Grayscale (as it will be sent).
    pub(crate) fn network_grayscale(&self) -> bool {
        self.net_print.state.draft.request(None).color == Some(net::ColorMode::Monochrome)
    }

    /// Send the print-ready `pdf` (of `page_count` sheets, the job's page count) to the
    /// queried printer, off the UI thread. It needs the person's confirmation, which it spends.
    /// `scaling` comes from the layout: fitted for slide-shaped pages, exact for a chosen paper.
    /// Returns `true` when a job started.
    pub(crate) fn network_send(&mut self, name: &str, pdf: Vec<u8>, page_count: usize, paper: &str, scaling: Scaling) -> bool {
        let net = &mut self.net_print;
        let address = net.state.draft.normalized_url();
        // The attestation comes from the ticked box and nothing else.
        let Ok(confirmed) = UserConfirmed::attest(Some(net.state.draft.confirmed)) else { return false };
        let queried = crate::netprint::canonical(&address).ok().and_then(|a| net.queried(&a));
        let send = send_job(net.transport(), net.ctx.clone(), name.to_owned(), Arc::new(pdf), queried, confirmed, scaling);
        let net = &mut self.net_print;
        // The sheets are the pages of the document sent: the total that progress shows until the printer reports one.
        net.state.draft.expected_pages = u32::try_from(page_count).ok();
        net.state.start_job(&send, validate(&address).is_ok(), true, FORMATS, media_for_paper(paper))
    }

    /// Collect finished work (each frame), also while the dialog is closed; with the dialog gone the
    /// person still hears how a job went.
    pub(crate) fn poll_network(&mut self) {
        let was_running = self.net_print.state.draft.running();
        let ctx = self.net_print.ctx.clone().unwrap_or_default();
        self.net_print.state.poll(&ctx);
        // With the Print page on screen the person sees how the job went there; otherwise a notice says.
        let on_print_page = self.backstage.as_ref().is_some_and(|b| b.page == crate::backstage::Page::Print);
        if !was_running || self.net_print.state.draft.running() || on_print_page {
            return;
        }
        let labels = labels();
        let message = match &self.net_print.state.draft.job {
            net::JobView::Finished(r) => {
                let text = labels.outcome_text(r.outcome).map_or_else(|| labels.sent.replace("{printer}", &r.printer), str::to_owned);
                Some(text)
            }
            net::JobView::Failed(f) => {
                let mut text = format!("{}: {}", labels.failure_heading(f.kind), f.detail);
                if f.outcome_unknown {
                    text = format!("{text} {}", labels.outcome_unknown_warning);
                }
                Some(text)
            }
            _ => None,
        };
        if let Some(message) = message {
            self.notify(message);
        }
    }
}

#[cfg(test)]
mod tests;
