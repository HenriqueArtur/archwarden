//! What the pages say, in each language they say it in.
//!
//! Only the pages. The terminal, the JSON and the markdown digest stay in
//! English whatever this is set to, and that is a decision rather than an
//! omission: a CI log is pasted into an issue, searched for, and *read by an
//! agent* — `AGENTS.md` teaches one to read that output — and a log whose
//! language depends on who ran it is worse than a log in a language somebody
//! has to translate. The JSON is a contract and its slugs are stable
//! identifiers, so it was never in question.
//!
//! # Why a trait rather than a table
//!
//! The compiler. A `Phrases` implementation with a method missing does not
//! build, so a page cannot grow a heading that exists in one language only —
//! which is exactly how a half-translated interface happens, and it is the same
//! property `engines_for`'s exhaustive match gives a rule kind added without an
//! engine.
//!
//! Adding a language is one file, and the compiler lists everything it needs.
//!
//! # What is not here yet
//!
//! The sentences a rule produces — `describe_observed`, and the requirement
//! lines `agent-guide` renders — are written once and shown in the terminal, in
//! the digest and on a page. Translating those means giving each of those
//! renderers a language, and it is translation work rather than engineering.
//! Until then a page reads as Portuguese with English technical sentences,
//! which is honest about what has been done.

mod en;
mod pt_br;

pub use en::En;
pub use pt_br::PtBr;

/// A language the pages are written in.
///
/// Never detected from the environment. A report whose language depends on the
/// machine that produced it cannot be diffed, and the guide page is meant to be
/// committable.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, clap::ValueEnum)]
pub enum Language {
    /// English.
    #[default]
    En,
    /// Brazilian Portuguese.
    #[value(name = "pt-br")]
    PtBr,
}

impl Language {
    /// The language a configuration asked for, if it asked.
    ///
    /// The flag wins where both are present: a config is what a repository
    /// decided and a flag is what this one run wants.
    #[must_use]
    pub fn of(config: Option<archwarden_config::config::PageLanguage>) -> Self {
        match config {
            Some(archwarden_config::config::PageLanguage::PtBr) => Self::PtBr,
            _ => Self::En,
        }
    }

    /// The phrases this language says.
    #[must_use]
    pub fn phrases(self) -> &'static dyn Phrases {
        match self {
            Self::En => &En,
            Self::PtBr => &PtBr,
        }
    }

    /// The `lang` attribute a browser needs to hyphenate and to read aloud.
    #[must_use]
    pub fn tag(self) -> &'static str {
        match self {
            Self::En => "en",
            Self::PtBr => "pt-BR",
        }
    }
}

/// Everything the pages say in their own voice.
///
/// Not what a *rule* says — those sentences belong to the rule and are the same
/// in every rendering. These are the page's headings, its labels and the
/// sentences that explain what the reader is looking at.
pub trait Phrases: Send + Sync {
    // --- the guide page ---------------------------------------------------
    /// The browser tab, on the guide page.
    fn guide_title(&self) -> &'static str;
    /// The line above the guide page's heading.
    fn guide_stamp(&self) -> &'static str;
    /// `12 rules across 5 modules`.
    fn guide_heading(&self, rules: usize, modules: usize) -> String;
    /// The label under the rule count.
    fn tally_rules(&self) -> &'static str;
    /// The label under the module count.
    fn tally_modules(&self) -> &'static str;
    /// The label for rules that belong to no module.
    fn tally_cross_module(&self) -> &'static str;
    /// The label for rules that record no reason.
    fn tally_no_reason(&self) -> &'static str;
    /// The eyebrow above the rule list.
    fn rules_eyebrow(&self) -> &'static str;
    /// The heading above the rule list.
    fn rules_heading(&self) -> &'static str;
    /// What the rule list is for.
    fn rules_lede(&self) -> &'static str;
    /// The line naming the globs a rule governs.
    fn applies_to(&self, globs: &str) -> String;
    /// The guide page's footer, before the command that follows it.
    fn guide_footer(&self) -> &'static str;

    // --- the check page ---------------------------------------------------
    /// The browser tab, on the report page.
    fn report_title(&self) -> &'static str;
    /// The line above the report page's heading.
    fn report_stamp(&self) -> &'static str;
    /// `5 modules, 9 walls, 4 of them being crossed`.
    fn report_heading(&self, modules: usize, walls: usize, crossed: usize) -> String;
    /// The label under the file count.
    fn tally_files(&self) -> &'static str;
    /// The label under the error count.
    fn tally_errors(&self) -> &'static str;
    /// The label under the warning count.
    fn tally_warnings(&self) -> &'static str;
    /// The label under the accepted-debt count.
    fn tally_accepted(&self) -> &'static str;
    /// The label under the count of checks nobody could make.
    fn tally_undecided(&self) -> &'static str;

    // --- the map ----------------------------------------------------------
    /// The eyebrow above the module map.
    fn map_eyebrow(&self) -> &'static str;
    /// The heading above the module map.
    fn map_heading(&self) -> &'static str;
    /// What the module map is for.
    fn map_lede(&self) -> &'static str;
    /// Said in place of a module's reason when it has none.
    fn no_reason_recorded(&self) -> &'static str;
    /// A module with nothing reported against it.
    fn clean(&self) -> &'static str;
    /// `3 errors`.
    fn errors(&self, n: usize) -> String;
    /// `1 warning`.
    fn warnings(&self, n: usize) -> String;
    /// `412 files`.
    fn files(&self, n: usize) -> String;
    /// `3 rules`.
    fn rules(&self, n: usize) -> String;

    // --- the grid ---------------------------------------------------------
    /// The eyebrow above the grid.
    fn walls_eyebrow(&self) -> &'static str;
    /// The heading above the grid.
    fn walls_heading(&self) -> &'static str;
    /// How to read the grid.
    fn walls_lede(&self) -> &'static str;
    /// The legend entry for a cell nothing forbids.
    fn legend_allowed(&self) -> &'static str;
    /// The legend entry for a wall.
    fn legend_forbidden(&self) -> &'static str;
    /// The legend entry for a wall being crossed.
    fn legend_crossed(&self) -> &'static str;

    // --- the pressure -----------------------------------------------------
    /// The eyebrow above the walls under pressure.
    fn pressure_eyebrow(&self) -> &'static str;
    /// The heading above the walls under pressure.
    fn pressure_heading(&self) -> &'static str;
    /// Why that section is grouped by wall.
    fn pressure_lede(&self) -> &'static str;
    /// A wall nothing is crossing.
    fn holding(&self) -> &'static str;
    /// `3 crossing now`.
    fn crossing_now(&self, n: usize) -> String;
    /// Said in place of a crossing list when there is none.
    fn nothing_crosses(&self) -> &'static str;
    /// The summary line of a folded crossing list.
    fn imports(&self, n: usize) -> String;

    // --- the blind spots --------------------------------------------------
    /// The heading above what the run could not decide.
    fn blindspots_heading(&self) -> &'static str;
    /// Why that section is not hidden.
    fn blindspots_lede(&self) -> &'static str;
    /// Introduces a file nobody could read.
    fn not_read(&self) -> &'static str;
    /// `2 checks nobody could make.`
    fn checks_nobody_could_make(&self, n: usize) -> String;
    /// `4 imports could not be resolved…`
    fn unresolved_imports(&self, n: usize) -> String;
    /// `13 findings are accepted in the baseline…`
    fn accepted_in_baseline(&self, n: usize) -> String;

    // --- the footer -------------------------------------------------------
    /// Said in the footer: the page decides nothing.
    fn read_only(&self) -> &'static str;
    /// Introduces the command that rewrites the page.
    fn regenerate_with(&self) -> &'static str;
    /// `4 187 files · 1 268 directories`.
    fn scanned(&self, files: usize, directories: usize) -> String;
}

/// One or the other, by count.
///
/// Both languages happen to pluralise on "is it exactly one", which is why this
/// is shared. A language whose rule is different would not use it, and the
/// trait is where that difference would live.
pub(super) fn plural(n: usize, one: &'static str, many: &'static str) -> &'static str {
    if n == 1 { one } else { many }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A browser needs the tag to hyphenate and to read the page aloud.
    #[test]
    fn each_language_carries_its_tag() {
        assert_eq!(Language::En.tag(), "en");
        assert_eq!(Language::PtBr.tag(), "pt-BR");
    }

    /// English is the default, and stays it whatever the machine's locale says.
    /// A report whose language depends on who ran it cannot be diffed.
    #[test]
    fn english_is_the_default() {
        assert_eq!(Language::default(), Language::En);
    }

    /// One is one, and the pages count things a reader sees as singular often
    /// enough for it to matter. The other language asserts its own half, in its
    /// own file.
    #[test]
    fn counted_phrases_agree_with_their_number() {
        assert_eq!(Language::En.phrases().errors(1), "1 error");
        assert_eq!(Language::En.phrases().errors(3), "3 errors");
    }
}
