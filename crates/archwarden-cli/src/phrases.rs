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

/// Brazilian Portuguese.
///
/// # A term of art is not translated
///
/// `boundary`, `check`, `import`, `finding`, `baseline` and `config` stay as
/// they are, because that is how the people reading this page talk. The first
/// draft said "parede" for a boundary — a metaphor that is *the page's own*, in
/// English, and reads as a literal wall in Portuguese. It also sat next to
/// `import-boundary`, the rule kind, printed in the same card: the reader had
/// to bridge two names for one thing.
///
/// The metaphor stays where it belongs, in the English copy. What crosses the
/// language is the concept, under the name the field already uses for it.
///
/// Ordinary words are still translated. `módulo`, `arquivo`, `erro`, `aviso`,
/// `regra` are not jargon in either language, and leaving them in English would
/// be a different affectation.
pub struct PtBr;

impl Phrases for PtBr {
    fn guide_title(&self) -> &'static str {
        "archwarden — a arquitetura como declarada"
    }
    fn guide_stamp(&self) -> &'static str {
        "archwarden · a arquitetura como declarada"
    }
    fn guide_heading(&self, rules: usize, modules: usize) -> String {
        format!(
            "{rules} {} em {modules} {}",
            plural(rules, "regra", "regras"),
            plural(modules, "módulo", "módulos")
        )
    }
    fn tally_rules(&self) -> &'static str {
        "regras"
    }
    fn tally_modules(&self) -> &'static str {
        "módulos"
    }
    fn tally_cross_module(&self) -> &'static str {
        "entre módulos"
    }
    fn tally_no_reason(&self) -> &'static str {
        "sem motivo"
    }
    fn rules_eyebrow(&self) -> &'static str {
        "as boundaries"
    }
    fn rules_heading(&self) -> &'static str {
        "Cada regra, e para que ela serve"
    }
    fn rules_lede(&self) -> &'static str {
        "Os requisitos são o que fazer; o motivo é o porquê. Uma regra cujo \
         motivo não está em lugar nenhum é uma que só dá para obedecer."
    }
    fn applies_to(&self, globs: &str) -> String {
        format!("aplica-se a {globs}")
    }
    fn guide_footer(&self) -> &'static str {
        "a arquitetura como declarada · como ela está agora precisa de"
    }

    fn report_title(&self) -> &'static str {
        "archwarden — a arquitetura como está"
    }
    fn report_stamp(&self) -> &'static str {
        "archwarden · a arquitetura como está"
    }
    fn report_heading(&self, modules: usize, walls: usize, crossed: usize) -> String {
        format!(
            "{modules} {}, {walls} {}, {crossed} sendo {}",
            plural(modules, "módulo", "módulos"),
            plural(walls, "boundary", "boundaries"),
            plural(crossed, "atravessada", "atravessadas")
        )
    }
    fn tally_files(&self) -> &'static str {
        "arquivos"
    }
    fn tally_errors(&self) -> &'static str {
        "erros agora"
    }
    fn tally_warnings(&self) -> &'static str {
        "avisos"
    }
    fn tally_accepted(&self) -> &'static str {
        "débito aceito"
    }
    fn tally_undecided(&self) -> &'static str {
        "não decidido"
    }

    fn map_eyebrow(&self) -> &'static str {
        "o mapa"
    }
    fn map_heading(&self) -> &'static str {
        "O que a config governa"
    }
    fn map_lede(&self) -> &'static str {
        "Cada módulo que as regras selecionam, com o motivo de ele existir. Um \
         módulo sem motivo registrado é um com que ninguém consegue discordar."
    }
    fn no_reason_recorded(&self) -> &'static str {
        "Nenhum motivo registrado para este módulo."
    }
    fn clean(&self) -> &'static str {
        "limpo"
    }
    fn errors(&self, n: usize) -> String {
        format!("{n} {}", plural(n, "erro", "erros"))
    }
    fn warnings(&self, n: usize) -> String {
        format!("{n} {}", plural(n, "aviso", "avisos"))
    }
    fn files(&self, n: usize) -> String {
        format!("{n} {}", plural(n, "arquivo", "arquivos"))
    }
    fn rules(&self, n: usize) -> String {
        format!("{n} {}", plural(n, "regra", "regras"))
    }

    fn walls_eyebrow(&self) -> &'static str {
        "as boundaries"
    }
    fn walls_heading(&self) -> &'static str {
        "Quem pode importar quem"
    }
    fn walls_lede(&self) -> &'static str {
        "As linhas importam, as colunas são importadas. Hachura é uma boundary \
         — isso é o desenho funcionando, não um problema. Um número é uma \
         boundary sendo atravessada agora."
    }
    fn legend_allowed(&self) -> &'static str {
        "permitido"
    }
    fn legend_forbidden(&self) -> &'static str {
        "uma boundary — nenhuma regra permite isto"
    }
    fn legend_crossed(&self) -> &'static str {
        "atravessada agora, com quantos imports"
    }

    fn pressure_eyebrow(&self) -> &'static str {
        "onde a realidade empurra"
    }
    fn pressure_heading(&self) -> &'static str {
        "As boundaries sob pressão"
    }
    fn pressure_lede(&self) -> &'static str {
        "Agrupado por boundary e não por arquivo, porque uma boundary \
         atravessada onze vezes é uma pergunta sobre a boundary."
    }
    fn holding(&self) -> &'static str {
        "segurando"
    }
    fn crossing_now(&self, n: usize) -> String {
        format!("{n} atravessando agora")
    }
    fn nothing_crosses(&self) -> &'static str {
        "Nada atravessa esta boundary hoje."
    }
    fn imports(&self, n: usize) -> String {
        format!("{n} imports")
    }

    fn blindspots_heading(&self) -> &'static str {
        "O que esta rodada não decidiu"
    }
    fn blindspots_lede(&self) -> &'static str {
        "Uma página que escondesse isto seria pior que o JSON, porque pareceria \
         mais confiável sabendo menos."
    }
    fn not_read(&self) -> &'static str {
        "Não lido."
    }
    fn checks_nobody_could_make(&self, n: usize) -> String {
        format!(
            "{n} {} que ninguém pôde fazer.",
            plural(n, "check", "checks")
        )
    }
    fn unresolved_imports(&self, n: usize) -> String {
        format!("{n} imports não resolveram, então nenhuma regra de boundary os viu.")
    }
    fn accepted_in_baseline(&self, n: usize) -> String {
        format!(
            "{n} {} aceitos no baseline e não contados acima.",
            plural(n, "finding está", "findings estão")
        )
    }

    fn read_only(&self) -> &'static str {
        "somente leitura"
    }
    fn regenerate_with(&self) -> &'static str {
        "regenere com"
    }
    fn scanned(&self, files: usize, directories: usize) -> String {
        format!(
            "{files} {} · {directories} {}",
            plural(files, "arquivo", "arquivos"),
            plural(directories, "diretório", "diretórios")
        )
    }
}

/// One or the other, by count.
///
/// Both languages happen to pluralise on "is it exactly one", which is why this
/// is shared. A language whose rule is different would not use it, and the
/// trait is where that difference would live.
fn plural(n: usize, one: &'static str, many: &'static str) -> &'static str {
    if n == 1 { one } else { many }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The page's own voice changes; a rule's identifier never does.
    #[test]
    fn the_pages_speak_the_language_they_were_asked_for() {
        assert_eq!(
            Language::En.phrases().map_heading(),
            "What the config governs"
        );
        assert_eq!(
            Language::PtBr.phrases().map_heading(),
            "O que a config governa"
        );
    }

    /// A browser needs the tag to hyphenate and to read the page aloud.
    #[test]
    fn each_language_carries_its_tag() {
        assert_eq!(Language::En.tag(), "en");
        assert_eq!(Language::PtBr.tag(), "pt-BR");
    }

    /// One is one in both languages, and the page counts things a reader will
    /// see as singular often enough for it to matter.
    #[test]
    fn counted_phrases_agree_with_their_number() {
        assert_eq!(Language::En.phrases().errors(1), "1 error");
        assert_eq!(Language::En.phrases().errors(3), "3 errors");
        assert_eq!(Language::PtBr.phrases().errors(1), "1 erro");
        assert_eq!(Language::PtBr.phrases().errors(3), "3 erros");
    }

    /// A term of art is not translated.
    ///
    /// The first draft said "parede" for a boundary — the page's own English
    /// metaphor, carried into a language where it reads as a literal wall, and
    /// printed beside `import-boundary` so the reader had two names for one
    /// thing. `fronteira` and `checagem` were the same mistake under other
    /// words.
    #[test]
    fn the_fields_own_words_survive_translation() {
        let say = Language::PtBr.phrases();
        let page = [
            say.walls_heading().to_owned(),
            say.walls_lede().to_owned(),
            say.pressure_heading().to_owned(),
            say.pressure_lede().to_owned(),
            say.legend_forbidden().to_owned(),
            say.nothing_crosses().to_owned(),
            say.unresolved_imports(4),
            say.checks_nobody_could_make(2),
            say.report_heading(5, 9, 4),
        ]
        .join(" ");

        for translated in ["parede", "fronteira", "checagem", "achado"] {
            assert!(
                !page.contains(translated),
                "`{translated}` is a term the field keeps in English: {page}"
            );
        }
        assert!(
            page.contains("boundary"),
            "and the word it does use: {page}"
        );
    }

    /// English is the default, and stays it whatever the machine's locale says.
    /// A report whose language depends on who ran it cannot be diffed.
    #[test]
    fn english_is_the_default() {
        assert_eq!(Language::default(), Language::En);
    }
}
