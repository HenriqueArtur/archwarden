//! Saying, in words, what the rules are about — a finding, or a path.
//!
//! Two operations that share a name because they share a job. [`describe`]
//! answers *what applies to this path*, which is the question an agent asks
//! before it writes; [`describe_observed`] answers *what was found*, which is
//! the sentence every surface says after. Both moved here for the same reason,
//! one release apart: a surface that assembled either for itself would be a
//! second implementation of a contract.
//!
//! # One sentence for what a rule found
//!
//! Shared by four surfaces and one committed file format, which is why it
//! sits at the boundary rather than in a renderer. `check` prints it under a
//! finding, the pre-write hook says it when it denies a write, `config
//! explain` shows it beside a rule, and `baseline` *writes it into
//! `arch.baseline.json`* as the `note` on every accepted entry.
//!
//! That last one is the argument. A sentence a committed file carries is not
//! terminal output — it is part of a format, and a format belongs where the
//! operations are. The alternative was for the baseline to reach back into
//! the CLI for its own file's contents, which is a dependency pointing the
//! wrong way.
//!
//! Generated from the same [`Observed`] value the JSON report carries, so the
//! prose and the machine-readable form can never describe a finding
//! differently.
//!
//! # What applies to a path
//!
//! The informant half of decision 9. `check` tells an agent what it got wrong
//! after the fact; [`describe`] tells it what the rules are while there is
//! still time to follow them, for a path that need not exist yet.
//!
//! It reads no file and parses nothing. Every rule's `describe_expectation` is
//! purely lexical by contract, which is what makes this answerable about a
//! file nobody has created.
//!
//! It lived in `archwarden-cli` until 0.18, which was tolerable while the CLI
//! was the only surface asking. MCP asks the same question, and an MCP server
//! reaching into the CLI for the answer is the dependency pointing backwards
//! that this crate exists to refuse.

use archwarden_core::{facts::ExportKind, finding::Observed};

/// One sentence for what was found.
///
/// Shared with the hook and with the baseline file, so a blocked write, a
/// failing `check` and an accepted entry describe the same problem in the
/// same words.
#[allow(
    clippy::too_many_lines,
    reason = "one arm per observation, each a sentence. Splitting it by category \
              would scatter prose that has to read consistently -- the wording \
              of two findings side by side in one report is the thing being \
              maintained here, and it is only reviewable in one place"
)]
#[must_use]
pub fn describe_observed(observed: &Observed) -> String {
    match observed {
        Observed::UnexpectedSubfolder { name } => {
            format!("folder `{name}` is not allowed here")
        }
        Observed::DiscouragedSubfolder { name } => {
            format!("folder `{name}` is allowed for now, as documented debt")
        }
        Observed::UnexpectedFilename { name } => {
            format!("filename `{name}` matches none of the allowed patterns")
        }
        Observed::ExportMissing { name } => format!("no export named `{name}`"),
        Observed::ExportWrongKind { name, found } => {
            let kinds: Vec<_> = found.iter().map(ExportKind::as_str).collect();
            format!("`{name}` is declared as {}", join_or(&kinds, "nothing"))
        }
        // "declares no type of its own" rather than "has no annotation": the
        // reader's next action is to write one, and the sentence that names
        // the absence names the fix.
        Observed::ExportMissingAnnotation { name } => {
            format!("`{name}` declares no type of its own")
        }
        Observed::ExportWrongAnnotation { name, found } => {
            let written: Vec<&str> = found.iter().map(String::as_str).collect();
            format!("`{name}` is declared as {}", join_or(&written, "nothing"))
        }
        Observed::OnlyDefaultExport => {
            "the only export is a default, whose name does not bind importers".to_owned()
        }
        Observed::ReexportOfUnknownKind { name, from } => {
            format!("`{name}` is re-exported from `{from}`, so its kind is not determinable here")
        }
        Observed::Passthrough {
            exports,
            whole_file,
        } => {
            let names = exports
                .iter()
                .map(|name| format!("`{name}`"))
                .collect::<Vec<_>>()
                .join(", ");
            let forwards = if exports.len() == 1 {
                "only forwards"
            } else {
                "only forward"
            };
            if *whole_file {
                format!("adds nothing of its own: {names} {forwards} another module")
            } else {
                // A different sentence, because it is a different decision:
                // the file is real and part of it is an indirection.
                format!("{names} {forwards} another module; the rest of the file is its own")
            }
        }
        // "is not here" rather than "does not exist": the finding is on the
        // directory, and what the reader has to do is create the file *in it*.
        Observed::RequiredFileMissing { name } => format!("`{name}` is not here"),
        Observed::NoFileMatching { pattern } => format!("no file here matches `{pattern}`"),
        Observed::FrontmatterAbsent => "has no frontmatter block".to_owned(),
        Observed::FrontmatterMalformed { reason } => {
            format!("its frontmatter block is not YAML: {reason}")
        }
        Observed::FrontmatterKeyMissing { key } => {
            format!("its frontmatter carries no `{key}`")
        }
        // The value is quoted back rather than merely called wrong: a
        // vocabulary miss is almost always a spelling, and seeing the spelling
        // is the fix.
        Observed::FrontmatterValueOutsideVocabulary { key, found } => {
            format!("`{key}` is `{found}`, which is not one of the accepted values")
        }
        Observed::FrontmatterValueDisagrees { key, found, wanted } => {
            format!("`{key}` is `{found}`, and the path says `{wanted}`")
        }
        Observed::FrontmatterValueNotScalar { key } => {
            format!("`{key}` is not a single value, so there is nothing to compare")
        }
        Observed::CompanionMissing { path } | Observed::SiblingMissing { path } => {
            format!("`{path}` does not exist")
        }
        Observed::SpecIsEmpty { path } => format!("`{path}` contains no test cases"),
        Observed::ForbiddenImport {
            specifier,
            resolved,
        } => format!("imports `{specifier}`, which resolves to `{resolved}`"),
        Observed::ForbiddenPackageImport { specifier, package } => {
            // Named separately only when they differ, because for a deep import
            // they do and reading "imports `three/examples/jsm/loaders/
            // GLTFLoader.js`" without being told the rule is about `three`
            // leaves the reader to work out which package they hit.
            //
            // `node:` is stripped from both first: `fs` is not *part of*
            // `node:fs`, it is the same module spelled the other way, and
            // saying otherwise reads as a bug in the rule.
            let bare = |name: &str| name.strip_prefix("node:").unwrap_or(name).to_owned();
            if bare(specifier) == bare(package) {
                format!("imports the package `{package}`")
            } else {
                format!("imports `{specifier}`, which is part of the package `{package}`")
            }
        }
        // "is not on the list" rather than "is forbidden": under an allowlist
        // nothing is forbidden by name, and a reader told their import is
        // banned would go looking for the ban.
        Observed::ImportNotPermitted {
            specifier,
            resolved,
        } => format!(
            "imports `{specifier}`, which resolves to `{resolved}` and is not on this \
             rule's list"
        ),
        Observed::PackageNotPermitted { specifier, package } => {
            if specifier == package {
                format!("imports the package `{package}`, which is not on this rule's list")
            } else {
                format!(
                    "imports `{specifier}`, which is part of the package `{package}` and is \
                     not on this rule's list"
                )
            }
        }
        Observed::RequiredImportMissing => "no import satisfies the requirement".to_owned(),
        Observed::RequiredCallMissing { symbol } => {
            format!("`{symbol}` is imported but never called")
        }
        Observed::RequiredImportForCallMissing { symbol, module } => {
            format!("`{symbol}` is not imported from `{module}`")
        }
        // The destination first, because that is the rule that was broken, and
        // the chain after it, because that is where the edit goes. A reader
        // given only the destination opens this file and finds no such import.
        Observed::ForbiddenReach { chain } => match chain.split_last() {
            None => "ends up depending on something the rule forbids".to_owned(),
            Some((last, _)) => {
                let steps: Vec<String> = chain.iter().map(|step| format!("`{step}`")).collect();
                format!(
                    "ends up depending on `{last}`, through {}",
                    steps.join(" → ")
                )
            }
        },
        // "no rule governs it" rather than "it is not governed": the reader's
        // next action is to write a rule or to ignore the file deliberately,
        // and naming the absent thing is what points at both.
        Observed::Ungoverned => "no rule governs it".to_owned(),
        // The chain, not the fact. "is in a cycle" tells a reader they have a
        // problem and not where it is; the arrows name every edge that could
        // be cut, and the repeated first entry is what shows the loop closed.
        Observed::ImportCycle { chain } if chain.is_empty() => "sits on an import cycle".to_owned(),
        Observed::ImportCycle { chain } => {
            let steps: Vec<String> = chain.iter().map(|step| format!("`{step}`")).collect();
            format!("sits on an import cycle: {}", steps.join(" → "))
        }
        // `Observed` is non_exhaustive; a variant added later says what it is
        // rather than failing to compile here.
        other => format!("{other:?}"),
    }
}

/// `a`, `b` or `c` — the list form the expectations are written in.
///
/// Public because the text renderer builds twenty other sentences with it,
/// and two copies of a comma rule is two copies that drift.
#[must_use]
pub fn join_or(items: &[impl AsRef<str>], empty: &str) -> String {
    let quoted: Vec<String> = items
        .iter()
        .map(|item| format!("`{}`", item.as_ref()))
        .collect();

    match quoted.split_last() {
        None => empty.to_owned(),
        Some((last, [])) => last.clone(),
        Some((last, rest)) => format!("{} or {last}", rest.join(", ")),
    }
}

/// The version of the `describe` JSON shape.
///
/// Separate from the report's version: an agent consuming one may never read
/// the other, and coupling them would force a bump on consumers of a contract
/// that did not change.
pub const DESCRIBE_VERSION: u32 = 0;

/// One rule that has something to say about a path.
pub struct Applies<'a> {
    /// The rule itself, for its id, kind, level and module.
    pub rule: &'a archwarden_core::compiled::CompiledRule,
    /// What it requires of this path. Never empty -- a rule with nothing to
    /// say is not in the list.
    pub expectations: Vec<archwarden_core::finding::Expectation>,
}

/// Every rule that applies to `path`, in configuration order.
///
/// An ignored path yields nothing, which is the same answer `check` gives: an
/// `ignore` entry wins over any rule's scope.
#[must_use]
pub fn describe<'a>(
    config: &'a archwarden_core::compiled::CompiledConfig,
    path: &archwarden_core::path::RepoRelPath,
) -> Vec<Applies<'a>> {
    if config.is_ignored(path) {
        return Vec::new();
    }

    config
        .rules()
        .zip(archwarden_rules::engines_for(config))
        .filter_map(|(rule, engine)| {
            let expectations = engine.describe_expectation(path);
            (!expectations.is_empty()).then_some(Applies { rule, expectations })
        })
        .collect()
}

/// Turns a path as typed on the command line into a repository-relative one.
///
/// # Two readings of one relative path
///
/// Standing in `packages/domain`, `src/order/x.ts` means the file under
/// here — that is what `git diff` and an editor hand a developer. But every
/// path archwarden *prints* is repository-relative, so the one an agent copies
/// out of a report is `packages/domain/src/order/x.ts`, and reading that
/// against the working directory gives `packages/domain/packages/domain/...`.
///
/// That did not fail. It resolved to a path no rule selects and answered "no
/// rule applies", which reads exactly like "nothing constrains this file" —
/// the wrong answer that looks like good news.
///
/// So both readings are tried, in this order:
///
/// 1. Against the working directory, when that names something on disk. It is
///    the reading a developer means, and it wins whenever both are real.
/// 2. Against the repository root, when *that* names something on disk.
/// 3. Otherwise, whichever the argument's own shape indicates: a path already
///    beginning with where the user is standing is repository-relative, since
///    nobody nests `packages/domain` inside `packages/domain`.
/// 4. Failing all of that, against the working directory, as before.
///
/// Steps 1 and 2 touch the filesystem, which this function used to avoid.
/// Existence is the only evidence available about which reading was meant, and
/// steps 3 and 4 are what keep `describe` and `scaffold` answering about files
/// that do not exist yet — which is most of what they are for.
///
/// From the repository root both readings are the same question, so none of
/// this costs the common case anything.
///
/// # Errors
/// A message naming the path, when it falls outside the repository.
pub fn repo_relative(
    root: &camino::Utf8Path,
    working_directory: &camino::Utf8Path,
    seen_as: Option<&camino::Utf8Path>,
    argument: &str,
) -> Result<archwarden_core::path::RepoRelPath, String> {
    let raw = camino::Utf8Path::new(argument);

    let relative = if raw.is_absolute() {
        raw.strip_prefix(root)
            .map(camino::Utf8Path::to_string)
            .or_else(|_| {
                same_directory_by_another_name(root, raw)
                    .or_else(|| re_rooted(root, seen_as, raw))
                    .ok_or_else(|| outside(argument, root, seen_as))
            })?
    } else {
        let inside = working_directory.strip_prefix(root).map_err(|_| {
            format!("the working directory `{working_directory}` is outside `{root}`")
        })?;

        let here = inside.join(raw).to_string();
        if inside.as_str().is_empty() {
            here
        } else {
            disambiguate(root, inside, raw, here)
        }
    };

    archwarden_core::path::RepoRelPath::new(&relative)
        .map_err(|error| format!("`{argument}` is not a path inside the repository: {error}"))
}

/// The same path, when the text says otherwise and the filesystem disagrees.
///
/// A repository has more than one absolute path more often than it looks: a
/// symlinked checkout, a bind-mounted worktree, `/tmp` → `/private/tmp` on
/// macOS, a container whose mount path differs from the host's. A harness hands
/// over whichever spelling its own `cwd` resolved to, and comparing the two as
/// text says "outside the repository" about a file plainly inside it.
///
/// # Why the parent and not the whole path
///
/// A pre-write hook is asked *before* the write, so the file it names usually
/// does not exist and `canonicalize` on it would fail — the case this most
/// needs to work. The parent directory does exist, so that is what gets
/// resolved, and the file name is put back afterwards.
///
/// It also keeps a change nobody asked for from creeping in. Resolving the
/// whole path would follow a symlinked *file* to wherever it points, so a link
/// inside the repository aimed outside it would start being refused. That may
/// even be right, but it is a different question from this one and it should be
/// asked on its own.
///
/// Returns `None` when the path is genuinely somewhere else, which is still
/// most of the times this is reached.
fn same_directory_by_another_name(
    root: &camino::Utf8Path,
    raw: &camino::Utf8Path,
) -> Option<String> {
    let name = raw.file_name()?;
    let parent = raw.parent()?;

    let real_root = std::fs::canonicalize(root).ok()?;
    let real_parent = std::fs::canonicalize(parent).ok()?;

    let inside = real_parent.strip_prefix(&real_root).ok()?;

    let relative = camino::Utf8Path::from_path(inside)?.join(name);
    Some(relative.to_string())
}

/// The same file, named from where the caller stands.
///
/// A harness on the host says `/home/dev/proj/src/x.ts`; an archwarden inside a
/// container has `/app` as its root. Both mean one file, and until 0.19 the
/// second answered *outside the repository* about it — correctly, and
/// uselessly. Issue #93.
///
/// `seen_as` is where the caller thinks the repository is, and it is *derived*
/// rather than configured: every hook payload carries `cwd`, and an MCP client
/// answers `roots/list`. The one thing a shared config file could not carry is
/// this, because the host root differs per developer.
///
/// # Why an ancestor has to exist
///
/// Because otherwise this turns a loud, useless failure into a quiet, wrong
/// one. A wrapper pointed at a container holding a *different* project would
/// have its paths rewritten into ours and judged against our rules, and the
/// answer would be an approval nobody could question.
///
/// Existence is the evidence available exactly when the two roots really are
/// one repository through two mounts: the code is mounted, so the directories
/// are there. Requiring the *whole* path to exist would be too strict by the
/// case that matters — `describe`, `scaffold` and the pre-write hook are all
/// asked about files that do not exist yet — so the test is that some ancestor
/// of the result does.
///
/// Decision 24.
fn re_rooted(
    root: &camino::Utf8Path,
    seen_as: Option<&camino::Utf8Path>,
    raw: &camino::Utf8Path,
) -> Option<String> {
    let inside = raw.strip_prefix(seen_as?).ok()?;
    let here = root.join(inside);

    // `RepoRelPath` refuses anything that escapes, and this refuses anything
    // with nothing under it. Together they are what keeps a translation from
    // being a guess.
    has_an_existing_ancestor(root, &here).then(|| inside.to_string())
}

/// Whether any directory on the way to `candidate` is really there.
///
/// Stops at `root`: the root itself existing says only that archwarden is
/// somewhere, which is true in every case including the wrong one.
fn has_an_existing_ancestor(root: &camino::Utf8Path, candidate: &camino::Utf8Path) -> bool {
    candidate
        .ancestors()
        .skip(1)
        .take_while(|ancestor| ancestor.as_str().len() > root.as_str().len())
        .any(camino::Utf8Path::exists)
}

/// The sentence for a path that is inside nothing this can reach.
///
/// Names **both** roots when there are two. "Outside the repository" about a
/// path the caller believes is inside it is a sentence that sends a reader
/// nowhere, and the two roots side by side are the whole diagnosis.
fn outside(argument: &str, root: &camino::Utf8Path, seen_as: Option<&camino::Utf8Path>) -> String {
    match seen_as {
        Some(seen_as) if seen_as != root => format!(
            "`{argument}` is outside the repository at `{root}`, and outside \
             `{seen_as}`, which is where the caller says the repository is"
        ),
        _ => format!("`{argument}` is outside the repository at `{root}`"),
    }
}

/// Picks between the two readings. See [`repo_relative`].
fn disambiguate(
    root: &camino::Utf8Path,
    inside: &camino::Utf8Path,
    raw: &camino::Utf8Path,
    here: String,
) -> String {
    let there = raw.to_string();

    if root.join(&here).exists() {
        return here;
    }
    if root.join(&there).exists() {
        return there;
    }
    // Nothing on disk to go by, which is the case `describe` exists for. A
    // path that already carries the way here was written from the root.
    if raw.starts_with(inside) {
        return there;
    }
    here
}

/// The JSON envelope for one path.
///
/// Public, and built here rather than in a renderer, for the reason
/// [`crate::render`] gives about the report: a shape a program consumes is a
/// contract, and MCP has to emit the one `describe --format json` does. A
/// server assembling its own would be a second implementation of the contract.
#[derive(Debug, serde::Serialize)]
pub struct JsonDescribe<'a> {
    /// The shape's version, [`DESCRIBE_VERSION`].
    pub version: u32,
    /// The path asked about.
    pub path: &'a archwarden_core::path::RepoRelPath,
    /// Every rule with something to say about it.
    pub rules: Vec<JsonRule<'a>>,
}

/// One rule, as the JSON carries it.
#[derive(Debug, serde::Serialize)]
pub struct JsonRule<'a> {
    /// The rule's id.
    pub id: &'a str,
    /// Its kind, as written in the config's `type`.
    pub kind: &'static str,
    /// `error` or `warning`.
    pub level: &'a str,
    /// The module it belongs to, when it belongs to one.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub module: Option<&'a str>,
    /// Why the rule exists, when its author said. Issue #46: an agent that
    /// knows the rule and not the reason can comply and nothing else, which is
    /// how a config gets edited to make a check pass.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub why: Option<&'a str>,
    /// Why the module it belongs to exists. A separate answer, not a fallback.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub module_why: Option<&'a str>,
    /// What it requires of this path.
    pub expectations: &'a [archwarden_core::finding::Expectation],
}

/// The JSON envelope for many paths at once.
///
/// A different shape from the one-path answer, because a different question
/// was asked. A consumer that passed a glob knows to expect it.
#[derive(Debug, serde::Serialize)]
pub struct JsonScope<'a> {
    /// The shape's version, [`DESCRIBE_VERSION`].
    pub version: u32,
    /// The glob that was asked about.
    pub scope: &'a str,
    /// One answer per path it matched.
    pub paths: Vec<JsonDescribe<'a>>,
}

/// The JSON answer for one path.
#[must_use]
pub fn envelope<'a>(
    path: &'a archwarden_core::path::RepoRelPath,
    applies: &'a [Applies<'a>],
) -> JsonDescribe<'a> {
    JsonDescribe {
        version: DESCRIBE_VERSION,
        path,
        rules: applies
            .iter()
            .map(|entry| JsonRule {
                id: entry.rule.id.as_str(),
                kind: entry.rule.kind.type_name(),
                level: entry.rule.level.as_str(),
                module: entry
                    .rule
                    .module
                    .as_ref()
                    .map(archwarden_core::ids::ModuleId::as_str),
                why: entry.rule.why.as_deref(),
                module_why: entry.rule.module_why.as_deref(),
                expectations: &entry.expectations,
            })
            .collect(),
    }
}

/// The JSON answer for a glob and everything under it.
#[must_use]
pub fn envelope_many<'a>(
    scope: &'a str,
    answers: &'a [(archwarden_core::path::RepoRelPath, Vec<Applies<'a>>)],
) -> JsonScope<'a> {
    JsonScope {
        version: DESCRIBE_VERSION,
        scope,
        paths: answers
            .iter()
            .map(|(path, applies)| envelope(path, applies))
            .collect(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use archwarden_core::{facts::ExportTags, path::RepoRelPath};

    fn path(p: &str) -> RepoRelPath {
        RepoRelPath::new(p).expect("valid path")
    }

    /// "no rule governs it" rather than "it is not governed".
    ///
    /// The reader's next action is to write a rule or to say in `ignore` that
    /// the file is outside the architecture on purpose, and naming the absent
    /// thing is what points at both. This sentence also lands in
    /// `arch.baseline.json` as the note on every accepted entry, where a
    /// repository migrating onto `governance: closed` will have a great many
    /// of them.
    #[test]
    fn an_ungoverned_file_names_the_absent_rule() {
        assert_eq!(
            describe_observed(&Observed::Ungoverned),
            "no rule governs it"
        );
    }

    /// A deep import names both the specifier and the package; a bare one
    /// names it once.
    ///
    /// Reading "imports `three`, which is part of the package `three`" is the
    /// sentence the `if` exists to avoid, and the shorter half is the one
    /// almost every finding takes — so getting it backwards would be the
    /// common case.
    #[test]
    fn a_package_that_is_not_permitted_names_the_subpath_only_when_there_is_one() {
        assert_eq!(
            describe_observed(&Observed::PackageNotPermitted {
                specifier: "three".to_owned(),
                package: "three".to_owned(),
            }),
            "imports the package `three`, which is not on this rule's list"
        );

        let deep = describe_observed(&Observed::PackageNotPermitted {
            specifier: "three/examples/jsm/loaders/GLTFLoader.js".to_owned(),
            package: "three".to_owned(),
        });
        assert!(
            deep.contains("three/examples/jsm/loaders/GLTFLoader.js"),
            "{deep}"
        );
        assert!(
            deep.contains("part of the package `three`"),
            "the package is what the rule named, and the reader has to see the \
             link between it and what they wrote: {deep}"
        );
    }

    /// The sentence names the destination *and* the way in. A reader told
    /// "depends on `packages/db`" opens the file and finds no such import;
    /// what they need is the middle of the chain, which is where the edit goes.
    #[test]
    fn a_reach_reads_as_the_chain_that_got_there() {
        assert_eq!(
            describe_observed(&Observed::ForbiddenReach {
                chain: vec![
                    path("packages/ui/button.tsx"),
                    path("packages/orders/cart.ts"),
                    path("packages/db/client.ts"),
                ],
            }),
            "ends up depending on `packages/db/client.ts`, through \
             `packages/ui/button.tsx` → `packages/orders/cart.ts` → \
             `packages/db/client.ts`"
        );
    }

    /// A chain that arrived without a destination still reads as a sentence.
    /// Not reachable through the engine, and this is a format a committed
    /// baseline file carries, where a malformed note outlives the run.
    #[test]
    fn a_reach_with_no_chain_still_reads_as_a_sentence() {
        assert_eq!(
            describe_observed(&Observed::ForbiddenReach { chain: Vec::new() }),
            "ends up depending on something the rule forbids"
        );
    }

    /// The chain *is* the sentence. "sits on an import cycle" alone leaves a
    /// reader with nowhere to look; the arrow form names every edge that could
    /// be cut to break it, which is the whole reason the finding carries a
    /// chain rather than a boolean.
    #[test]
    fn a_cycle_reads_as_the_loop_it_closed() {
        assert_eq!(
            describe_observed(&Observed::ImportCycle {
                chain: vec![path("src/a.ts"), path("src/b.ts"), path("src/a.ts")],
            }),
            "sits on an import cycle: `src/a.ts` → `src/b.ts` → `src/a.ts`"
        );
    }

    /// A chain that somehow arrived empty still reads as a sentence rather
    /// than as a stray colon. Not reachable through the engine — the graph
    /// always returns both ends — and this is a format shared with a committed
    /// baseline file, where a malformed note outlives the run that wrote it.
    #[test]
    fn a_cycle_with_no_chain_still_reads_as_a_sentence() {
        assert_eq!(
            describe_observed(&Observed::ImportCycle { chain: Vec::new() }),
            "sits on an import cycle"
        );
    }

    /// The prose comes from the same values the JSON carries, so the two can
    /// never describe one finding differently.
    #[test]
    fn every_observation_has_a_sentence() {
        let cases = [
            (
                Observed::UnexpectedFilename {
                    name: "helpers.ts".to_owned(),
                },
                "helpers.ts",
            ),
            (
                Observed::ExportMissing {
                    name: "Foo".to_owned(),
                },
                "no export named",
            ),
            (
                Observed::ExportWrongKind {
                    name: "Foo".to_owned(),
                    found: ExportTags::only(ExportKind::Const).with(ExportKind::Arrow),
                },
                "`arrow` or `const`",
            ),
            (Observed::OnlyDefaultExport, "does not bind importers"),
            (
                Observed::SiblingMissing {
                    path: path("a.spec.ts"),
                },
                "does not exist",
            ),
            (
                Observed::RequiredCallMissing {
                    symbol: "Event.save".to_owned(),
                },
                "never called",
            ),
        ];

        for (observed, expected_fragment) in cases {
            let sentence = describe_observed(&observed);
            assert!(
                sentence.contains(expected_fragment),
                "{observed:?} rendered as {sentence}"
            );
        }
    }

    /// Issue #44. Six ways a frontmatter block can disappoint a rule, and six
    /// sentences, because they are six different edits.
    #[test]
    fn a_frontmatter_fault_reads_as_a_sentence() {
        let cases = [
            (Observed::FrontmatterAbsent, "has no frontmatter block"),
            (
                Observed::FrontmatterMalformed {
                    reason: "mapping values are not allowed here".to_owned(),
                },
                "is not YAML",
            ),
            (
                Observed::FrontmatterKeyMissing {
                    key: "componentes".to_owned(),
                },
                "carries no `componentes`",
            ),
            (
                Observed::FrontmatterValueOutsideVocabulary {
                    key: "status".to_owned(),
                    found: "concluido".to_owned(),
                },
                "`status` is `concluido`",
            ),
            (
                Observed::FrontmatterValueDisagrees {
                    key: "id".to_owned(),
                    found: "semaforo".to_owned(),
                    wanted: "03-semaforo".to_owned(),
                },
                "`id` is `semaforo`, and the path says `03-semaforo`",
            ),
            (
                Observed::FrontmatterValueNotScalar {
                    key: "nivel".to_owned(),
                },
                "`nivel` is not a single value",
            ),
        ];

        for (observed, fragment) in cases {
            let sentence = describe_observed(&observed);
            assert!(
                sentence.contains(fragment),
                "{observed:?} rendered as {sentence}"
            );
        }
    }

    /// The two annotation faults are different sentences because they are
    /// different fixes. Both would otherwise fall through to the
    /// `non_exhaustive` arm and reach a user as a Rust `Debug` dump, which is
    /// the failure mode that arm exists to soften and not one to ship.
    #[test]
    fn an_annotation_fault_reads_as_a_sentence() {
        let missing = describe_observed(&Observed::ExportMissingAnnotation {
            name: "AGENT_TOOL".to_owned(),
        });
        assert_eq!(missing, "`AGENT_TOOL` declares no type of its own");

        let wrong = describe_observed(&Observed::ExportWrongAnnotation {
            name: "AGENT_TOOL".to_owned(),
            found: vec!["LegacyToolModule".to_owned()],
        });
        assert_eq!(wrong, "`AGENT_TOOL` is declared as `LegacyToolModule`");

        // A class names one contract per `implements` clause, and a sentence
        // that showed only the first would be describing a file that is not
        // there.
        let several = describe_observed(&Observed::ExportWrongAnnotation {
            name: "Tool".to_owned(),
            found: vec!["Disposable".to_owned(), "Serializable".to_owned()],
        });
        assert_eq!(
            several,
            "`Tool` is declared as `Disposable` or `Serializable`"
        );
    }

    /// A deep import names a package the specifier does not spell, so the
    /// sentence has to carry both; a bare one would read "imports `three`,
    /// which is part of the package `three`". And `fs` is not *part of*
    /// `node:fs` — it is the same module, spelled the other way.
    #[test]
    fn a_forbidden_package_names_the_package_only_when_it_differs() {
        let observed = |specifier: &str, package: &str| {
            describe_observed(&Observed::ForbiddenPackageImport {
                specifier: specifier.to_owned(),
                package: package.to_owned(),
            })
        };

        assert_eq!(observed("three", "three"), "imports the package `three`");
        assert_eq!(
            observed("three/examples/jsm/loaders/GLTFLoader.js", "three"),
            "imports `three/examples/jsm/loaders/GLTFLoader.js`, which is part \
             of the package `three`"
        );
        for (written, configured) in [("fs", "node:fs"), ("node:fs", "fs")] {
            assert_eq!(
                observed(written, configured),
                format!("imports the package `{configured}`"),
                "`{written}` and `{configured}` are one module"
            );
        }
    }

    /// The comma rule, at each length it has to answer for. An empty list is
    /// the one that reads wrong by default: "expected " with nothing after it
    /// says the rule wanted nothing.
    #[test]
    fn a_list_reads_as_a_sentence_at_every_length() {
        let none: [&str; 0] = [];
        assert_eq!(join_or(&none, "nothing"), "nothing");
        assert_eq!(join_or(&["a"], "nothing"), "`a`");
        assert_eq!(join_or(&["a", "b"], "nothing"), "`a` or `b`");
        assert_eq!(join_or(&["a", "b", "c"], "nothing"), "`a`, `b` or `c`");
    }

    /// `Observed` is `non_exhaustive`, so a variant added later must still
    /// produce a sentence rather than failing to compile — and a sentence
    /// that names the variant is more use to a reader than a blank.
    #[test]
    fn an_observation_this_build_has_no_prose_for_still_says_something() {
        let sentence = describe_observed(&Observed::RequiredImportMissing);
        assert!(!sentence.is_empty());
    }

    /// The arms the CLI's own tests used to reach only through a rendered
    /// report. They are the sentences four surfaces show and a committed file
    /// stores, so each one is worth pinning where it is written rather than
    /// three layers up through a renderer.
    #[test]
    fn every_remaining_observation_has_a_sentence_too() {
        let cases = [
            (
                Observed::DiscouragedSubfolder {
                    name: "legacy".to_owned(),
                },
                "folder `legacy` is allowed for now, as documented debt",
            ),
            (
                Observed::ReexportOfUnknownKind {
                    name: "Order".to_owned(),
                    from: "./order".to_owned(),
                },
                "`Order` is re-exported from `./order`, so its kind is not determinable here",
            ),
            (
                Observed::RequiredFileMissing {
                    name: "index.ts".to_owned(),
                },
                "`index.ts` is not here",
            ),
            (
                Observed::NoFileMatching {
                    pattern: "*.spec.ts".to_owned(),
                },
                "no file here matches `*.spec.ts`",
            ),
            (
                Observed::SpecIsEmpty {
                    path: path("order.spec.ts"),
                },
                "`order.spec.ts` contains no test cases",
            ),
            (
                Observed::ForbiddenImport {
                    specifier: "../infra/db".to_owned(),
                    resolved: path("src/infra/db.ts"),
                },
                "imports `../infra/db`, which resolves to `src/infra/db.ts`",
            ),
            (
                Observed::RequiredImportForCallMissing {
                    symbol: "track".to_owned(),
                    module: "@app/telemetry".to_owned(),
                },
                "`track` is not imported from `@app/telemetry`",
            ),
        ];

        for (observed, expected) in cases {
            assert_eq!(describe_observed(&observed), expected);
        }
    }

    /// A file that is nothing but a re-export is a different fact from a file
    /// that has one, and the sentence says which — because the reader's next
    /// move differs: delete the file, or delete a line in it.
    ///
    /// The verb agrees with the count as well. "one export only forward" is
    /// the kind of sentence that makes a tool look unfinished.
    #[test]
    fn a_passthrough_says_whether_the_whole_file_is_one() {
        let whole = describe_observed(&Observed::Passthrough {
            exports: vec!["Order".to_owned()],
            whole_file: true,
        });
        assert_eq!(
            whole,
            "adds nothing of its own: `Order` only forwards another module"
        );

        let part = describe_observed(&Observed::Passthrough {
            exports: vec!["Order".to_owned(), "Client".to_owned()],
            whole_file: false,
        });
        assert_eq!(
            part,
            "`Order`, `Client` only forward another module; the rest of the file is its own"
        );
    }

    // --- what applies to a path ------------------------------------------
    //
    // Moved here from `archwarden-cli` in 0.18. `describe` is an operation
    // every surface asks: the command prints it, MCP returns it, and the
    // pre-write hook resolves a path through `repo_relative` before judging a
    // write. It answered from the CLI while MCP was being built, which is the
    // dependency pointing backwards that decision 20 exists to refuse.

    fn a_path(p: &str) -> RepoRelPath {
        RepoRelPath::new(p).expect("valid path")
    }

    fn a_rule(
        id: &str,
        module: Option<&str>,
        scope: &[&str],
        kind: archwarden_core::compiled::CompiledRuleKind,
    ) -> archwarden_core::compiled::CompiledRule {
        archwarden_core::compiled::CompiledRule {
            id: archwarden_core::ids::RuleId::new(id).expect("valid id"),
            module: module.map(|m| archwarden_core::ids::ModuleId::new(m).expect("valid module")),
            why: None,
            module_why: None,
            imports: None,
            level: archwarden_core::level::Level::Error,
            scope: archwarden_core::scope::Scope::compile(scope.iter().copied())
                .expect("valid scope"),
            kind,
        }
    }

    fn a_naming_rule() -> archwarden_core::compiled::CompiledRuleKind {
        archwarden_core::compiled::CompiledRuleKind::Naming {
            file_pattern: archwarden_core::pattern::Pattern::compile(
                r"^(?<name>[a-z0-9-]+)\.use-case\.ts$",
            )
            .expect("valid pattern"),
            dir_pattern: None,
            name_template: "{{pascal(name)}}".to_owned(),
            kind: archwarden_core::facts::KindFilter::OneOf(ExportTags::only(ExportKind::Function)),
            annotation: Vec::new(),
            signature_hint: Some("(deps: Deps) => UseCase".to_owned()),
        }
    }

    fn a_spec_pair_rule() -> archwarden_core::compiled::CompiledRuleKind {
        archwarden_core::compiled::CompiledRuleKind::SpecPair {
            subfolders: vec![".".to_owned()],
            spec_markers: vec!["spec".to_owned()],
            ignore_files: archwarden_core::glob::PathSet::default(),
            spec_dirs: Vec::new(),
            require_non_empty_spec: true,
            skip_type_only: false,
        }
    }

    fn a_structure_rule() -> archwarden_core::compiled::CompiledRuleKind {
        archwarden_core::compiled::CompiledRuleKind::Structure {
            allowed_subfolders: Some(vec!["types".to_owned()]),
            warn_subfolders: Vec::new(),
            recurse_into: Vec::new(),
            subfolder_patterns: Vec::new(),
            filename_patterns: Vec::new(),
        }
    }

    fn a_config(
        rules: Vec<archwarden_core::compiled::CompiledRule>,
        ignore: &[&str],
    ) -> archwarden_core::compiled::CompiledConfig {
        archwarden_core::compiled::CompiledConfig::new(
            rules,
            archwarden_core::glob::PathSet::compile(ignore.iter().map(|g| (*g).to_owned()))
                .expect("valid globs"),
            archwarden_core::compiled::SkipDirs::default(),
            archwarden_core::hash::ContentHash::of(b"describe"),
        )
    }

    /// The JSON as a value, which is what every surface serialises.
    fn as_json(
        config: &archwarden_core::compiled::CompiledConfig,
        target: &RepoRelPath,
    ) -> serde_json::Value {
        serde_json::to_value(envelope(target, &describe(config, target))).expect("serialises")
    }

    #[test]
    fn a_path_that_does_not_exist_still_has_rules() {
        let config = a_config(
            vec![
                a_rule("usecase-name", Some("app"), &["src/*"], a_naming_rule()),
                a_rule("usecase-spec", None, &["src/*"], a_spec_pair_rule()),
            ],
            &[],
        );
        let target = a_path("src/user/create-client.use-case.ts");

        let applies = describe(&config, &target);
        let ids: Vec<_> = applies.iter().map(|a| a.rule.id.as_str()).collect();

        assert_eq!(ids, ["usecase-name", "usecase-spec"]);
    }

    /// A rule whose scope covers the path but which has nothing to say about
    /// *this* file is not listed. "Applies" means "has a requirement", not
    /// "the glob matched".
    #[test]
    fn a_rule_with_nothing_to_say_is_not_listed() {
        let config = a_config(
            vec![a_rule("usecase-name", None, &["src/*"], a_naming_rule())],
            &[],
        );

        assert!(describe(&config, &a_path("src/user/helper.ts")).is_empty());
    }

    /// An `ignore` entry wins over any rule's scope, and `describe` has to
    /// agree with `check` about that or an agent would be told to satisfy a
    /// rule that will never fire.
    #[test]
    fn an_ignored_path_has_no_rules() {
        let config = a_config(
            vec![a_rule("usecase-name", None, &["src/*"], a_naming_rule())],
            &["src/legacy/**"],
        );

        assert!(
            describe(&config, &a_path("src/legacy/old.use-case.ts")).is_empty(),
            "ignore wins"
        );
        assert_eq!(
            describe(&config, &a_path("src/user/new.use-case.ts")).len(),
            1,
            "and only for the ignored subtree"
        );
    }

    /// Configuration order is preserved, so the answer reads in the order the
    /// user wrote their rules rather than in whatever order engines are built.
    #[test]
    fn rules_come_back_in_configuration_order() {
        let config = a_config(
            vec![
                a_rule("second", None, &["src/*"], a_spec_pair_rule()),
                a_rule("first", None, &["src/*"], a_naming_rule()),
            ],
            &[],
        );

        let applies = describe(&config, &a_path("src/user/create.use-case.ts"));
        let ids: Vec<_> = applies.iter().map(|a| a.rule.id.as_str()).collect();
        assert_eq!(ids, ["second", "first"]);
    }

    /// The JSON is a contract with agents, so it is asserted field by field.
    #[test]
    fn the_json_shape_is_versioned_and_complete() {
        let config = a_config(
            vec![
                a_rule("usecase-name", Some("app"), &["src/*"], a_naming_rule()),
                a_rule("usecase-spec", None, &["src/*"], a_spec_pair_rule()),
            ],
            &[],
        );

        let parsed = as_json(&config, &a_path("src/user/create-client.use-case.ts"));

        assert_eq!(parsed["version"], 0);
        assert_eq!(parsed["path"], "src/user/create-client.use-case.ts");

        let first = &parsed["rules"][0];
        assert_eq!(first["id"], "usecase-name");
        assert_eq!(first["kind"], "naming");
        assert_eq!(first["level"], "error");
        assert_eq!(first["module"], "app");
        assert_eq!(first["expectations"][0]["type"], "required-export");
        assert_eq!(first["expectations"][0]["name"], "CreateClient");

        let second = &parsed["rules"][1];
        assert_eq!(second["kind"], "spec-pair");
        assert!(second["module"].is_null(), "a top-level rule has none");
        assert_eq!(
            second["expectations"][0]["path"],
            "src/user/create-client.use-case.spec.ts"
        );
    }

    /// Issue #46. `describe` is what an agent asks *before* writing, which is
    /// the moment a reason is worth most: knowing the rule is not knowing why,
    /// and a constraint that looks arbitrary is the one that gets worked
    /// around.
    #[test]
    fn a_rules_reason_reaches_the_json() {
        let mut governed = a_rule(
            "domain-forbids-app",
            Some("domain"),
            &["src/*"],
            a_structure_rule(),
        );
        governed.why = Some("domain is published and the app is not".to_owned());
        governed.module_why = Some("extracted so billing could depend on it".to_owned());

        let parsed = as_json(&a_config(vec![governed], &[]), &a_path("src/user"));

        assert_eq!(
            parsed["rules"][0]["why"],
            "domain is published and the app is not"
        );
        assert_eq!(
            parsed["rules"][0]["module_why"],
            "extracted so billing could depend on it"
        );
    }

    /// A rule with no module omits the field rather than sending `null`, so
    /// the common answer stays small.
    #[test]
    fn a_rule_without_a_module_omits_the_field() {
        let config = a_config(
            vec![a_rule("usecase-name", None, &["src/*"], a_naming_rule())],
            &[],
        );
        let json = serde_json::to_string(&envelope(
            &a_path("src/user/create.use-case.ts"),
            &describe(&config, &a_path("src/user/create.use-case.ts")),
        ))
        .expect("serialises");

        assert!(!json.contains("\"module\""), "{json}");
    }

    /// The many-path envelope is a different shape from the one-path answer,
    /// because a different question was asked.
    #[test]
    fn the_many_path_envelope_carries_the_full_answer_per_path() {
        let config = a_config(
            vec![a_rule(
                "shape",
                None,
                &["packages/domain/src/*"],
                a_structure_rule(),
            )],
            &[],
        );
        let path = a_path("packages/domain/src/invoice");
        let answers = vec![(path.clone(), describe(&config, &path))];

        let parsed = serde_json::to_value(envelope_many("packages/domain/src/*", &answers))
            .expect("serialises");

        assert_eq!(parsed["scope"], "packages/domain/src/*");
        assert_eq!(parsed["paths"][0]["path"], "packages/domain/src/invoice");
        assert_eq!(parsed["paths"][0]["rules"][0]["id"], "shape");
        assert!(
            parsed["paths"][0]["rules"][0]["expectations"].is_array(),
            "the detail is there"
        );
        assert!(
            parsed.get("path").is_none(),
            "a different shape, because a different question was asked"
        );
    }

    // --- path resolution -------------------------------------------------

    fn a_root() -> camino::Utf8PathBuf {
        camino::Utf8PathBuf::from("/repo")
    }

    /// The ordinary case: run from the root, name a path.
    #[test]
    fn a_relative_path_resolves_against_the_working_directory() {
        assert_eq!(
            repo_relative(&a_root(), &a_root(), None, "src/user/a.ts").expect("resolves"),
            a_path("src/user/a.ts")
        );
    }

    /// The case that makes this worth a function: the user is standing in a
    /// subdirectory, which is where anyone actually works.
    #[test]
    fn a_relative_path_from_a_subdirectory_is_still_repo_relative() {
        assert_eq!(
            repo_relative(&a_root(), &a_root().join("src/user"), None, "a.ts").expect("resolves"),
            a_path("src/user/a.ts")
        );
        assert_eq!(
            repo_relative(
                &a_root(),
                &a_root().join("src/user"),
                None,
                "../shared/b.ts"
            )
            .expect("resolves"),
            a_path("src/shared/b.ts")
        );
    }

    /// An absolute path is accepted, because a harness hook has one and should
    /// not have to make it relative first.
    #[test]
    fn an_absolute_path_inside_the_repository_resolves() {
        assert_eq!(
            repo_relative(&a_root(), &a_root(), None, "/repo/src/user/a.ts").expect("resolves"),
            a_path("src/user/a.ts")
        );
    }

    /// And one outside says so, naming both halves, rather than silently
    /// describing the wrong file.
    #[test]
    fn a_path_outside_the_repository_is_refused() {
        assert_eq!(
            repo_relative(&a_root(), &a_root(), None, "/elsewhere/a.ts").expect_err("outside"),
            "`/elsewhere/a.ts` is outside the repository at `/repo`"
        );
        assert_eq!(
            repo_relative(&a_root(), &a_root(), None, "../a.ts").expect_err("escapes"),
            "`../a.ts` is not a path inside the repository: `../a.ts` escapes the repository root"
        );
    }

    /// The working directory has to be inside the repository too, and saying
    /// which of the two is wrong is the difference between a fixable message
    /// and a puzzling one.
    #[test]
    fn a_working_directory_outside_the_repository_is_named_as_such() {
        assert_eq!(
            repo_relative(&a_root(), camino::Utf8Path::new("/elsewhere"), None, "a.ts")
                .expect_err("outside"),
            "the working directory `/elsewhere` is outside `/repo`"
        );
    }

    /// Two spellings of one directory are one directory.
    ///
    /// A symlinked checkout, a bind-mounted worktree, `/tmp` → `/private/tmp`
    /// on macOS, a container whose mount path differs from the host's: each
    /// gives a repository two absolute paths, and a harness hands over
    /// whichever one its own `cwd` resolved to. Comparing the two as text
    /// answers "outside the repository" about a file plainly inside it.
    ///
    /// Reported against 0.10.0, where the consequence was a pre-write hook
    /// that permitted every write on such a machine while reporting success.
    #[cfg(unix)]
    #[test]
    fn a_second_route_to_the_same_directory_is_the_same_directory() {
        let temporary = tempfile::tempdir().expect("temp dir");
        let real = temporary.path().join("real");
        std::fs::create_dir_all(real.join("src")).expect("create");
        std::fs::write(real.join("src/a.ts"), b"export const a = 1;").expect("write");

        let link = temporary.path().join("link");
        std::os::unix::fs::symlink(&real, &link).expect("symlink");

        let root = camino::Utf8PathBuf::from_path_buf(real).expect("utf-8");
        let through_link = camino::Utf8PathBuf::from_path_buf(link)
            .expect("utf-8")
            .join("src/a.ts");

        assert_eq!(
            repo_relative(&root, &root, None, through_link.as_str()).expect("resolves"),
            a_path("src/a.ts")
        );
    }

    /// And the same when the file is not there yet, which is the case a
    /// pre-write hook is always in: it is asked before the write, so the path
    /// it is handed usually names nothing on disk.
    #[cfg(unix)]
    #[test]
    fn a_second_route_resolves_for_a_file_that_does_not_exist_yet() {
        let temporary = tempfile::tempdir().expect("temp dir");
        let real = temporary.path().join("real");
        std::fs::create_dir_all(real.join("src")).expect("create");

        let link = temporary.path().join("link");
        std::os::unix::fs::symlink(&real, &link).expect("symlink");

        let root = camino::Utf8PathBuf::from_path_buf(real).expect("utf-8");
        let through_link = camino::Utf8PathBuf::from_path_buf(link)
            .expect("utf-8")
            .join("src/not-written-yet.ts");

        assert_eq!(
            repo_relative(&root, &root, None, through_link.as_str()).expect("resolves"),
            a_path("src/not-written-yet.ts")
        );
    }

    /// A path that is genuinely elsewhere still says so. The point is to stop
    /// mistaking one directory for two, not to stop refusing.
    #[cfg(unix)]
    #[test]
    fn a_path_that_is_really_outside_is_still_refused() {
        let temporary = tempfile::tempdir().expect("temp dir");
        let real = temporary.path().join("real");
        let other = temporary.path().join("other");
        std::fs::create_dir_all(&real).expect("create");
        std::fs::create_dir_all(&other).expect("create");
        std::fs::write(other.join("a.ts"), b"export const a = 1;").expect("write");

        let root = camino::Utf8PathBuf::from_path_buf(real).expect("utf-8");
        let outside = camino::Utf8PathBuf::from_path_buf(other)
            .expect("utf-8")
            .join("a.ts");

        assert!(
            repo_relative(&root, &root, None, outside.as_str()).is_err(),
            "a path in another directory was accepted"
        );
    }

    /// The repository root itself is a directory, and a structure rule has
    /// something to say about directories.
    #[test]
    fn the_root_is_addressable() {
        assert_eq!(
            repo_relative(&a_root(), &a_root(), None, ".").expect("resolves"),
            a_path("")
        );
    }

    // --- the two readings of one relative path ---------------------------

    /// A tree on disk, because existence is what tells the two readings apart.
    fn tree(entries: &[&str]) -> (tempfile::TempDir, camino::Utf8PathBuf) {
        let guard = tempfile::tempdir().expect("temp dir");
        let root = camino::Utf8PathBuf::from_path_buf(guard.path().to_path_buf())
            .expect("temp path is UTF-8");
        for entry in entries {
            let file = root.join(entry);
            std::fs::create_dir_all(file.parent().expect("a file has a parent"))
                .expect("create dirs");
            std::fs::write(&file, "export const a = 1;\n").expect("write");
        }
        (guard, root)
    }

    /// The defect this replaces. Every path archwarden prints is
    /// repository-relative, so the one an agent copies out of a report is too
    /// — and pasting it back while standing in a subdirectory used to resolve
    /// to `packages/domain/packages/domain/...`, which does not exist.
    ///
    /// It did not fail. It answered "no rule applies", which reads exactly
    /// like "nothing constrains this file".
    #[test]
    fn a_repository_relative_path_pasted_from_a_report_resolves() {
        let (_guard, root) = tree(&["packages/domain/src/order/calcs/x.ts"]);
        let inside = root.join("packages/domain");

        assert_eq!(
            repo_relative(&root, &inside, None, "packages/domain/src/order/calcs/x.ts")
                .expect("resolves"),
            a_path("packages/domain/src/order/calcs/x.ts")
        );
    }

    /// And the reading that was always right stays right. This is the path a
    /// developer has in hand from `git diff` or an editor, and it wins when
    /// both readings name something real.
    #[test]
    fn a_path_relative_to_where_you_stand_still_wins() {
        let (_guard, root) = tree(&[
            "packages/domain/src/order/calcs/x.ts",
            // The same relative path, real from the root as well. Whoever is
            // standing in `packages/domain` means theirs.
            "src/order/calcs/x.ts",
        ]);
        let inside = root.join("packages/domain");

        assert_eq!(
            repo_relative(&root, &inside, None, "src/order/calcs/x.ts").expect("resolves"),
            a_path("packages/domain/src/order/calcs/x.ts")
        );
    }

    /// A path only the root reading finds is the root reading's, even though
    /// it does not begin with where the user is standing.
    #[test]
    fn a_path_only_the_repository_reading_finds_is_taken() {
        let (_guard, root) = tree(&["src/shared/b.ts"]);
        let inside = root.join("packages/domain");
        std::fs::create_dir_all(&inside).expect("create dirs");

        assert_eq!(
            repo_relative(&root, &inside, None, "src/shared/b.ts").expect("resolves"),
            a_path("src/shared/b.ts")
        );
    }

    /// `describe` is asked about files that do not exist yet -- that is what
    /// it is for. With nothing on disk to go by, a path that already starts
    /// with where the user is standing is repository-relative: nobody nests
    /// `packages/domain` inside `packages/domain`.
    #[test]
    fn a_file_that_does_not_exist_yet_is_read_by_its_prefix() {
        let (_guard, root) = tree(&[]);
        let inside = root.join("packages/domain");
        std::fs::create_dir_all(&inside).expect("create dirs");

        assert_eq!(
            repo_relative(&root, &inside, None, "packages/domain/src/new/thing.ts")
                .expect("resolves"),
            a_path("packages/domain/src/new/thing.ts")
        );
        // And one that does not carry the prefix is where the user is standing,
        // which is the older behaviour and the common case.
        assert_eq!(
            repo_relative(&root, &inside, None, "src/new/thing.ts").expect("resolves"),
            a_path("packages/domain/src/new/thing.ts")
        );
    }

    /// From the root the two readings are the same question, so nothing here
    /// costs the common case anything.
    #[test]
    fn from_the_root_there_is_only_one_reading() {
        let (_guard, root) = tree(&["src/user/a.ts"]);

        assert_eq!(
            repo_relative(&root, &root, None, "src/user/a.ts").expect("resolves"),
            a_path("src/user/a.ts")
        );
        assert_eq!(
            repo_relative(&root, &root, None, "src/nothing/here.ts").expect("resolves"),
            a_path("src/nothing/here.ts")
        );
    }

    /// A directory answers the same way a file does: `describe` and `scaffold`
    /// both take one, and a structure rule has more to say about a directory
    /// than about anything in it.
    #[test]
    fn a_directory_resolves_by_the_same_rules() {
        let (_guard, root) = tree(&["packages/domain/src/order/calcs/x.ts"]);
        let inside = root.join("packages/domain");

        assert_eq!(
            repo_relative(&root, &inside, None, "packages/domain/src/order").expect("resolves"),
            a_path("packages/domain/src/order")
        );
    }

    // --- one repository, two roots --------------------------------------
    //
    // Issue #93, decided in 24. A harness on the host and an archwarden inside
    // a container disagree about the repository's absolute path and agree
    // about everything inside it.

    /// The reported case, end to end: the harness's path, our root, one file.
    #[test]
    fn a_path_named_from_the_callers_root_is_found_under_ours() {
        let (_guard, root) = tree(&["src/order/x.ts"]);
        let theirs = camino::Utf8Path::new("/home/dev/projeto");

        assert_eq!(
            repo_relative(
                &root,
                &root,
                Some(theirs),
                "/home/dev/projeto/src/order/x.ts"
            )
            .expect("resolves"),
            a_path("src/order/x.ts")
        );
    }

    /// And for a file that does not exist yet, which is what the pre-write hook
    /// is always asking about. Its directory is what carries the evidence.
    #[test]
    fn a_file_that_does_not_exist_yet_is_translated_by_its_directory() {
        let (_guard, root) = tree(&["src/order/x.ts"]);
        let theirs = camino::Utf8Path::new("/home/dev/projeto");

        assert_eq!(
            repo_relative(
                &root,
                &root,
                Some(theirs),
                "/home/dev/projeto/src/order/not-written-yet.ts"
            )
            .expect("resolves"),
            a_path("src/order/not-written-yet.ts")
        );
    }

    /// The guard, and the reason this is a decision rather than a patch.
    ///
    /// A wrapper pointed at a container holding a *different* project would
    /// have its paths rewritten into ours and judged against our rules — a
    /// quiet, wrong approval in place of a loud, useless refusal. Nothing on
    /// our side stands under the translated path, so it is refused.
    #[test]
    fn a_path_from_another_project_entirely_is_refused_rather_than_translated() {
        let (_guard, root) = tree(&["src/order/x.ts"]);
        let theirs = camino::Utf8Path::new("/home/dev/outro");

        let refusal = repo_relative(
            &root,
            &root,
            Some(theirs),
            "/home/dev/outro/servico/interno/y.ts",
        )
        .expect_err("nothing here stands under that");

        assert!(refusal.contains("outside the repository"), "{refusal}");
    }

    /// When it refuses, it names **both** roots. "Outside the repository" about
    /// a path the caller believes is inside it sends a reader nowhere.
    #[test]
    fn a_refusal_names_the_callers_root_as_well_as_ours() {
        let (_guard, root) = tree(&[]);
        let theirs = camino::Utf8Path::new("/home/dev/projeto");

        let refusal = repo_relative(&root, &root, Some(theirs), "/somewhere/else/x.ts")
            .expect_err("outside both");

        assert!(refusal.contains(root.as_str()), "ours: {refusal}");
        assert!(refusal.contains("/home/dev/projeto"), "theirs: {refusal}");
        assert!(
            refusal.contains("where the caller says the repository is"),
            "{refusal}"
        );
    }

    /// A caller whose root is ours changes nothing. The translation is not
    /// reached, and the ordinary case does not pay for a branch it never takes.
    #[test]
    fn a_caller_standing_where_we_stand_is_the_ordinary_case() {
        let (_guard, root) = tree(&["src/user/a.ts"]);

        assert_eq!(
            repo_relative(
                &root,
                &root,
                Some(&root),
                root.join("src/user/a.ts").as_str()
            )
            .expect("resolves"),
            a_path("src/user/a.ts")
        );
        // And the refusal for a path outside it says one root, not the same
        // one twice.
        let refusal =
            repo_relative(&root, &root, Some(&root), "/elsewhere/a.ts").expect_err("outside");
        assert!(!refusal.contains("the caller says"), "{refusal}");
    }

    /// A translation that would escape our root is refused by `RepoRelPath`,
    /// which is the second half of what keeps this from being a guess.
    #[test]
    fn a_translation_that_would_escape_the_repository_is_still_refused() {
        let (_guard, root) = tree(&["src/x.ts"]);
        let theirs = camino::Utf8Path::new("/home/dev/projeto");

        assert!(
            repo_relative(
                &root,
                &root,
                Some(theirs),
                "/home/dev/projeto/../outro/x.ts"
            )
            .is_err()
        );
    }

    /// No caller root is how every other surface calls this, and it behaves
    /// exactly as it did before 0.19.
    #[test]
    fn no_callers_root_leaves_the_old_answer_untouched() {
        let (_guard, root) = tree(&["src/x.ts"]);

        assert_eq!(
            repo_relative(&root, &root, None, "src/x.ts").expect("resolves"),
            a_path("src/x.ts")
        );
        let refusal = repo_relative(&root, &root, None, "/elsewhere/x.ts").expect_err("outside");
        assert_eq!(
            refusal,
            format!("`/elsewhere/x.ts` is outside the repository at `{root}`")
        );
    }
}
