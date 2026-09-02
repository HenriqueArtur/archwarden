//! Turning one config field into one compiled value.
//!
//! Every function here takes the rule it belongs to, so a refusal names it.

use archwarden_core::{
    facts::{ExportKind, ExportTags, KindFilter},
    glob::PathSet,
    ids::RuleId,
    pattern::Pattern,
    template,
};

use crate::rule::{MustExport, Rule, SpecPairRule};

use super::error::CompileError;

/// The spelling `must_export.kind` uses for "any declaration form".
const KIND_ANY: &str = "any";

/// Compiles a rule's import filter, when it has one.
///
/// Decision 25. Globs are matched against the resolved path, so they are built
/// the same way a boundary's are — the alternative would be a second glob
/// dialect for the same job, and two dialects eventually disagree.
/// The directive filter, when the rule asks for one.
///
/// `None` rather than an empty filter, on the same terms as
/// [`import_filter`]: "does not narrow" and "narrows to nothing" are different
/// statements. Issue #144.
pub(super) fn directive_filter(rule: &Rule) -> Option<archwarden_core::compiled::DirectiveFilter> {
    let (declaring, not_declaring) = rule.when_declaring();

    if declaring.is_empty() && not_declaring.is_empty() {
        return None;
    }

    Some(archwarden_core::compiled::DirectiveFilter {
        declaring: declaring.as_slice().to_vec(),
        not_declaring: not_declaring.as_slice().to_vec(),
    })
}

pub(super) fn import_filter(
    id: &RuleId,
    rule: &Rule,
) -> Result<Option<archwarden_core::compiled::ImportFilter>, CompileError> {
    let paths = rule.when_importing();
    let packages = rule.when_importing_packages();

    if paths.is_empty() && packages.is_empty() {
        return Ok(None);
    }

    Ok(Some(archwarden_core::compiled::ImportFilter {
        paths: archwarden_core::glob::PathSet::compile(paths.as_slice().iter().cloned()).map_err(
            |source| CompileError::Glob {
                rule: id.clone(),
                field: "when_importing",
                source,
            },
        )?,
        packages: packages.to_vec(),
    }))
}

/// A metadata key, refused if no comment could spell it.
///
/// Asked of the fact grammar itself rather than of a list of reserved words
/// kept here, so the two can never drift: whatever the suppression parser
/// accepts is exactly what this refuses.
pub(super) fn reachable_key(rule: &RuleId, key: &str) -> Result<String, CompileError> {
    if archwarden_core::facts::MetadataFact::key_is_reachable(key) {
        return Ok(key.to_owned());
    }

    Err(CompileError::UnreachableMetadataKey {
        rule: rule.clone(),
        key: key.to_owned(),
    })
}

/// The only group a document template may name.
///
/// A `naming` template renders from the capture groups of a `file_pattern`; a
/// document has one thing worth agreeing with, and it is the directory it sits
/// in. Refused rather than rendered empty, because a template naming a group
/// nobody defines is a rule that would quietly demand the wrong value.
const DOCUMENT_GROUP: &str = "dirname";

pub(super) fn check_document_template(rule: &RuleId, source: &str) -> Result<(), CompileError> {
    template::render(source, |group| {
        (group == DOCUMENT_GROUP).then(|| "placeholder".to_owned())
    })
    .map(|_| ())
    .map_err(|source| CompileError::Template {
        rule: rule.clone(),
        source,
    })
}

/// A `must_exist` path, refused if it is absolute or empty.
///
/// Relative, always: the file the rule is about is the anchor, and an absolute
/// path would make the rule say the same thing from every directory it covers
/// -- which is a `presence` rule scoped there, written the confusing way.
pub(super) fn companion(rule: &RuleId, path: &str) -> Result<String, CompileError> {
    let trimmed = path.trim();
    if trimmed.is_empty() || trimmed.starts_with('/') || trimmed.starts_with('\\') {
        return Err(CompileError::CompanionNotRelative {
            rule: rule.clone(),
            path: path.to_owned(),
        });
    }

    // Literal means literal, and saying so in the docs was not enough. The
    // template form is the one `naming.must_export` and `frontmatter.equals`
    // accept, so reaching for it here is the obvious mistake -- and it used to
    // compile, run, and report every governed file as missing a companion with
    // braces in its name. Sixteen confident findings about a file nothing could
    // create is worse than the rule not existing.
    if trimmed.contains("{{") {
        return Err(CompileError::CompanionIsATemplate {
            rule: rule.clone(),
            path: path.to_owned(),
        });
    }

    Ok(trimmed.to_owned())
}

/// A `require` entry, refused if it is a path rather than a name.
///
/// A rule answers for one directory's contract, which is what lets `describe`
/// answer for a directory that does not exist yet. An entry reaching into a
/// subdirectory would make one rule answer for two, and the same requirement
/// is already sayable by a second rule scoped one level down -- so this is a
/// redirection, not a limitation.
pub(super) fn require_name(rule: &RuleId, name: &str) -> Result<String, CompileError> {
    if name.contains('/') || name.contains('\\') {
        return Err(CompileError::RequireIsAPath {
            rule: rule.clone(),
            entry: name.to_owned(),
        });
    }

    Ok(name.to_owned())
}

pub(super) fn pattern(
    rule: &RuleId,
    field: &'static str,
    source: &str,
) -> Result<Pattern, CompileError> {
    Pattern::compile(source).map_err(|error| CompileError::Pattern {
        rule: rule.clone(),
        field,
        source: Box::new(error),
    })
}

pub(super) fn globs<'a, I>(
    rule: &RuleId,
    field: &'static str,
    patterns: I,
) -> Result<PathSet, CompileError>
where
    I: IntoIterator<Item = &'a String>,
{
    PathSet::compile(patterns).map_err(|source| CompileError::Glob {
        rule: rule.clone(),
        field,
        source,
    })
}

/// Validates the `spec-pair` markers.
///
/// A marker is one filename component -- `spec`, `test` -- and the extension
/// is taken from the source file. A marker carrying a dot or an extension is
/// almost always someone writing the old whole-suffix form, and guessing what
/// A `spec_dirs` entry, refused if it is a path rather than a directory name.
///
/// The rule reaches one level: a spec at `<dir>/<named>/x.spec.ts` counts and
/// `<dir>/<named>/unit/x.spec.ts` does not. An entry with a separator asks for
/// the second, and accepting it silently would make the rule reach further
/// than it says — which is how a `spec-pair` rule stops reporting and starts
/// looking like a repository that is fully tested.
pub(super) fn spec_dirs(rule: &RuleId, spec: &SpecPairRule) -> Result<Vec<String>, CompileError> {
    let mut names = Vec::new();
    for entry in &spec.spec_dirs {
        let trimmed = entry.trim();
        if trimmed.is_empty() || trimmed.contains('/') || trimmed.contains('\\') {
            return Err(CompileError::SpecDirIsAPath {
                rule: rule.clone(),
                entry: entry.clone(),
            });
        }
        names.push(trimmed.to_owned());
    }
    Ok(names)
}

/// A `spec_markers` entry, refused if any component of it is empty.
///
/// The leading dot is optional -- `.spec` and `spec` are the same marker, and
/// accepting both spares the author a refusal about punctuation. Every
/// component after that has to be a word, because a spec is
/// `<stem>.<marker>.<extension>` and an empty one asks for `user..ts`.
///
/// **A marker may hold a dot.** It used to be refused on the grounds that it
/// would change how many segments the name has and that guessing which
/// division was meant is worse than saying so. The first half is true and is
/// the point -- `unit.spec` is exactly the extra segment a repository
/// distinguishing `*.unit.spec.ts` from `*.intg.spec.ts` needs to name. The
/// second half was the mistake: nothing guesses, because matching is
/// `<stem>.<marker>` compared as written, not a search for a marker somewhere
/// in the name. Decision 38, issue #174.
///
/// What the old refusal was right to fear survives as
/// [`CompileError::SpecMarkerEndsAnother`].
pub(super) fn spec_markers(
    rule: &RuleId,
    spec: &SpecPairRule,
) -> Result<Vec<String>, CompileError> {
    let mut markers: Vec<String> = Vec::new();

    for marker in &spec.spec_markers {
        let trimmed = marker.trim_start_matches('.');
        if trimmed.is_empty() || trimmed.split('.').any(str::is_empty) {
            return Err(CompileError::InvalidSpecMarker {
                rule: rule.clone(),
                marker: marker.clone(),
            });
        }
        markers.push(trimmed.to_owned());
    }

    for (index, marker) in markers.iter().enumerate() {
        if let Some(shadowed) = markers
            .iter()
            .skip(index + 1)
            .find(|other| ends_with_marker(marker, other) || ends_with_marker(other, marker))
        {
            let (longer, shorter) = if marker.len() > shadowed.len() {
                (marker, shadowed)
            } else {
                (shadowed, marker)
            };
            // `ends_with_marker` held, so `longer` is `shorter` with at least
            // one component and the dot joining them in front.
            let prefix = longer
                .strip_suffix(&format!(".{shorter}"))
                .unwrap_or(longer)
                .to_owned();
            return Err(CompileError::SpecMarkerEndsAnother {
                rule: rule.clone(),
                longer: longer.clone(),
                shorter: shorter.clone(),
                prefix,
            });
        }
    }

    Ok(markers)
}

/// Whether `longer` is `shorter` with at least one component in front.
///
/// The relation that makes one spec file answer for two different units:
/// with `spec` and `unit.spec` both live, `a.unit.spec.ts` is the spec for
/// `a.ts` by the second marker and the spec for `a.unit.ts` by the first.
/// Equal markers are not this -- a repeated marker names one stem twice.
fn ends_with_marker(longer: &str, shorter: &str) -> bool {
    longer.len() > shorter.len() && longer.ends_with(&format!(".{shorter}"))
}

pub(super) fn export_kind(
    rule: &RuleId,
    must_export: &MustExport,
) -> Result<KindFilter, CompileError> {
    let mut tags = ExportTags::none();

    for name in &must_export.kind {
        if name == KIND_ANY {
            return Ok(KindFilter::Any);
        }

        let kind = ExportKind::parse(name).ok_or_else(|| CompileError::UnknownExportKind {
            rule: rule.clone(),
            name: name.clone(),
            available: ExportKind::ALL.map(ExportKind::as_str).join(", "),
        })?;

        tags = tags.with(kind);
    }

    Ok(KindFilter::OneOf(tags))
}

/// The forms that have somewhere to write a type down beside the name.
///
/// A binding takes an annotation after the colon; a class names its contracts
/// in `implements`. A function has a *return* type, an interface and a type
/// alias *are* the type, an enum declares one, and a re-export's declaration is
/// in another file — none of those is a place this rule could read.
const ANNOTATABLE: [ExportKind; 5] = [
    ExportKind::Const,
    ExportKind::Let,
    ExportKind::Var,
    ExportKind::Arrow,
    ExportKind::Class,
];

/// The required annotations, refusing a rule no file could satisfy.
///
/// `kind: "any"` passes: it accepts the annotatable forms among everything
/// else, so a file that satisfies the rule exists.
pub(super) fn annotation(
    rule: &RuleId,
    kind: &KindFilter,
    must_export: &MustExport,
) -> Result<Vec<String>, CompileError> {
    let Some(annotation) = must_export.annotation.as_ref() else {
        return Ok(Vec::new());
    };

    if !ANNOTATABLE
        .iter()
        .any(|form| kind.accepts(ExportTags::only(*form)))
    {
        return Err(CompileError::UnannotatableKind {
            rule: rule.clone(),
            kinds: must_export
                .kind
                .iter()
                .map(|name| format!("`{name}`"))
                .collect::<Vec<_>>()
                .join(", "),
        });
    }

    Ok(annotation.iter().cloned().collect())
}

/// Renders the export-name template against both patterns' capture groups.
///
/// A rule whose template names a group no pattern defines is a config bug that
/// would otherwise surface only when a file happened to match, which could be
/// months later or never.
pub(super) fn check_template(
    rule: &RuleId,
    file_pattern: &Pattern,
    dir_pattern: Option<&Pattern>,
    must_export: &MustExport,
) -> Result<(), CompileError> {
    let from_file = file_pattern.capture_names();
    let from_dir = dir_pattern.map(Pattern::capture_names).unwrap_or_default();

    // Refused rather than resolved by precedence. The two patterns share one
    // template namespace so that `{{pascal(entity)}}{{pascal(action)}}` reads
    // as one name rather than as two sources spliced together -- and the price
    // of that is that a group defined twice has no answer. Picking the
    // filename's silently would make the rule demand the wrong export on every
    // file in the scope, which is the state where a `naming` rule gets deleted
    // rather than fixed.
    if let Some(group) = from_file.iter().find(|group| from_dir.contains(group)) {
        return Err(CompileError::DuplicateCaptureGroup {
            rule: rule.clone(),
            group: (*group).to_owned(),
        });
    }

    let lookup = |group: &str| {
        (from_file.contains(&group) || from_dir.contains(&group))
            // The value is irrelevant: only whether the group exists is being
            // checked here.
            .then(|| "placeholder".to_owned())
    };

    let annotations = must_export
        .annotation
        .iter()
        .flat_map(|patterns| patterns.iter());

    for text in [Some(&must_export.name), must_export.signature_hint.as_ref()]
        .into_iter()
        .flatten()
        .chain(annotations)
    {
        template::render(text, lookup).map_err(|source| CompileError::Template {
            rule: rule.clone(),
            source,
        })?;
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::one_or_many::OneOrMany;
    use crate::rule::SpecPairRule;
    use archwarden_core::level::Level;

    fn id() -> RuleId {
        RuleId::new("r").expect("valid id")
    }

    /// The whole of `require`: a name survives unchanged, and that is the
    /// contract a `presence` rule is built on. Asserted here rather than only
    /// through a refusal test, because a function that accepted a name and
    /// returned something else would pass a test that only checks what it
    /// refuses.
    #[test]
    fn a_require_entry_is_a_name_and_survives_as_one() {
        assert_eq!(require_name(&id(), "DOC.md").expect("a name"), "DOC.md");
        assert_eq!(
            require_name(&id(), "README").expect("a name"),
            "README",
            "an extension is not required of it"
        );
    }

    /// Both separators, each on its own: a rule answers for one directory, and
    /// an entry reaching into a subdirectory would make it answer for two.
    ///
    /// Windows spells it `\\` and the config is written by hand on both, so
    /// neither separator may be the only one checked -- a test naming just one
    /// passes while the other silently reaches further than the rule says.
    #[test]
    fn a_require_entry_that_reaches_into_a_subdirectory_is_refused() {
        assert!(
            require_name(&id(), "docs/DOC.md").is_err(),
            "the posix separator"
        );
        assert!(
            require_name(&id(), "docs\\DOC.md").is_err(),
            "and the windows one"
        );
    }

    fn spec_with_markers(markers: &[&str]) -> SpecPairRule {
        SpecPairRule {
            id: id(),
            level: Level::Error,
            why: None,
            not_yet: None,
            decision: None,
            roots: OneOrMany::One("src/*".to_owned()),
            subfolders: OneOrMany::One(".".to_owned()),
            spec_markers: OneOrMany::Many(markers.iter().map(|m| (*m).to_owned()).collect()),
            spec_dirs: OneOrMany::Many(Vec::new()),
            ignore_files: OneOrMany::Many(Vec::new()),
            require_non_empty_spec: false,
            skip_type_only: false,
            when_importing: OneOrMany::Many(Vec::new()),
            when_importing_packages: Vec::new(),
        }
    }

    /// The marker the author wrote is the marker the rule gets, with the
    /// optional leading dot removed. `.spec` and `spec` are one marker.
    #[test]
    fn a_spec_marker_keeps_its_word_and_loses_its_leading_dots() {
        assert_eq!(
            spec_markers(&id(), &spec_with_markers(&["spec", "test"])).expect("valid"),
            vec!["spec".to_owned(), "test".to_owned()],
        );
        assert_eq!(
            spec_markers(&id(), &spec_with_markers(&[".spec"])).expect("valid"),
            vec!["spec".to_owned()],
            "the leading dot is optional and is not part of the marker"
        );
        assert!(
            spec_markers(&id(), &spec_with_markers(&[]))
                .expect("valid")
                .is_empty(),
            "naming none is not an error here; the default is applied elsewhere"
        );
    }

    /// A marker may name more than one component.
    ///
    /// `*.unit.spec.ts` beside `*.intg.spec.ts` is how a repository says what
    /// infrastructure a test needs, and a rule that cannot see the first as a
    /// spec for `account-sanitize.ts` reports every file in its scope. Issue
    /// #174, decision 38.
    #[test]
    fn a_spec_marker_may_name_more_than_one_component() {
        assert_eq!(
            spec_markers(&id(), &spec_with_markers(&["unit.spec"])).expect("valid"),
            vec!["unit.spec".to_owned()],
        );
        assert_eq!(
            spec_markers(&id(), &spec_with_markers(&[".unit.spec"])).expect("valid"),
            vec!["unit.spec".to_owned()],
            "the leading dot is still optional, and still not part of the marker"
        );
    }

    /// An empty marker, and an empty component inside one.
    ///
    /// A spec is `<stem>.<marker>.<extension>`, so an empty marker asks for
    /// `user..ts` and a marker with an empty component asks for
    /// `user.unit..ts`. Neither is a filename anybody writes, and a rule
    /// matching none reports nothing — which is indistinguishable from a
    /// repository that satisfies it.
    #[test]
    fn a_spec_marker_with_an_empty_component_is_refused() {
        assert!(
            spec_markers(&id(), &spec_with_markers(&["."])).is_err(),
            "a lone dot trims to nothing"
        );
        assert!(
            spec_markers(&id(), &spec_with_markers(&[""])).is_err(),
            "and so does an empty string"
        );
        assert!(
            spec_markers(&id(), &spec_with_markers(&["unit."])).is_err(),
            "a trailing dot is an empty component, and asks for `user.unit..ts`"
        );
        assert!(
            spec_markers(&id(), &spec_with_markers(&["unit..spec"])).is_err(),
            "and so is one in the middle"
        );
    }

    /// Two markers where one ends the other are refused together.
    ///
    /// With `spec` and `unit.spec` both live, `a.unit.spec.ts` is a spec for
    /// `a.ts` *and* a spec for `a.unit.ts`. One file satisfying two units is
    /// the shape where a gate reports nothing and looks like a repository that
    /// is fully tested. Refused where both can be named, rather than resolved
    /// by picking one — guessing which they meant is worse than saying so.
    #[test]
    fn two_markers_where_one_ends_the_other_are_refused() {
        assert!(
            spec_markers(&id(), &spec_with_markers(&["spec", "unit.spec"])).is_err(),
            "`unit.spec` ends with `.spec`"
        );
        assert!(
            spec_markers(&id(), &spec_with_markers(&["unit.spec", "spec"])).is_err(),
            "and the order it is written in does not change that"
        );
        assert!(
            spec_markers(&id(), &spec_with_markers(&["unit.spec", "intg.spec"]))
                .expect("valid")
                .len()
                == 2,
            "two markers of the same depth shadow nothing"
        );
        assert!(
            spec_markers(&id(), &spec_with_markers(&["spec", "test"]))
                .expect("valid")
                .len()
                == 2,
            "and neither does the default pair"
        );
        assert!(
            spec_markers(&id(), &spec_with_markers(&["spec", "spec"])).is_ok(),
            "a repeated marker names one stem, not two -- harmless, and not \
             this refusal's business"
        );
    }
}
