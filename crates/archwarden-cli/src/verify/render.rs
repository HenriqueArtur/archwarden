//! What a verification looks like as text and as JSON.

use super::{Verdict, Verification};

/// One rule's verdict, as JSON.
#[derive(Debug, serde::Serialize)]
pub(crate) struct JsonVerification<'a> {
    rule_id: &'a str,
    kind: &'a str,
    /// `fires`, `silent` or `unverified`: a stable slug a CI job can branch on.
    verdict: &'static str,
    /// What the rule was handed, when there was something to hand it.
    #[serde(skip_serializing_if = "Option::is_none")]
    probe: Option<&'a str>,
    /// Why nothing could be handed to it, when that is the answer.
    #[serde(skip_serializing_if = "Option::is_none")]
    reason: Option<&'a str>,
}

/// Writes the verifications in the requested format.
pub fn render(
    verifications: &[Verification],
    format: crate::report::Format,
    out: &mut dyn std::io::Write,
) {
    match format {
        crate::report::Format::Json => {
            let envelope: Vec<JsonVerification<'_>> = verifications
                .iter()
                .map(|verification| {
                    let (verdict, probe, reason) = match &verification.verdict {
                        Verdict::Fires { on } => ("fires", Some(on.as_str()), None),
                        Verdict::Silent { on } => ("silent", Some(on.as_str()), None),
                        Verdict::Unverified { why } => ("unverified", None, Some(why.as_str())),
                    };
                    JsonVerification {
                        rule_id: &verification.rule_id,
                        kind: verification.kind,
                        verdict,
                        probe,
                        reason,
                    }
                })
                .collect();
            match serde_json::to_string_pretty(&envelope) {
                Ok(json) => {
                    let _ = writeln!(out, "{json}");
                }
                Err(error) => {
                    let _ = writeln!(out, r#"{{"error":"cannot serialise: {error}"}}"#);
                }
            }
        }
        crate::report::Format::Text => render_text(verifications, out),
    }
}

pub(crate) fn render_text(verifications: &[Verification], out: &mut dyn std::io::Write) {
    for verification in verifications {
        match &verification.verdict {
            Verdict::Fires { on } => {
                let _ = writeln!(out, "✓ {} — fires on {on}", verification.rule_id);
            }
            Verdict::Silent { on } => {
                let _ = writeln!(out, "✗ {} — silent on {on}", verification.rule_id);
            }
            Verdict::Unverified { why } => {
                let _ = writeln!(out, "? {} — not verified: {why}", verification.rule_id);
            }
        }
    }

    let silent = verifications
        .iter()
        .filter(|verification| verification.verdict.is_silent())
        .count();
    let unverified = verifications
        .iter()
        .filter(|verification| matches!(verification.verdict, Verdict::Unverified { .. }))
        .count();
    let fires = verifications.len() - silent - unverified;

    let _ = writeln!(
        out,
        "\n{fires} enforce something, {silent} enforce nothing, {unverified} not verified"
    );

    // Said on every run, including the clean one. The command's whole value is
    // that it does not overstate what it checked -- an agent reading a wall of
    // ticks and concluding its config is sound would be back in the state the
    // issue described, one level up.
    let _ = writeln!(
        out,
        "\nThis proves each rule fires on a violation of its own terms. It cannot\n\
         know what you meant: a list missing an entry is a question about intent,\n\
         and a rule with a hole in it ticks here."
    );
}
