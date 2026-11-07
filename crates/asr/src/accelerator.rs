//! Which processor a speech backend runs on, and what this build can actually use.
//!
//! The default is the CPU, everywhere: it needs no runtime and no hardware beyond the machine
//! itself. The enum is the seam an optional accelerator backend plugs into later — a
//! `WhisperTranscriber` asks [`resolve`] what to run and reports the answer through
//! [`crate::ProviderStatus`], so no caller of the feature changes when one is added.
//!
//! Detection is deliberately independent of the backend: this build has no non-CPU backend, so a
//! machine that has OpenVINO installed still runs on the CPU and is told so
//! ("runs on CPU; OpenVINO NPU/GPU not available") rather than failing or pretending.

use serde::{Deserialize, Serialize};

/// A processor the inference can run on.
#[derive(Clone, Copy, Debug, Default, Hash, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Accelerator {
    /// Any x86/ARM CPU, every platform.
    #[default]
    Cpu,
    /// Intel NPU/GPU through OpenVINO (x86 Windows and Linux). Not built into this release.
    OpenVino,
}

impl Accelerator {
    pub const ALL: [Accelerator; 2] = [Accelerator::Cpu, Accelerator::OpenVino];

    pub fn as_str(self) -> &'static str {
        match self {
            Accelerator::Cpu => "cpu",
            Accelerator::OpenVino => "openVino",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Accelerator::Cpu => "CPU",
            Accelerator::OpenVino => "OpenVINO NPU/GPU",
        }
    }

    /// Parse the spelling settings and commands use (`cpu`, `openvino`).
    pub fn parse(s: &str) -> Option<Accelerator> {
        let key: String = s.chars().filter(|c| c.is_ascii_alphanumeric()).collect::<String>().to_ascii_lowercase();
        Accelerator::ALL.into_iter().find(|a| a.as_str().to_ascii_lowercase() == key)
    }

    /// Whether this build has an inference backend for the accelerator. Only the CPU one ships.
    pub fn implemented(self) -> bool {
        matches!(self, Accelerator::Cpu)
    }
}

/// What the host reports for "which processor" in `show.subtitles.state`.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Capabilities {
    /// Accelerators this build can run on here.
    pub available: Vec<Accelerator>,
    /// Accelerators the machine was seen to have, whether or not a backend exists for them.
    pub detected: Vec<Accelerator>,
    /// What is being used: the request when it can run, else [`Accelerator::Cpu`].
    pub chosen: Accelerator,
    /// What the settings asked for.
    pub requested: Accelerator,
    /// Why `chosen` is not `requested`, in one sentence the status line can show.
    pub note: Option<String>,
}

/// Pick the accelerator to run on: the requested one when this build and machine can use it, the
/// CPU otherwise (with a note when the request could not be met).
pub fn resolve(requested: Accelerator, detected: &[Accelerator]) -> Capabilities {
    let mut available: Vec<Accelerator> = detected.iter().copied().filter(|a| a.implemented()).collect();
    if !available.contains(&Accelerator::Cpu) {
        available.insert(0, Accelerator::Cpu);
    }
    available.sort_by_key(|a| *a != Accelerator::Cpu);
    available.dedup();
    let chosen = if available.contains(&requested) { requested } else { Accelerator::Cpu };
    let note = (chosen != requested).then(|| format!("runs on CPU; {} not available", requested.label()));
    Capabilities { available, detected: detected.to_vec(), chosen, requested, note }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_cpu_is_always_available_and_nothing_else_is_yet() {
        let c = resolve(Accelerator::Cpu, &[Accelerator::Cpu]);
        assert_eq!(c.chosen, Accelerator::Cpu);
        assert_eq!(c.available, vec![Accelerator::Cpu]);
        assert_eq!(c.note, None);
        // Requesting an accelerator this build has no backend for runs on the CPU and says so.
        let c = resolve(Accelerator::OpenVino, &[Accelerator::Cpu, Accelerator::OpenVino]);
        assert_eq!(c.chosen, Accelerator::Cpu);
        assert_eq!(c.requested, Accelerator::OpenVino);
        assert_eq!(c.available, vec![Accelerator::Cpu]);
        assert!(c.detected.contains(&Accelerator::OpenVino));
        assert_eq!(c.note.as_deref(), Some("runs on CPU; OpenVINO NPU/GPU not available"));
        // A host that lists no accelerators still gets the CPU.
        let c = resolve(Accelerator::Cpu, &[]);
        assert_eq!(c.chosen, Accelerator::Cpu);
        assert!(c.available.contains(&Accelerator::Cpu));
    }

    #[test]
    fn names_round_trip() {
        for a in Accelerator::ALL {
            assert_eq!(Accelerator::parse(a.as_str()), Some(a));
            assert!(!a.label().is_empty());
        }
        assert_eq!(Accelerator::parse("OPENVINO"), Some(Accelerator::OpenVino));
        assert_eq!(Accelerator::parse("gpu"), None);
        assert_eq!(Accelerator::default(), Accelerator::Cpu);
    }
}
