//! `archwarden decisions find` -- has this already been rejected?

use archwarden_api::similar::{Hit, How, search};
use camino::Utf8Path;

/// One or many, said in English.
fn plural(count: usize, one: &'static str, many: &'static str) -> &'static str {
    if count == 1 { one } else { many }
}

use crate::command::{Location, Output};
use crate::commands::query::prepare;
use crate::exit::Exit;

/// Searches the declared decisions for anything the terms reach.
pub(crate) fn find_decisions(
    location: Location<'_>,
    working_directory: &Utf8Path,
    terms: &[String],
    format: crate::report::Format,
    output: &mut Output<'_>,
) -> Exit {
    let Ok((_, compiled)) = prepare(location, working_directory, output) else {
        return Exit::ConfigProblem;
    };

    let query = terms.join(" ");
    let hits = search(&compiled, &query);

    match format {
        crate::report::Format::Json => render_json(
            &archwarden_api::similar::similar_json(&compiled, &query),
            output.out,
        ),
        crate::report::Format::Text => render_text(&query, &hits, output.out),
    }

    // Clean either way. "Nothing has been rejected under this name" is an
    // answer, not a failure -- the doctor concern is where this becomes a
    // gate, and a command somebody runs to ask a question should not fail
    // them for asking.
    Exit::Clean
}

/// How a match happened, as a phrase a reader can act on.
fn how(reason: &archwarden_api::similar::Reason) -> String {
    match reason.how {
        How::Exact => format!("`{}` exact", reason.query),
        How::Prefix => format!("`{}` prefix of `{}`", reason.query, reason.candidate),
        How::Edits(1) => format!(
            "`{}` differs from `{}` by one character",
            reason.query, reason.candidate
        ),
        How::Edits(distance) => format!(
            "`{}` differs from `{}` by {distance} characters",
            reason.query, reason.candidate
        ),
    }
}

fn render_text(query: &str, hits: &[Hit<'_>], out: &mut dyn std::io::Write) {
    if hits.is_empty() {
        let _ = writeln!(out, "Nothing here has been said about `{query}`.");
        return;
    }

    let _ = writeln!(
        out,
        "{} {} `{query}`:",
        hits.len(),
        plural(hits.len(), "place mentions", "places mention"),
    );

    let mut last: Option<&str> = None;
    for hit in hits {
        // The decision's heading once, however many of its fields matched: a
        // reader is deciding whether to open one document, not four.
        if last != Some(hit.decision.id.as_str()) {
            let _ = writeln!(out, "\n  {} — {}", hit.decision.id, hit.decision.title);
            last = Some(hit.decision.id.as_str());
        }
        let _ = writeln!(out, "    {}  {:?}", hit.at.path(), hit.text);
        for reason in &hit.reasons {
            let _ = writeln!(out, "      {}", how(reason));
        }
    }
}

/// The JSON shape lives in the api, shared with the MCP tool.
fn render_json(body: &serde_json::Value, out: &mut dyn std::io::Write) {
    match serde_json::to_string_pretty(body) {
        Ok(json) => {
            let _ = writeln!(out, "{json}");
        }
        Err(error) => {
            let _ = writeln!(out, r#"{{"error":"cannot serialise: {error}"}}"#);
        }
    }
}
