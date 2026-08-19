//! The doctor's two output shapes.

use serde::Serialize;

use super::Concern;
use super::DOCTOR_VERSION;

/// The JSON envelope.
#[derive(Debug, Serialize)]
pub(super) struct JsonDoctor<'a> {
    version: u32,
    /// Always present, even when empty: a caller needs to see that the list is
    /// empty rather than infer it from absence.
    concerns: &'a [Concern],
}

/// Writes the diagnosis.
pub fn render(concerns: &[Concern], format: crate::report::Format, out: &mut dyn std::io::Write) {
    match format {
        crate::report::Format::Text => render_text(concerns, out),
        crate::report::Format::Json => render_json(concerns, out),
    }
}

pub(super) fn render_json(concerns: &[Concern], out: &mut dyn std::io::Write) {
    match serde_json::to_string_pretty(&JsonDoctor {
        version: DOCTOR_VERSION,
        concerns,
    }) {
        Ok(json) => {
            let _ = writeln!(out, "{json}");
        }
        Err(error) => {
            let _ = writeln!(out, r#"{{"error":"cannot serialise: {error}"}}"#);
        }
    }
}

pub(super) fn render_text(concerns: &[Concern], out: &mut dyn std::io::Write) {
    if concerns.is_empty() {
        let _ = writeln!(out, "No concerns.");
        return;
    }

    for concern in concerns {
        let subject = match (&concern.rule_id, &concern.path) {
            (Some(rule), Some(path)) => format!("{rule} · {path}"),
            (Some(rule), None) => rule.to_string(),
            (None, _) => "config".to_owned(),
        };
        let _ = writeln!(
            out,
            "{:<7} {} [{}]\n  {}",
            concern.level, subject, concern.code, concern.message
        );
        let _ = writeln!(out, "  fix: {}\n", concern.fix);
    }

    let _ = writeln!(
        out,
        "{} {}",
        concerns.len(),
        if concerns.len() == 1 {
            "concern"
        } else {
            "concerns"
        }
    );
}
