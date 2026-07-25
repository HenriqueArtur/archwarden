//! Property tests for the config layer.
//!
//! `docs/TESTING.md` requires that config loading never panics: malformed
//! input must yield a structured error. That is not a nicety. archwarden runs
//! as an agent pre-write hook, where a panic gives the user a stack trace in
//! place of their file being written, and as a CI gate, where it produces a
//! failure nobody can act on.
//!
//! Unit tests cover the malformed inputs we thought of. These cover the ones
//! we did not.

// This is test code; see the note in archwarden-cli/tests/cli.rs about why the
// clippy relaxations do not reach an integration-test crate.
#![allow(clippy::expect_used)]

use archwarden_config::{compile, config::Config, extends::MergedConfig};
use camino::Utf8PathBuf;
use proptest::prelude::*;

/// Wraps a parsed config the way `extends::merge` would, so `compile` can be
/// exercised without touching a filesystem.
fn merged(config: Config) -> MergedConfig {
    let path = Utf8PathBuf::from("arch.config.json");
    MergedConfig {
        config,
        path: path.clone(),
        root: Utf8PathBuf::from("."),
        sources: vec![path],
    }
}

proptest! {
    /// Arbitrary bytes are not a config, and saying so is the only acceptable
    /// outcome. Anything that reaches this function came off a disk somebody
    /// else controls.
    #[test]
    fn arbitrary_text_never_panics_the_parser(text in ".*") {
        let _ = serde_json::from_str::<Config>(&text);
    }

    /// Valid JSON that is not a config is the more interesting case: the
    /// parser gets far enough to walk the structure before rejecting it.
    #[test]
    fn arbitrary_json_never_panics_the_parser(value in any::<proptest::sample::Index>()
        .prop_map(|i| i.index(6))
        .prop_flat_map(arbitrary_json))
    {
        let _ = serde_json::from_str::<Config>(&value);
    }

    /// A config that parses must also compile or fail cleanly. Compilation
    /// builds globs and regexes out of user text, which is where a panic would
    /// otherwise be easiest to provoke.
    #[test]
    fn a_parsed_config_never_panics_the_compiler(
        roots in ".{0,40}",
        pattern in ".{0,40}",
        ignore in ".{0,40}",
    ) {
        let json = format!(
            r#"{{"version":0,"ignore":[{}],"rules":[
                {{"type":"structure","id":"generated","level":"error",
                 "roots":[{}],"filename_patterns":[{}]}}]}}"#,
            serde_json::to_string(&ignore).expect("string encodes"),
            serde_json::to_string(&roots).expect("string encodes"),
            serde_json::to_string(&pattern).expect("string encodes"),
        );

        if let Ok(config) = serde_json::from_str::<Config>(&json) {
            let _ = compile::compile(&merged(config));
        }
    }

    /// Rule ids come from user text and are validated on the way in. Whatever
    /// the input, the answer is accept or reject, never a crash.
    #[test]
    fn an_arbitrary_rule_id_never_panics(id in ".{0,60}") {
        let json = format!(
            r#"{{"version":0,"rules":[
                {{"type":"structure","id":{},"level":"error","roots":"x/*"}}]}}"#,
            serde_json::to_string(&id).expect("string encodes"),
        );
        let _ = serde_json::from_str::<Config>(&json);
    }

    /// The export-name template is user text interpreted by our own renderer,
    /// which makes it the most likely place for an unbalanced-delimiter bug.
    #[test]
    fn an_arbitrary_export_template_never_panics(template in r"[{}()a-z_]{0,40}") {
        let json = format!(
            r#"{{"version":0,"rules":[
                {{"type":"naming","id":"n","level":"error","roots":"x/*",
                 "file_pattern":"^(?<name>[a-z]+)\\.ts$",
                 "must_export":{{"kind":"function","name":{}}}}}]}}"#,
            serde_json::to_string(&template).expect("string encodes"),
        );

        if let Ok(config) = serde_json::from_str::<Config>(&json) {
            let _ = compile::compile(&merged(config));
        }
    }
}

/// A small generator of arbitrary JSON documents.
///
/// Hand-rolled rather than pulled from a crate: the shapes that matter here
/// are the ones a config could plausibly be confused with, and six is enough
/// to cover them.
fn arbitrary_json(kind: usize) -> BoxedStrategy<String> {
    match kind {
        0 => Just("null".to_owned()).boxed(),
        1 => any::<i64>().prop_map(|n| n.to_string()).boxed(),
        2 => any::<bool>().prop_map(|b| b.to_string()).boxed(),
        3 => ".{0,20}"
            .prop_map(|s| serde_json::to_string(&s).unwrap_or_else(|_| "\"\"".to_owned()))
            .boxed(),
        4 => proptest::collection::vec(any::<i64>(), 0..5)
            .prop_map(|v| serde_json::to_string(&v).unwrap_or_else(|_| "[]".to_owned()))
            .boxed(),
        _ => proptest::collection::hash_map("[a-z$]{1,8}", any::<i64>(), 0..5)
            .prop_map(|m| serde_json::to_string(&m).unwrap_or_else(|_| "{}".to_owned()))
            .boxed(),
    }
}
