//! The module map: what a session should know before it writes anything.
//!
//! Issue #66. A `SessionStart` hook can put archwarden's rules in an agent's
//! context without the user referencing a file from their `CLAUDE.md` by hand.
//! What goes in is the question, and the answer is **a pointer, not the guide**.
//!
//! The full digest costs context in every session, including the ones touching
//! no governed file — and a long block is the first thing compaction drops,
//! which is precisely the moment this exists to survive. So this is the module
//! names, one line each on what they govern, and the two commands that answer
//! the rest.
//!
//! > A short thing that is read beats a complete thing that is compacted away.
//!
//! It is deliberately not [`crate::guide`]. That one teaches every rule and is
//! written to be grepped on disk; this one is written to be *carried*, and its
//! job is to make an agent ask [`crate::describe`] rather than to answer for it.
//!
//! # A repository with no modules still gets a map
//!
//! A config may declare rules at the top level and no modules at all, and a
//! session told nothing would take the silence for "nothing is governed here" —
//! the failure `CONFIG.md` calls the worst a linter has, one layer out. So the
//! map always says how many rules are active and always names the commands.

use archwarden_core::compiled::CompiledConfig;

/// One module, as a session needs to know it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Module {
    /// The label findings report in brackets.
    pub id: String,
    /// The globs it governs, as its author wrote them.
    pub scope: Vec<String>,
    /// Why it exists, when its author said.
    ///
    /// A module is a bigger decision than any rule inside it — one sentence
    /// explaining why `domain` is sealed explains every rule under it — which
    /// is why this is the line worth spending context on. Issue #46.
    pub why: Option<String>,
    /// How many rules it carries.
    pub rules: usize,
}

/// What a session is told.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Map {
    /// The modules, in the order their author declared them.
    pub modules: Vec<Module>,
    /// Every active rule, modules included.
    pub rules: usize,
    /// Rules belonging to no module, typically import boundaries.
    pub unscoped_rules: usize,
}

impl Map {
    /// Whether there is anything to say at all.
    ///
    /// A config with no rules is a config that governs nothing, and a session
    /// told "archwarden governs this repository" about it would be misled.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.rules == 0
    }
}

/// Reads the map from a configuration.
///
/// Takes the declared config rather than the compiled one: the map is about
/// what the author *said*, and `why` and the globs as written are exactly what
/// compilation lowers away.
#[must_use]
pub fn map(config: &archwarden_config::config::Config, compiled: &CompiledConfig) -> Map {
    let modules: Vec<Module> = config
        .modules
        .iter()
        .map(|module| Module {
            id: module.id.as_str().to_owned(),
            scope: module.scope.iter().map(ToOwned::to_owned).collect(),
            why: module.why.clone(),
            rules: module.rules.len(),
        })
        .collect();

    Map {
        modules,
        rules: compiled.rules().count(),
        unscoped_rules: config.rules.len(),
    }
}

/// The map as the lines a session carries.
///
/// Written as prose rather than JSON because its reader is a language model
/// with a context budget, and the shape that costs fewest tokens for the same
/// meaning is the one that survives.
///
/// `invocation` is how archwarden can actually be run from here — the same
/// answer the pre-write hook installs, because telling an agent to run
/// something it cannot run is the same defect one layer out.
#[must_use]
pub fn render(map: &Map, invocation: &str) -> String {
    use std::fmt::Write as _;

    let mut text = String::new();
    let _ = writeln!(
        text,
        "This repository's architecture is enforced by archwarden: {} active {}.",
        map.rules,
        if map.rules == 1 { "rule" } else { "rules" }
    );

    if !map.modules.is_empty() {
        let _ = writeln!(text, "\nModules:");
        for module in &map.modules {
            let _ = write!(text, "\n  {}", module.id);
            if !module.scope.is_empty() {
                let _ = write!(text, "  ({})", module.scope.join(", "));
            }
            let _ = writeln!(text);
            if let Some(why) = &module.why {
                let _ = writeln!(text, "    {why}");
            }
        }
    }

    if map.unscoped_rules > 0 {
        let (noun, verb) = if map.unscoped_rules == 1 {
            ("rule", "belongs")
        } else {
            ("rules", "belong")
        };
        let _ = writeln!(
            text,
            "\n{} {noun} {verb} to no module — typically import boundaries, which \
             are cross-module by nature.",
            map.unscoped_rules,
        );
    }

    // The point of the whole thing. The map says a rule exists; these say what
    // it requires, and an agent that runs them does not have to have been told.
    let _ = write!(
        text,
        "\nBefore creating or editing a file here, ask:\n\
         \x20 `{invocation} describe <path>` — the rules that apply to a path\n\
         \x20 `{invocation} scaffold <path>` — the smallest shape that would satisfy them\n"
    );

    text
}

#[cfg(test)]
mod tests {
    use super::*;
    use camino::Utf8PathBuf;

    fn prepared(
        source: &str,
    ) -> (
        tempfile::TempDir,
        archwarden_config::config::Config,
        CompiledConfig,
    ) {
        let guard = tempfile::tempdir().expect("temp dir");
        let root = Utf8PathBuf::from_path_buf(guard.path().to_path_buf()).expect("utf-8");
        std::fs::write(root.join("arch.config.json"), source).expect("write");

        let prepared = crate::prepare(
            crate::Location {
                config: None,
                root: None,
            },
            &root,
        )
        .expect("the config is good");

        (guard, prepared.merged.config, prepared.compiled)
    }

    const GOVERNED: &str = r#"{"version":0,
        "modules":[
          {"id":"domain","scope":["packages/domain/**"],
           "why":"published, so it may not reach into the app",
           "rules":[{"type":"structure","id":"domain-shape","level":"error",
                     "roots":["packages/domain/src/*"],"allowed_subfolders":["types"]}]},
          {"id":"app","scope":["packages/app/**"],
           "rules":[{"type":"structure","id":"app-shape","level":"error",
                     "roots":["packages/app/src/*"],"allowed_subfolders":["types"]}]}],
        "rules":[{"type":"import-boundary","id":"no-infra","level":"error",
                  "from":["packages/**"],"forbid_import_from":["packages/infra/**"]}]}"#;

    /// The names, the globs, and the sentence their author wrote — which is
    /// the line worth spending context on, because it explains every rule
    /// under it at once.
    #[test]
    fn the_map_carries_the_modules_and_the_reasons_their_authors_gave() {
        let (_guard, config, compiled) = prepared(GOVERNED);

        let map = map(&config, &compiled);

        assert_eq!(map.modules.len(), 2);
        assert_eq!(map.modules[0].id, "domain");
        assert_eq!(map.modules[0].scope, ["packages/domain/**"]);
        assert_eq!(
            map.modules[0].why.as_deref(),
            Some("published, so it may not reach into the app")
        );
        assert_eq!(map.modules[1].why, None, "a module without one has none");
    }

    /// Declaration order, which is the order their author wrote them in and
    /// the order a reader will look for them.
    #[test]
    fn modules_come_back_in_the_order_they_were_declared() {
        let (_guard, config, compiled) = prepared(GOVERNED);

        let read = map(&config, &compiled);
        let ids: Vec<&str> = read
            .modules
            .iter()
            .map(|module| module.id.as_str())
            .collect();

        assert_eq!(ids, ["domain", "app"]);
    }

    /// Rules belonging to no module are counted rather than dropped. They are
    /// typically the import boundaries, which are the rules an agent is most
    /// likely to break and least likely to guess.
    #[test]
    fn rules_outside_every_module_are_counted_and_named() {
        let (_guard, config, compiled) = prepared(GOVERNED);
        let map = map(&config, &compiled);

        assert_eq!(map.rules, 3);
        assert_eq!(map.unscoped_rules, 1);

        let text = render(&map, "archwarden");
        assert!(text.contains("belongs to no module"), "{text}");
    }

    /// The rendered map is a pointer: it names the modules and then sends the
    /// reader to `describe` and `scaffold`. It must not try to be the guide.
    #[test]
    fn the_rendered_map_points_at_the_commands_that_answer() {
        let (_guard, config, compiled) = prepared(GOVERNED);

        let text = render(&map(&config, &compiled), "./node_modules/.bin/archwarden");

        assert!(
            text.contains("./node_modules/.bin/archwarden describe <path>"),
            "{text}"
        );
        assert!(
            text.contains("./node_modules/.bin/archwarden scaffold <path>"),
            "{text}"
        );
        assert!(
            !text.contains("allowed_subfolders"),
            "the rules themselves are what `describe` is for: {text}"
        );
    }

    /// Short enough to survive. The whole argument for a pointer over the
    /// guide is that a long block is the first thing compaction drops, so the
    /// length is a property worth pinning rather than hoping for.
    #[test]
    fn the_map_stays_short_enough_to_be_carried() {
        let (_guard, config, compiled) = prepared(GOVERNED);

        let text = render(&map(&config, &compiled), "archwarden");

        assert!(
            text.len() < 800,
            "a map that grew into a guide is a map that gets compacted away: {} bytes\n{text}",
            text.len()
        );
    }

    /// A config with rules and no modules still gets a map. Silence would read
    /// as "nothing is governed here", which is the failure this exists to
    /// refuse.
    #[test]
    fn a_config_with_no_modules_still_says_what_governs_the_repository() {
        let (_guard, config, compiled) = prepared(
            r#"{"version":0,"rules":[{"type":"import-boundary","id":"no-infra",
                "level":"error","from":["packages/**"],"forbid_import_from":["packages/infra/**"]}]}"#,
        );

        let map = map(&config, &compiled);
        assert!(map.modules.is_empty());
        assert!(!map.is_empty(), "one rule is not nothing");

        let text = render(&map, "archwarden");
        assert!(text.contains("1 active rule"), "{text}");
        assert!(text.contains("describe <path>"), "{text}");
    }

    /// And a config that governs nothing says so by being empty, so the hook
    /// can stay silent rather than announce a gate that is not there.
    #[test]
    fn a_config_with_no_rules_at_all_is_an_empty_map() {
        let (_guard, config, compiled) = prepared(r#"{"version":0,"rules":[]}"#);

        assert!(map(&config, &compiled).is_empty());
    }

    /// A config with no modules gets no `Modules:` heading. A heading with
    /// nothing under it reads as a repository whose modules failed to load,
    /// which is a different and much louder statement than "there are none".
    #[test]
    fn no_modules_means_no_module_section() {
        let (_guard, config, compiled) = prepared(
            r#"{"version":0,"rules":[{"type":"import-boundary","id":"no-infra",
                "level":"error","from":["packages/**"],
                "forbid_import_from":["packages/infra/**"]}]}"#,
        );

        let text = render(&map(&config, &compiled), "archwarden");

        assert!(!text.contains("Modules:"), "{text}");
    }

    /// A module without a scope of its own is a label, and has been since
    /// before scopes existed. Its line carries the name and nothing in
    /// brackets — empty parentheses would read as a scope that matched
    /// nothing.
    #[test]
    fn a_module_with_no_scope_of_its_own_gets_no_empty_brackets() {
        let (_guard, config, compiled) = prepared(
            r#"{"version":0,"modules":[{"id":"domain","rules":[
                {"type":"structure","id":"domain-shape","level":"error",
                 "roots":["packages/domain/src/*"],"allowed_subfolders":["types"]}]}]}"#,
        );

        let text = render(&map(&config, &compiled), "archwarden");

        assert!(text.contains("domain"), "{text}");
        assert!(!text.contains("()"), "no empty brackets: {text}");
    }

    /// And a config whose rules all live in modules says nothing about rules
    /// belonging to none. A sentence about zero of them is a line of context
    /// spent saying nothing.
    #[test]
    fn no_unscoped_rules_means_no_sentence_about_them() {
        let (_guard, config, compiled) = prepared(
            r#"{"version":0,"modules":[{"id":"domain","scope":["packages/domain/**"],
                "rules":[{"type":"structure","id":"domain-shape","level":"error",
                 "roots":["packages/domain/src/*"],"allowed_subfolders":["types"]}]}]}"#,
        );

        let map = map(&config, &compiled);
        assert_eq!(map.unscoped_rules, 0);

        let text = render(&map, "archwarden");
        assert!(!text.contains("to no module"), "{text}");
    }

    /// One rule reads as one rule, and one module's rule count is its own.
    /// Plurals are worth a test because the alternative is prose that says
    /// "1 active rules" in the one place an agent reads first.
    #[test]
    fn one_of_something_reads_as_one() {
        let (_guard, config, compiled) = prepared(
            r#"{"version":0,"rules":[{"type":"import-boundary","id":"no-infra",
                "level":"error","from":["packages/**"],
                "forbid_import_from":["packages/infra/**"]}]}"#,
        );

        let text = render(&map(&config, &compiled), "archwarden");

        assert!(text.contains("1 active rule."), "{text}");
        assert!(!text.contains("1 active rules"), "{text}");
        assert!(text.contains("1 rule belongs to no module"), "{text}");
    }
}
