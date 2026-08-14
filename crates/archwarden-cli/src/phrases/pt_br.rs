//! The pages in Brazilian Portuguese.
//!
//! Excluded from the spell checker in `_typos.toml`: it is an English
//! dictionary, and on this file it produces nothing but false positives —
//! `erro` offered as `error`, `regenere` as `regenerate`.
//!
//! That exclusion is why this file holds its own tests. Assertions about this
//! translation are in the language they are about, so moving them into the
//! shared module would put Portuguese back under the checker.

use super::{Phrases, plural};

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
    fn tally_decisions(&self) -> &'static str {
        "decisões"
    }
    fn decisions_eyebrow(&self) -> &'static str {
        "o que foi decidido"
    }
    fn decisions_heading(&self) -> &'static str {
        "As decisões, e o que as sustenta"
    }
    fn decisions_lede(&self) -> &'static str {
        "Uma arquitetura é um conjunto de escolhas que alguém fez. As regras \
         abaixo são como cada uma se mantém; uma decisão que nada sustenta é \
         uma escolha que este repositório apenas descreve."
    }
    fn enforced_by(&self, rules: &str) -> String {
        format!("Sustentada por {rules}")
    }
    fn enforced_by_nothing(&self) -> &'static str {
        "Nada sustenta esta decisão."
    }
    fn written_down_in(&self) -> &'static str {
        "Escrita em"
    }
    /// The three status words, which are the only place this page translates a
    /// value that also appears in the JSON. The slug stays English there — it
    /// is an identifier — and the page is prose.
    fn decision_status(&self, status: &str) -> String {
        match status {
            "accepted" => "aceita",
            "proposed" => "proposta",
            "superseded" => "substituída",
            other => other,
        }
        .to_owned()
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

#[cfg(test)]
mod tests {
    use crate::phrases::Language;

    /// The page's own voice changes; a rule's identifier never does.
    #[test]
    fn the_pages_speak_the_language_they_were_asked_for() {
        assert_eq!(
            Language::PtBr.phrases().map_heading(),
            "O que a config governa"
        );
    }

    #[test]
    fn counted_phrases_agree_with_their_number() {
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
}
