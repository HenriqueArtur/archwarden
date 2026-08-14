//! The pages in English.
//!
//! Split from the trait so each language is one file — which is what makes
//! "adding a language is one file" true, and what lets the spell checker read
//! this one and skip the others. See `_typos.toml`.

use super::{Phrases, plural};

/// English.
pub struct En;

impl Phrases for En {
    fn guide_title(&self) -> &'static str {
        "archwarden — the architecture as declared"
    }
    fn guide_stamp(&self) -> &'static str {
        "archwarden · the architecture as declared"
    }
    fn guide_heading(&self, rules: usize, modules: usize) -> String {
        format!(
            "{rules} {} across {modules} {}",
            plural(rules, "rule", "rules"),
            plural(modules, "module", "modules")
        )
    }
    fn tally_rules(&self) -> &'static str {
        "rules"
    }
    fn tally_modules(&self) -> &'static str {
        "modules"
    }
    fn tally_cross_module(&self) -> &'static str {
        "cross-module"
    }
    fn tally_no_reason(&self) -> &'static str {
        "say no why"
    }
    fn tally_decisions(&self) -> &'static str {
        "decisions"
    }
    fn decisions_eyebrow(&self) -> &'static str {
        "what was decided"
    }
    fn decisions_heading(&self) -> &'static str {
        "The decisions, and what enforces them"
    }
    fn decisions_lede(&self) -> &'static str {
        "An architecture is a set of choices somebody made. The rules below are \
         how each one is kept; a decision nothing enforces is a choice this \
         repository is only describing."
    }
    fn enforced_by(&self, rules: &str) -> String {
        format!("Enforced by {rules}")
    }
    fn enforced_by_nothing(&self) -> &'static str {
        "Nothing enforces this."
    }
    fn written_down_in(&self) -> &'static str {
        "Written down in"
    }
    fn decision_status(&self, status: &str) -> String {
        status.to_owned()
    }
    fn rules_eyebrow(&self) -> &'static str {
        "the walls"
    }
    fn rules_heading(&self) -> &'static str {
        "Every rule, and what it is for"
    }
    fn rules_lede(&self) -> &'static str {
        "The requirements are what to do; the reason is why. A rule whose reason \
         is nowhere is one a reader can only obey."
    }
    fn applies_to(&self, globs: &str) -> String {
        format!("applies to {globs}")
    }
    fn guide_footer(&self) -> &'static str {
        "the architecture as declared · what it currently is needs"
    }

    fn report_title(&self) -> &'static str {
        "archwarden — the architecture as it stands"
    }
    fn report_stamp(&self) -> &'static str {
        "archwarden · the architecture as it stands"
    }
    fn report_heading(&self, modules: usize, walls: usize, crossed: usize) -> String {
        format!(
            "{modules} {}, {walls} {}, {crossed} of them being crossed",
            plural(modules, "module", "modules"),
            plural(walls, "wall", "walls")
        )
    }
    fn tally_files(&self) -> &'static str {
        "files"
    }
    fn tally_errors(&self) -> &'static str {
        "errors now"
    }
    fn tally_warnings(&self) -> &'static str {
        "warnings"
    }
    fn tally_accepted(&self) -> &'static str {
        "accepted debt"
    }
    fn tally_undecided(&self) -> &'static str {
        "not decided"
    }

    fn map_eyebrow(&self) -> &'static str {
        "the map"
    }
    fn map_heading(&self) -> &'static str {
        "What the config governs"
    }
    fn map_lede(&self) -> &'static str {
        "Every module the rules select, with the reason it exists. A module with \
         no reason recorded is one nobody can argue with."
    }
    fn no_reason_recorded(&self) -> &'static str {
        "No reason recorded for this module."
    }
    fn clean(&self) -> &'static str {
        "clean"
    }
    fn errors(&self, n: usize) -> String {
        format!("{n} {}", plural(n, "error", "errors"))
    }
    fn warnings(&self, n: usize) -> String {
        format!("{n} {}", plural(n, "warning", "warnings"))
    }
    fn files(&self, n: usize) -> String {
        format!("{n} {}", plural(n, "file", "files"))
    }
    fn rules(&self, n: usize) -> String {
        format!("{n} {}", plural(n, "rule", "rules"))
    }

    fn walls_eyebrow(&self) -> &'static str {
        "the walls"
    }
    fn walls_heading(&self) -> &'static str {
        "Who may import whom"
    }
    fn walls_lede(&self) -> &'static str {
        "Rows import, columns are imported. Hatching is a wall — that is the \
         design working, not a problem. A number is a wall being crossed right \
         now."
    }
    fn legend_allowed(&self) -> &'static str {
        "allowed"
    }
    fn legend_forbidden(&self) -> &'static str {
        "a wall — no rule permits this"
    }
    fn legend_crossed(&self) -> &'static str {
        "crossed now, with how many imports"
    }

    fn pressure_eyebrow(&self) -> &'static str {
        "where reality pushes back"
    }
    fn pressure_heading(&self) -> &'static str {
        "The walls under pressure"
    }
    fn pressure_lede(&self) -> &'static str {
        "Grouped by wall rather than by file, because a wall crossed eleven \
         times is a question about the wall."
    }
    fn holding(&self) -> &'static str {
        "holding"
    }
    fn crossing_now(&self, n: usize) -> String {
        format!("{n} crossing now")
    }
    fn nothing_crosses(&self) -> &'static str {
        "Nothing crosses this today."
    }
    fn imports(&self, n: usize) -> String {
        format!("{n} {}", plural(n, "import", "imports"))
    }

    fn blindspots_heading(&self) -> &'static str {
        "What this run did not decide"
    }
    fn blindspots_lede(&self) -> &'static str {
        "A page that hid these would be worse than the JSON, because it would \
         look more trustworthy while knowing less."
    }
    fn not_read(&self) -> &'static str {
        "Not read."
    }
    fn checks_nobody_could_make(&self, n: usize) -> String {
        format!("{n} {} nobody could make.", plural(n, "check", "checks"))
    }
    fn unresolved_imports(&self, n: usize) -> String {
        format!(
            "{n} {} could not be resolved, so no boundary rule saw them.",
            plural(n, "import", "imports")
        )
    }
    fn accepted_in_baseline(&self, n: usize) -> String {
        format!(
            "{n} {} accepted in the baseline and not counted above.",
            plural(n, "finding is", "findings are")
        )
    }

    fn read_only(&self) -> &'static str {
        "read-only"
    }
    fn regenerate_with(&self) -> &'static str {
        "regenerate with"
    }
    fn scanned(&self, files: usize, directories: usize) -> String {
        format!(
            "{files} {} · {directories} {}",
            plural(files, "file", "files"),
            plural(directories, "directory", "directories")
        )
    }
}
