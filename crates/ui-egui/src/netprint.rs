//! Network printing on craft-print's `wsd` (WS-Print) and `ipp` backends. Cargo feature
//! `network-print`; not built for wasm32 (there is no socket to open).
//!
//! The protocols, the print ticket and the job classification are craft-print's. This module is the
//! part DeckCraft adds on top, shared by the Print page's glue ([`crate::network_glue`]) and the tests:
//!
//! * [`Printer`]: one printer the person chose, behind a private [`craft_print::Registry`] that holds
//!   one backend (WS-Print, or IPP when the address says so) with only that printer added. It asks
//!   what the printer is ([`Printer::info`]), sends a document ([`Printer::submit`], through
//!   `Registry::submit_job_with_details`) and follows the accepted job ([`Printer::watch`]).
//! * **Two phases.** [`Printer::submit`] returns when the printer has *accepted* the document
//!   ([`Sent`]); it never waits for the paper. Following the job is a separate, caller-driven step.
//! * **No retry of an unknown outcome.** Nothing here sends a document twice.
//! * **Formats are the registry's.** The document is the print-ready PDF. A printer that lists PDF
//!   gets it as it is; with no way to get to a format the printer lists the print is
//!   [`ErrorKind::UnsupportedFormat`] before a job exists (no converter is registered: see
//!   [`register_converter`]). Every job needs a [`UserConfirmed`].

use std::sync::{Arc, Mutex, PoisonError, RwLock};

pub use craft_print::ipp;
use craft_print::ipp::{IppBackend, PrinterAttrs, PrinterUri};
pub use craft_print::wsd;
use craft_print::wsd::error::Op;
use craft_print::wsd::{Client, PrinterElements, PrinterUrl, WsdBackend};
pub use craft_print::{
    CancelToken, ColourMode, Document, DocumentConverter, DocumentFormat, Duplex, ErrorKind, Job, JobId, JobOptions, JobSource, JobState, JobStatus,
    JobWatch, Orientation, Phase, PrintError, PrinterDetails, Quality, Registry, Resolution, Scaling, Submitted, UserConfirmed, WaitPolicy,
    WatchOutcome, WatchStep,
};
use craft_print::{SubmitRequest, namespaced_id};

/// A transport shared with worker threads: the plain-HTTP one of craft-wsd, or an in-memory fake.
pub type Transport = Arc<dyn wsd::adapter::Transport + Send + Sync>;

/// The converter every [`Printer`] opened afterwards registers on its registry.
static CONVERTER: RwLock<Option<Arc<dyn DocumentConverter>>> = RwLock::new(None);

/// Register the converter the app brings (PDF to a raster format, say): printers opened from now
/// on use it for a document the printer does not list. DeckCraft registers none today.
pub fn register_converter(converter: Arc<dyn DocumentConverter>) {
    *CONVERTER.write().unwrap_or_else(PoisonError::into_inner) = Some(converter);
}

fn registered_converter() -> Option<Arc<dyn DocumentConverter>> {
    CONVERTER.read().unwrap_or_else(PoisonError::into_inner).clone()
}

/// Which protocol an address speaks.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Proto {
    /// WS-Print (Web Services for Devices), usually `http://ip:5357/uuid`.
    Wsd,
    /// IPP, `ipp://ip[:631]/path` or an `http://` address on an `/ipp/` path or port 631.
    Ipp,
}

impl Proto {
    /// The backend's registry name, the prefix of its printer and job ids.
    const fn backend(self) -> &'static str {
        match self {
            Self::Wsd => "wsd",
            Self::Ipp => "ipp",
        }
    }

    /// The protocol an address selects: `ipp://`, a path starting `/ipp`, or port 631 mean IPP;
    /// anything else is WS-Print.
    pub fn of(address: &str) -> Self {
        let t = address.trim();
        let lower = t.to_ascii_lowercase();
        if lower.starts_with("ipp://") {
            return Self::Ipp;
        }
        let rest = lower.split_once("://").map_or(lower.as_str(), |(_, r)| r);
        let (authority, path) = rest.split_once('/').map_or((rest, ""), |(a, p)| (a, p));
        let port_631 = authority.rsplit_once(':').is_some_and(|(_, p)| p == "631");
        if port_631 || path == "ipp" || path.starts_with("ipp/") { Self::Ipp } else { Self::Wsd }
    }
}

/// The canonical form of an address (what the printer is called inside the registry), or the
/// network policy's reason for refusing it. Opens no connection.
pub fn canonical(address: &str) -> Result<String, String> {
    match Proto::of(address) {
        Proto::Wsd => PrinterUrl::parse(address).map(|u| u.as_url()).map_err(|e| e.to_string()),
        Proto::Ipp => PrinterUri::parse(address).map(|u| u.canonical()).map_err(|e| e.to_string()),
    }
}

/// What a read of a printer returned, whichever protocol it speaks.
#[derive(Clone, Debug)]
pub enum Elements {
    Wsd(PrinterElements),
    Ipp(Box<PrinterAttrs>),
}

/// What the backend keeps for itself, whatever the job: set when a printer is opened.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Settings {
    /// Refuse a setting the printer does not list, and send every setting as mandatory.
    pub strict: bool,
}

/// What a printer accepted: its job and what was sent.
#[derive(Clone, Debug)]
pub struct Sent {
    /// The registry's own record of the acceptance (message, warnings, what it can do afterwards).
    pub submitted: Submitted,
    /// The accepted job (`wsd:<printer url>#<number>` or `ipp:...`).
    pub job: JobId,
    /// The job's number on the printer.
    pub job_number: u32,
    /// The format the document was sent in, as the printer spells it.
    pub format: String,
    pub bytes_sent: usize,
    pub printer_name: String,
    /// Settings that were dropped or changed because the printer does not list them.
    pub warnings: Vec<String>,
}

enum Wire {
    Wsd { client: Client<Transport> },
    Ipp { client: ipp::Client<Transport> },
}

/// One network printer: checked address, transport and the registry that follows its jobs.
pub struct Printer {
    proto: Proto,
    id: String,
    wire: Wire,
    registry: Registry,
    known: Mutex<Option<Known>>,
}

/// What a read of the printer left: the details the registry takes, the formats as the printer
/// spells them and its name.
#[derive(Clone)]
struct Known {
    details: PrinterDetails,
    listed: Vec<String>,
    name: Option<String>,
}

impl Printer {
    /// A printer at `address` (a numeric loopback, link-local or private IP address over plain
    /// HTTP: craft-wsd's policy decides, and a refusal is [`ErrorKind::InvalidRequest`]).
    /// `transport` replaces the plain-HTTP one (tests, embedding). Opens no connection.
    pub fn open(address: &str, transport: Option<Transport>) -> Result<Self, PrintError> {
        Self::open_with(address, transport, Settings::default())
    }

    /// [`Self::open`] with the backend's own [`Settings`].
    pub fn open_with(address: &str, transport: Option<Transport>, settings: Settings) -> Result<Self, PrintError> {
        let transport = transport.unwrap_or_else(wsd::adapter::default_transport);
        let proto = Proto::of(address);
        let registry = Registry::new();
        let (wire, id) = match proto {
            Proto::Wsd => {
                let backend = WsdBackend::new().with_transport(Arc::clone(&transport)).with_strict(settings.strict);
                let added = backend.add(address)?;
                let url = PrinterUrl::parse(&added).map_err(PrintError::from)?;
                let id = url.as_url();
                registry.register(Box::new(backend.clone())).map_err(|e| PrintError::new(ErrorKind::Failed, e.to_string()))?;
                (Wire::Wsd { client: Client::new(Arc::clone(&transport), url) }, id)
            }
            Proto::Ipp => {
                let backend = IppBackend::new().with_transport(Arc::clone(&transport)).with_strict(settings.strict);
                let id = backend.add(address)?;
                let uri = PrinterUri::parse(&id).map_err(PrintError::from)?;
                registry.register(Box::new(backend)).map_err(|e| PrintError::new(ErrorKind::Failed, e.to_string()))?;
                (Wire::Ipp { client: ipp::Client::new(Arc::clone(&transport), uri) }, id)
            }
        };
        if let Some(converter) = registered_converter() {
            registry.set_converter(converter);
        }
        Ok(Self { proto, id, wire, registry, known: Mutex::new(None) })
    }

    /// Which protocol this printer speaks.
    pub fn proto(&self) -> Proto {
        self.proto
    }

    /// The canonical address.
    pub fn address(&self) -> String {
        self.id.clone()
    }

    /// What the printer is and what it accepts. Sends no job. The answer is kept: [`Self::submit`]
    /// uses it instead of reading the printer again.
    pub fn info(&self) -> Result<Elements, PrintError> {
        let elements = match &self.wire {
            Wire::Wsd { client } => {
                let e = wsd::printer_info(client).map_err(|e| e.print_error(Op::Query))?;
                Elements::Wsd(e)
            }
            Wire::Ipp { client } => {
                let a = client.get_printer_attributes(&[]).map_err(|e| e.print_error(ipp::Op::Query))?;
                Elements::Ipp(Box::new(a))
            }
        };
        self.remember(&elements);
        Ok(elements)
    }

    /// Hand over the answer of an earlier read of this printer (a page that has just shown it), so
    /// that the next [`Self::submit`] reads nothing before the registry decides on the format.
    pub fn remember(&self, elements: &Elements) {
        *self.known.lock().unwrap_or_else(PoisonError::into_inner) = Some(known_from(self.proto, &self.id, elements));
    }

    fn remembered(&self) -> Option<Known> {
        self.known.lock().unwrap_or_else(PoisonError::into_inner).clone()
    }

    /// Send `document` and return when the printer has accepted it.
    ///
    /// A document in a format the printer lists is sent as it is; any other goes through the
    /// registered [`DocumentConverter`], and one that cannot be made into a listed format is
    /// [`ErrorKind::UnsupportedFormat`] before a job exists. Set [`JobOptions::expected_pages`] to
    /// the document's page count. An error says whether sending again is safe
    /// ([`PrintError::retry_safety`]).
    pub fn submit(
        &self,
        document: Document<'_>,
        job: Job,
        options: JobOptions,
        consent: UserConfirmed,
        cancel: &CancelToken,
        progress: &mut dyn FnMut(craft_print::Progress),
    ) -> Result<Sent, PrintError> {
        if cancel.is_cancelled() {
            return Err(PrintError::new(ErrorKind::Cancelled, "the print job was cancelled before it started"));
        }
        let known = match self.remembered() {
            Some(known) => known,
            None => {
                self.info()?;
                self.remembered().ok_or_else(|| PrintError::new(ErrorKind::Failed, "the printer's details were not kept"))?
            }
        };
        let (format, len) = (document.format, document.bytes.len());
        let job = Job { printer: Some(namespaced_id(self.proto.backend(), &self.id)), ..job };
        let request =
            SubmitRequest::new(document, job).with_options(options).with_consent(consent).with_cancel(cancel.clone()).with_progress(progress);
        let submitted = self.registry.submit_job_with_details(request, Some(known.details.clone()))?;
        let job_id =
            submitted.job.clone().ok_or_else(|| PrintError::new(ErrorKind::Failed, "the printer accepted the document but gave no job handle"))?;
        let job_number = match self.proto {
            Proto::Wsd => WsdBackend::parse_job_id(&job_id)?.job_number,
            Proto::Ipp => IppBackend::parse_job_id(&job_id)?.job_number,
        };
        let conversion = submitted.conversion.clone();
        let sent_as = conversion.as_ref().map_or(format, |c| c.to);
        let spelled =
            known.listed.iter().find(|l| DocumentFormat::from_mime(l) == Some(sent_as)).cloned().unwrap_or_else(|| sent_as.mime().to_owned());
        Ok(Sent {
            job: job_id,
            job_number,
            format: spelled,
            bytes_sent: conversion.as_ref().map_or(len, |c| c.bytes),
            printer_name: known.name.unwrap_or_else(|| self.id.clone()),
            warnings: submitted.warnings.clone(),
            submitted,
        })
    }

    /// A follower for an accepted job. Tick it with [`Self::source`].
    pub fn watch(&self, sent: &Sent, policy: WaitPolicy) -> Result<JobWatch, PrintError> {
        JobWatch::for_submitted(&sent.submitted, policy).ok_or_else(|| PrintError::new(ErrorKind::Failed, "the accepted job cannot be followed"))
    }

    /// Where a [`JobWatch`] of this printer asks for status and sends its cancel.
    pub fn source(&self) -> &dyn JobSource {
        &self.registry
    }

    /// The printer's own address as a [`PrinterUrl`]/[`PrinterUri`] check, for tests.
    #[cfg(test)]
    pub(crate) fn wire_is_ipp(&self) -> bool {
        matches!(self.wire, Wire::Ipp { .. })
    }
}

/// The registry's view of a printer, from what the read returned: the formats craft-print has a
/// name for (in the printer's order), colour, paper, copies and the listed resolutions.
fn known_from(proto: Proto, id: &str, elements: &Elements) -> Known {
    let mut details = PrinterDetails { id: id.to_owned(), name: id.to_owned(), ..PrinterDetails::default() };
    let mut listed = Vec::new();
    let mut take_formats = |formats: &[String], details: &mut PrinterDetails| {
        for spelled in formats {
            if let Some(format) = DocumentFormat::from_mime(spelled)
                && !details.formats.contains(&format)
            {
                details.formats.push(format);
                listed.push(spelled.clone());
            }
        }
    };
    let name = match elements {
        Elements::Wsd(e) => {
            let name = e.description.as_ref().map(|d| d.name.clone()).filter(|n| !n.is_empty());
            details.colour = e.description.as_ref().and_then(|d| d.color_supported);
            if let Some(caps) = &e.capabilities {
                take_formats(&caps.formats, &mut details);
                details.two_sided = if caps.sides.is_empty() { None } else { Some(caps.sides.iter().any(|s| s.starts_with("TwoSided"))) };
                details.paper.clone_from(&caps.media_sizes);
                details.max_copies = caps.copies.map(|(_, max)| max);
                details.resolutions = caps.resolutions.iter().map(|&(x, y)| Resolution::new(x, y)).collect();
            }
            name
        }
        Elements::Ipp(a) => {
            let name = a.name.clone().or_else(|| a.make_and_model.clone()).filter(|n| !n.is_empty());
            details.colour = a.color_supported;
            take_formats(&a.formats, &mut details);
            details.two_sided = a.sides.as_ref().map(|s| s.iter().any(|k| k.starts_with("two-sided")));
            if let Some(media) = &a.media {
                details.paper.clone_from(media);
            }
            details.max_copies = a.copies.and_then(|(_, max)| u32::try_from(max).ok());
            details.resolutions = a.resolutions.iter().flatten().map(|&(x, y)| Resolution::new(x, y)).collect();
            name
        }
    };
    let _ = proto;
    if let Some(n) = &name {
        details.name.clone_from(n);
    }
    Known { details, listed, name }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_protocol_follows_the_address() {
        for (address, want) in [
            ("http://192.168.1.50:5357/uuid", Proto::Wsd),
            ("192.168.1.50:5357/printer", Proto::Wsd),
            ("ipp://192.168.1.50/ipp/print", Proto::Ipp),
            ("IPP://192.168.1.50:631/x", Proto::Ipp),
            ("http://192.168.1.50:631/anything", Proto::Ipp),
            ("http://192.168.1.50/ipp/print", Proto::Ipp),
            ("http://192.168.1.50/ippish", Proto::Wsd),
        ] {
            assert_eq!(Proto::of(address), want, "{address}");
        }
    }

    #[test]
    fn an_address_opens_with_the_protocol_it_selects() {
        let printer = Printer::open("http://127.0.0.1:9/other", None).expect("allowed");
        assert_eq!(printer.address(), "http://127.0.0.1:9/other");
        assert!(!printer.wire_is_ipp());
        let ipp = Printer::open("ipp://127.0.0.1:9/ipp/print", None).expect("allowed");
        assert!(ipp.wire_is_ipp());
    }
}
