use super::*;
use craft_print::RetrySafety;
use craft_print::wsd::adapter::{DeviceUrl, Request as WireRequest, Response, TransportKind, WireError};
use craft_print::wsd::testing::{Ending, FakeConfig as Config, FakePrinter, MemTransport};
use deckcraft_engine::Session;
use net::{JobView, Query};

use crate::Services;

const ADDRESS: &str = "127.0.0.1:5357/printer";

fn app_with(transport: Transport) -> SlideApp {
    let mut app = SlideApp::new(Session::with_new(), Services::default());
    app.set_wsd_transport(transport);
    app.net_print.state.draft = net::Draft::for_address(ADDRESS);
    app
}

fn app(config: Config) -> (SlideApp, Arc<FakePrinter>) {
    let printer = FakePrinter::new(config);
    (app_with(Arc::new(MemTransport(printer.clone()))), printer)
}

fn wait(app: &mut SlideApp, done: impl Fn(&SlideApp) -> bool) {
    for _ in 0..400 {
        app.poll_network();
        if done(app) {
            return;
        }
        std::thread::sleep(Duration::from_millis(25));
    }
    panic!("timed out waiting");
}

fn state(app: &SlideApp) -> &net::State {
    &app.net_print.state
}

/// Ask the printer, as the Query button does, and wait for the answer.
fn query(app: &mut SlideApp) {
    let address_ok = validate(&app.net_print.state.draft.normalized_url()).is_ok();
    let worker = query_printer(app.net_print.transport(), None, Arc::clone(&app.net_print.queried));
    let started = app.net_print.state.start_query(&worker, address_ok);
    assert_eq!(started, address_ok);
    wait(app, |a| !state(a).draft.querying());
}

fn tiny_pdf() -> Vec<u8> {
    b"%PDF-1.4\n1 0 obj<</Type/Catalog/Pages 2 0 R>>endobj\n2 0 obj<</Type/Pages/Kids[3 0 R]/Count 1/MediaBox[0 0 200 200]>>endobj\n3 0 obj<</Type/Page/Parent 2 0 R>>endobj\ntrailer<</Root 1 0 R/Size 4>>\n%%EOF\n".to_vec()
}

fn send(app: &mut SlideApp) -> bool {
    app.network_send("report.pdf", tiny_pdf(), 1, "A4")
}

#[test]
fn the_policy_refuses_what_it_should_before_anything_is_sent() {
    for bad in ["printer.local", "http://8.8.8.8/", "https://10.0.0.1/", "http://user@10.0.0.1/", "http://0.0.0.0/"] {
        assert_eq!(validate(&net::normalize_url(bad)).map_err(|f| f.kind), Err(FailureKind::Policy), "{bad}");
    }
    assert!(validate(&net::normalize_url("192.168.1.50:5357/service")).is_ok());
    assert!(validate("http://[::1]:5357/x").is_ok());
}

#[test]
fn errors_map_onto_the_failures_the_dialog_words_and_keep_whether_the_outcome_is_unknown() {
    for (kind, want) in [
        (ErrorKind::InvalidRequest, FailureKind::Invalid),
        (ErrorKind::UnsupportedFormat, FailureKind::NoCommonFormat),
        (ErrorKind::Unsupported, FailureKind::Unsupported),
        (ErrorKind::Unavailable, FailureKind::Transport),
        (ErrorKind::Timeout, FailureKind::Transport),
        (ErrorKind::Failed, FailureKind::Protocol),
        (ErrorKind::Rejected, FailureKind::Fault),
        (ErrorKind::Cancelled, FailureKind::Stopped),
    ] {
        assert_eq!(failure(&PrintError::new(kind, "x"), false).kind, want, "{kind:?}");
    }
    assert_eq!(failure(&PrintError::new(ErrorKind::InvalidRequest, "x"), true).kind, FailureKind::Policy);
    // A document that could not be made into a format the printer takes is "couldn't be prepared".
    assert_eq!(failure(&PrintError::new(ErrorKind::Failed, "x").with_phase(Phase::Prepare), false).kind, FailureKind::Encode);
    assert!(failure(&PrintError::new(ErrorKind::Timeout, "x").with_safety(RetrySafety::OutcomeUnknown), false).outcome_unknown);
    assert!(!failure(&PrintError::new(ErrorKind::Busy, "x").with_safety(RetrySafety::Safe), false).outcome_unknown);
}

#[test]
fn a_query_fills_in_the_printer_and_what_it_lists() {
    let (mut app, printer) = app(Config::default());
    query(&mut app);
    let d = &state(&app).draft;
    assert!(matches!(d.query, Query::Done(_)), "{:?}", d.query);
    assert_eq!(d.offered_sides().len(), 2);
    assert_eq!(d.max_copies(), 10);
    // Reading is all that happened: no job exists.
    assert_eq!(printer.jobs(), 0);
}

#[test]
fn a_refused_address_is_never_contacted_and_a_failed_query_is_shown() {
    let (mut app, printer) = app(Config { http_error: Some(503), ..Default::default() });
    app.net_print.state.draft.url = "8.8.8.8".into();
    query(&mut app);
    assert!(!state(&app).draft.querying() && printer.actions().is_empty());
    app.net_print.state.draft.url = "127.0.0.1:1/x".into();
    query(&mut app);
    assert!(matches!(&state(&app).draft.query, Query::Failed(f) if f.kind == FailureKind::Transport), "{:?}", state(&app).draft.query);
}

#[test]
fn nothing_is_sent_without_the_confirmation_and_a_query() {
    let (mut app, printer) = app(Config::default());
    assert!(!send(&mut app), "not queried");
    query(&mut app);
    assert!(!send(&mut app), "not confirmed");
    assert_eq!(printer.jobs(), 0);
}

#[test]
fn a_confirmed_job_runs_to_the_end_and_spends_the_confirmation() {
    let (mut app, printer) = app(Config::default());
    query(&mut app);
    app.net_print.state.draft.confirmed = true;
    app.net_print.state.draft.copies = 2;
    assert!(send(&mut app));
    assert!(!state(&app).draft.confirmed && state(&app).draft.running());
    wait(&mut app, |a| !state(a).draft.running());
    match &state(&app).draft.job {
        JobView::Finished(r) => assert_eq!(r.outcome, Outcome::Completed),
        other => panic!("{other:?}"),
    }
    assert_eq!(printer.jobs(), 1);
    // The next print needs a new yes.
    assert!(!send(&mut app));
    assert_eq!(printer.jobs(), 1);
}

#[test]
fn the_job_is_accepted_first_and_only_then_followed_to_its_end() {
    let (mut app, printer) = app(Config { ending: Ending::Hangs, ..Default::default() });
    query(&mut app);
    app.net_print.state.draft.confirmed = true;
    assert!(send(&mut app));
    // The printer has the document and the job is still going: accepted, not finished.
    wait(&mut app, |a| matches!(state(a).draft.job, JobView::Accepted { .. }));
    assert_eq!(printer.jobs(), 1);
    assert!(state(&app).draft.running());
    // It carries on at the printer; the dialog can cancel it, once.
    assert!(app.net_print.state.cancel_job());
    wait(&mut app, |a| !state(a).draft.running());
    assert!(matches!(&state(&app).draft.job, JobView::Finished(r) if r.outcome == Outcome::Canceled), "{:?}", state(&app).draft.job);
    assert_eq!(printer.actions().iter().filter(|a| *a == "SendDocument").count(), 1);
    assert_eq!(printer.actions().iter().filter(|a| *a == "CancelJob").count(), 1);
}

#[test]
fn the_documents_page_count_gives_the_progress_a_total_the_printer_does_not_report() {
    let (mut app, _printer) = app(Config { ending: Ending::Hangs, ..Default::default() });
    query(&mut app);
    let draft = &mut app.net_print.state.draft;
    draft.confirmed = true;
    draft.copies = 2;
    assert!(send(&mut app));
    // One sheet, two copies: the app supplied the page count, craft-ui the estimate; WS-Print reports no total.
    assert_eq!(state(&app).draft.expected_pages, Some(1));
    wait(&mut app, |a| matches!(&state(a).draft.job, JobView::Accepted { progress, .. } if progress.sheets_total.is_some()));
    match &state(&app).draft.job {
        JobView::Accepted { progress, .. } => assert_eq!(progress.sheets_total, Some(2), "{progress:?}"),
        other => panic!("{other:?}"),
    }
    assert!(app.net_print.state.cancel_job());
    wait(&mut app, |a| !state(a).draft.running());
}

#[test]
fn a_job_the_printer_aborts_is_reported_as_such() {
    let (mut app, _printer) = app(Config { ending: Ending::Aborts("DocumentFormatError"), ..Default::default() });
    query(&mut app);
    app.net_print.state.draft.confirmed = true;
    assert!(send(&mut app));
    wait(&mut app, |a| !state(a).draft.running());
    assert!(matches!(&state(&app).draft.job, JobView::Finished(r) if r.outcome == Outcome::Aborted), "{:?}", state(&app).draft.job);
}

/// Delivers every request to the fake and loses the reply to `SendDocument`: the printer did the
/// work and the client saw the connection close with no answer.
struct LoseSendReply(Arc<FakePrinter>);

impl craft_print::wsd::adapter::Transport for LoseSendReply {
    fn post(&self, _: &DeviceUrl, request: WireRequest) -> Result<Response, WireError> {
        let (status, content_type, body) = self.0.handle(&request.content_type, &request.body);
        if request.soap_action.ends_with("SendDocument") {
            return Err(WireError::transport(TransportKind::ConnectionClosed, "closed without a reply"));
        }
        Ok(Response::new(status, Some(content_type), body))
    }
}

#[test]
fn a_send_whose_outcome_is_unknown_asks_to_check_the_printer_and_is_never_repeated() {
    let printer = FakePrinter::new(Config::default());
    let mut app = app_with(Arc::new(LoseSendReply(printer.clone())));
    query(&mut app);
    app.net_print.state.draft.confirmed = true;
    assert!(send(&mut app));
    wait(&mut app, |a| !state(a).draft.running());
    match &state(&app).draft.job {
        JobView::Failed(f) => assert!(f.outcome_unknown, "{f:?}"),
        other => panic!("{other:?}"),
    }
    assert_eq!(printer.actions().iter().filter(|a| *a == "SendDocument").count(), 1, "{:?}", printer.actions());
    assert!(!printer.actions().iter().any(|a| a == "CancelJob"), "a job that may be printing is left alone");
    // The confirmation was spent: sending again needs a new yes.
    assert!(!send(&mut app));
}

#[test]
fn options_carry_only_what_was_requested() {
    let r = Request {
        copies: Some(2),
        sides: Some(net::Sides::LongEdge),
        orientation: Some(net::Orientation::Landscape),
        quality: Some(net::Quality::High),
        color: Some(net::ColorMode::Monochrome),
        resolution: Some(net::Resolution::new(600, 300)),
        media_size: Some("iso_a4_210x297mm".into()),
        expected_pages: Some(5),
    };
    let (job, o) = options(&r, "a\u{7}b.pdf");
    assert_eq!((job.copies, job.title.as_str(), job.duplex), (2, "ab.pdf", Some(Duplex::LongEdge)));
    assert_eq!(o.orientation, Some(Orientation::Landscape));
    assert_eq!(o.quality, Some(Quality::High));
    assert_eq!((o.colour, o.resolution), (ColourMode::Mono, Some(Resolution::new(600, 300))));
    assert_eq!((o.media.as_deref(), o.expected_pages), (Some("iso_a4_210x297mm"), Some(5)));
    let (job, o) = options(&Request::default(), "");
    assert_eq!((job.title, job.copies, job.duplex), (Job::default().title, 1, None));
    assert_eq!(o, JobOptions::new());
    // Photo is craft-print's own quality now: it goes to the registry like the others.
    let (_, o) = options(&Request { quality: Some(net::Quality::Photo), ..Request::default() }, "x");
    assert_eq!(o.quality, Some(Quality::Photo));
}

#[test]
fn what_a_printer_leaves_out_is_unknown_not_unsupported() {
    let none = caps_of(&wsd::Capabilities::default(), None);
    assert_eq!(
        (none.sides, none.qualities, none.copies, none.color_modes),
        (Capability::Unknown, Capability::Unknown, Capability::Unknown, Capability::Unknown)
    );
    let listed =
        wsd::Capabilities { sides: vec!["OneSided".into()], copies: Some((1, 9)), resolutions: vec![(600, 600)], ..wsd::Capabilities::default() };
    let caps = caps_of(&listed, Some(false));
    assert!(caps.sides.is_supported() && caps.copies.is_supported() && caps.resolutions.is_supported());
    assert_eq!(caps.color_modes, Capability::from_option_list(Some(&["Monochrome".to_owned()])));
    assert!(caps_of(&listed, Some(true)).color_modes.is_supported());
}

#[test]
fn completed_with_warnings_is_completed_plus_warnings() {
    let mut status = JobStatus::new(JobState::Completed);
    let outcome = |s: &JobStatus| report(&WatchOutcome::Terminal(s.clone()), "p", &[]).outcome;
    assert_eq!(outcome(&status), Outcome::Completed);
    status.warnings.push("a setting was ignored".into());
    assert_eq!(outcome(&status), Outcome::CompletedWithWarnings);
}

#[test]
fn paper_names_map_to_the_sizes_printers_list() {
    assert_eq!(media_for_paper("A4"), Some("iso_a4_210x297mm"));
    assert_eq!(media_for_paper("ARCH A (9 x 12 in)"), None);
}

fn reads(printer: &FakePrinter) -> usize {
    printer.actions().iter().filter(|a| *a == "GetPrinterElements").count()
}

#[test]
fn photo_quality_chosen_in_the_dialog_reaches_the_ticket() {
    let (mut app, printer) = app(Config { qualities: vec!["Normal", "Photo"], ..Default::default() });
    query(&mut app);
    assert!(state(&app).draft.offered_qualities().contains(&net::Quality::Photo), "the printer lists it, so the dialog offers it");
    app.net_print.state.draft.confirmed = true;
    app.net_print.state.draft.quality = Some(net::Quality::Photo);
    assert!(send(&mut app));
    wait(&mut app, |a| !state(a).draft.running());
    assert!(matches!(&state(&app).draft.job, JobView::Finished(r) if r.outcome == Outcome::Completed), "{:?}", state(&app).draft.job);
    let ticket = printer.job(1).unwrap().ticket;
    assert!(ticket.contains("Photo"), "Photo missing from {ticket}");
}

#[test]
fn a_printer_that_lists_no_pdf_is_refused_before_any_job_exists() {
    // DeckCraft registers no raster converter: a raster-only printer cannot be sent the PDF, and says so
    // (craft-ui blocks Print with the "no common format" reason; nothing reaches the printer).
    let (mut app, printer) = app(Config { formats: vec!["image/pwg-raster"], ..Default::default() });
    query(&mut app);
    app.net_print.state.draft.confirmed = true;
    assert!(!send(&mut app));
    assert_eq!(printer.jobs(), 0);
    assert_eq!(reads(&printer), 1, "only the query: {:?}", printer.actions());
}

#[test]
fn a_printer_that_lists_pdf_gets_the_print_ready_pdf_unchanged() {
    let (mut app, printer) = app(Config::default());
    query(&mut app);
    app.net_print.state.draft.confirmed = true;
    assert!(send(&mut app));
    wait(&mut app, |a| !state(a).draft.running());
    assert_eq!(printer.job(1).unwrap().document.as_deref(), Some(&tiny_pdf()[..]));
    assert_eq!(reads(&printer), 2, "the query and the backend's own: {:?}", printer.actions());
}

/// An in-memory IPP printer: just enough of RFC 8011 for a query, a job and its status.
mod ipp_fake {
    use std::sync::Mutex;

    use craft_print::ipp::adapter::{DeviceUrl, Request as WireRequest, Response, Transport, WireError};
    use craft_print::ipp::codec::{Attribute, Group, GroupTag, Limits, Message, Value};

    #[derive(Default)]
    pub struct FakeIpp {
        pub ops: Mutex<Vec<u16>>,
        pub document: Mutex<Option<Vec<u8>>>,
        pub formats: Vec<&'static str>,
    }

    impl FakeIpp {
        pub fn new() -> Self {
            Self { formats: vec!["application/pdf", "image/jpeg"], ..Self::default() }
        }

        fn printer(&self) -> Group {
            Group::new(GroupTag::Printer)
                .with(Attribute::name_value("printer-name", "Front desk"))
                .with(Attribute::enumeration("printer-state", 3))
                .with(Attribute::keyword("printer-state-reasons", "none"))
                .with(Attribute::boolean("printer-is-accepting-jobs", true))
                .with(Attribute::set("operations-supported", [2, 4, 5, 6, 8, 9, 10, 11].map(Value::Enum).to_vec()))
                .with(Attribute::set("document-format-supported", self.formats.iter().map(|m| Value::MimeMediaType((*m).into())).collect::<Vec<_>>()))
                .with(Attribute::keywords("sides-supported", &["one-sided", "two-sided-long-edge"]))
                .with(Attribute::keywords("print-color-mode-supported", &["auto", "color", "monochrome"]))
                .with(Attribute::keywords("media-supported", &["iso_a4_210x297mm", "na_letter_8.5x11in"]))
                .with(Attribute::new("copies-supported", Value::Range { lower: 1, upper: 99 }))
                .with(Attribute::new("printer-resolution-supported", Value::Resolution { cross_feed: 600, feed: 600, units: 3 }))
                .with(Attribute::set("print-quality-supported", vec![Value::Enum(3), Value::Enum(4), Value::Enum(5)]))
                .with(Attribute::new("multiple-operation-time-out", Value::Integer(120)))
                .with(Attribute::new("printer-up-time", Value::Integer(5000)))
        }
    }

    impl Transport for FakeIpp {
        fn post(&self, _: &DeviceUrl, request: WireRequest) -> Result<Response, WireError> {
            let message = Message::decode(&request.body, &Limits::default().with_max_message(usize::MAX)).expect("well-formed IPP");
            self.ops.lock().expect("ops").push(message.code);
            if message.code == 0x0002 {
                *self.document.lock().expect("document") = Some(message.data.clone());
            }
            let op = Group::new(GroupTag::Operation)
                .with(Attribute::new("attributes-charset", Value::Charset("utf-8".into())))
                .with(Attribute::new("attributes-natural-language", Value::NaturalLanguage("en".into())));
            let mut reply = Message::new(message.major, message.minor, 0, message.request_id).with_group(op);
            match message.code {
                0x000B => reply.groups.push(self.printer()),
                0x0002 | 0x0005 => reply.groups.push(
                    Group::new(GroupTag::Job)
                        .with(Attribute::integer("job-id", 7))
                        .with(Attribute::uri("job-uri", "ipp://127.0.0.1:631/ipp/print/7"))
                        .with(Attribute::enumeration("job-state", 3)),
                ),
                0x0009 => reply.groups.push(
                    Group::new(GroupTag::Job)
                        .with(Attribute::integer("job-id", 7))
                        .with(Attribute::enumeration("job-state", 9))
                        .with(Attribute::keyword("job-state-reasons", "job-completed-successfully"))
                        .with(Attribute::integer("job-media-sheets-completed", 1)),
                ),
                _ => {}
            }
            Ok(Response::new(200, Some("application/ipp".into()), reply.encode().expect("encodes")))
        }
    }
}

#[test]
fn an_ipp_printer_is_queried_and_sent_the_pdf_and_followed_to_the_end() {
    let fake = Arc::new(ipp_fake::FakeIpp::new());
    let transport: Transport = fake.clone();
    let mut app = app_with(transport);
    app.net_print.state.draft = net::Draft::for_address("ipp://127.0.0.1:631/ipp/print");
    query(&mut app);
    let d = &state(&app).draft;
    assert!(matches!(&d.query, Query::Done(s) if s.name == "Front desk"), "{:?}", d.query);
    assert_eq!(d.offered_sides().len(), 2, "one-sided and long edge");
    assert_eq!(d.max_copies(), 99);
    assert_eq!(fake.ops.lock().unwrap().as_slice(), &[0x000B], "a query sends no job");
    app.net_print.state.draft.confirmed = true;
    assert!(send(&mut app));
    wait(&mut app, |a| !state(a).draft.running());
    match &state(&app).draft.job {
        JobView::Finished(r) => assert_eq!((r.outcome, r.printer.as_str()), (Outcome::Completed, "Front desk")),
        other => panic!("{other:?}"),
    }
    assert_eq!(fake.document.lock().unwrap().as_deref(), Some(&tiny_pdf()[..]));
}

#[test]
fn an_ipp_printer_without_pdf_is_refused_before_a_job_exists() {
    let fake = Arc::new(ipp_fake::FakeIpp { formats: vec!["image/jpeg", "image/urf"], ..Default::default() });
    let transport: Transport = fake.clone();
    let mut app = app_with(transport);
    app.net_print.state.draft = net::Draft::for_address("ipp://127.0.0.1:631/ipp/print");
    query(&mut app);
    app.net_print.state.draft.confirmed = true;
    assert!(!send(&mut app));
    assert!(fake.document.lock().unwrap().is_none());
}

#[test]
fn the_colour_choice_is_called_grayscale() {
    assert_eq!(labels().color_monochrome, "Grayscale");
    assert!(!labels().unsupported_color.contains("black and white"));
}
