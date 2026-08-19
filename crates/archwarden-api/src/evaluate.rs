//! Evaluate: running the rules over the tree, and the cache that makes the
//! next run faster.

use archwarden_cache::store::{Cache, CacheError};
use archwarden_core::compiled::CompiledConfig;
use archwarden_engine::{run::Report, walk::RepoTree};
use camino::{Utf8Path, Utf8PathBuf};

/// archwarden's own directory in the repository, and the cache inside it.
///
/// Decision 4 in `DECISIONS.md`: archwarden owns `.archwarden/` for generated
/// artefacts and never writes anywhere else in the user's tree.
pub const CACHE_DIRECTORY: &str = ".archwarden/cache";

/// The database file itself. Its format version lives inside it, so this name
/// does not change when the shape does.
pub const CACHE_FILE: &str = "cache.redb";

/// Where this repository's cache lives.
#[must_use]
pub fn cache_path(root: &Utf8Path) -> Utf8PathBuf {
    root.join(CACHE_DIRECTORY).join(CACHE_FILE)
}

/// Whether this run may use the repository's cache.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CachePolicy {
    /// Read and write it, when a rule reads files at all.
    Use,
    /// Neither read nor write it. `--no-cache`, and the commands that ask a
    /// question about one file rather than running the repository.
    Ignore,
}

/// Something worth telling the caller that is not a failure.
///
/// The distinction this type exists to keep: a cache that will not open costs
/// the next run its speed and nothing else, so refusing to lint over it would
/// be the wrong trade. But *silently* degrading is the other wrong trade —
/// a user whose runs got slow has no way to find out why.
///
/// Returning it rather than writing it is what lets each surface decide.
/// `check` prints it to stderr; MCP will put it in an array beside the
/// findings. Before this existed, `check` reported the flush failure and
/// `baseline` discarded it with `let _ = cache.flush()` — two copies of one
/// orchestration quietly disagreeing about whether a user hears it.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum Note {
    /// The run went ahead without a cache.
    #[error("running without a cache — {source}")]
    CacheUnavailable {
        /// Why it could not be opened.
        #[source]
        source: CacheError,
    },

    /// The findings are correct and were not stored for next time.
    #[error("the cache was not written — {source}")]
    CacheNotWritten {
        /// Why it could not be persisted.
        #[source]
        source: CacheError,
    },
}

/// What to evaluate, and under what cache policy.
#[derive(Debug)]
pub struct Evaluation<'a> {
    /// The repository root.
    pub root: &'a Utf8Path,
    /// The compiled configuration.
    pub compiled: &'a CompiledConfig,
    /// The walked repository.
    pub tree: &'a RepoTree,
    /// Whether the cache may be used.
    pub cache: CachePolicy,
    /// The day this run answers for.
    ///
    /// Threaded to every rule through `FileContext::as_of` rather than read
    /// from a clock, so two machines given the same date give the same answer.
    /// Only `metadata.deadline` asks. Issue #117.
    pub as_of: archwarden_core::date::Date,
}

/// What evaluating produced.
#[derive(Debug)]
pub struct Evaluated {
    /// The findings, and the counts beside them.
    pub report: Report,
    /// Anything degraded along the way. Empty on a healthy run.
    pub notes: Vec<Note>,
}

/// Evaluate: runs every rule over the tree.
///
/// Never fails. Everything that can go wrong at this stage is a cache problem,
/// and a cache problem is a slower run rather than a wrong one — so it comes
/// back as a [`Note`] beside a report that is complete either way.
#[must_use]
pub fn evaluate(input: &Evaluation<'_>) -> Evaluated {
    let mut notes = Vec::new();

    // Opened only when a rule will actually look inside a file. A purely
    // structural configuration reads no bytes, and a cache it never consults
    // would just be a file somebody has to wonder about.
    let mut cache = if input.cache == CachePolicy::Ignore
        || !archwarden_engine::run::reads_files(input.compiled)
    {
        None
    } else {
        match Cache::open(&cache_path(input.root)) {
            Ok(cache) => Some(cache),
            Err(source) => {
                notes.push(Note::CacheUnavailable { source });
                None
            }
        }
    };

    let report = archwarden_engine::run::check(archwarden_engine::run::Run {
        root: input.root,
        config: input.compiled,
        tree: input.tree,
        cache: cache.as_mut(),
        as_of: input.as_of,
    });

    if let Some(cache) = cache.as_mut() {
        notes.extend(persisted(cache.flush()));
    }

    Evaluated { report, notes }
}

/// Turns a flush outcome into a note, or into nothing when it persisted.
///
/// Its own function because the failing branch cannot be produced from a test.
/// Making redb fail a commit means a disk that fills or a file that vanishes
/// mid-run, and `archwarden-cache` has no test for it either — every case
/// there asserts the flush succeeded. Left inline, `Note::CacheNotWritten`
/// would be a variant of a public enum that nothing constructs and no mutant
/// could be caught in.
///
/// So the *mapping* is pinned here, on an error value a test can hold, and the
/// only thing left unexercised is redb failing — which is not this crate's
/// behaviour to assert.
fn persisted(outcome: Result<(), CacheError>) -> Option<Note> {
    outcome.err().map(|source| Note::CacheNotWritten { source })
}

#[cfg(test)]
mod tests {
    use crate::{CachePolicy, Evaluation, Note, evaluate};
    use camino::{Utf8Path, Utf8PathBuf};

    /// A `structure` rule is purely lexical: it decides from the tree alone
    /// and never opens a file.
    const READS_NOTHING: &str = r#"{"version":0,"rules":[
        {"type":"structure","id":"shape","level":"error",
         "roots":"src/*","allowed_subfolders":["domain"]}]}"#;

    /// A `naming` rule with `must_export` has to parse the file to know what
    /// it exports, so a run of it has something worth caching.
    const READS_FILES: &str = r#"{"version":0,"rules":[
        {"type":"naming","id":"usecase-name","level":"error","roots":"src/*",
         "file_pattern":"^(?<name>[a-z0-9-]+)\\.use-case\\.ts$",
         "must_export":{"name":"{{pascal(name)}}","kind":"function"}}]}"#;

    fn repository(config: &str) -> (tempfile::TempDir, Utf8PathBuf) {
        let directory = tempfile::tempdir().unwrap();
        let root = Utf8PathBuf::from_path_buf(directory.path().canonicalize().unwrap()).unwrap();

        std::fs::write(root.join("arch.config.json"), config).unwrap();
        std::fs::create_dir_all(root.join("src/user")).unwrap();
        std::fs::write(
            root.join("src/user/create-client.use-case.ts"),
            "export function CreateClient() {}",
        )
        .unwrap();

        (directory, root)
    }

    fn run(root: &Utf8Path, cache: CachePolicy) -> crate::Evaluated {
        let prepared = crate::prepare(
            crate::Location {
                config: None,
                root: None,
            },
            root,
        )
        .unwrap();
        let tree = crate::walk(root, root, &prepared.compiled).unwrap();

        evaluate(&Evaluation {
            root,
            compiled: &prepared.compiled,
            tree: &tree,
            cache,
            as_of: archwarden_core::date::Date::EPOCH,
        })
    }

    /// A purely structural configuration reads no bytes, so a cache it never
    /// consults would just be a file somebody has to wonder about.
    #[test]
    fn a_configuration_that_reads_no_files_leaves_no_cache_behind() {
        let (_directory, root) = repository(READS_NOTHING);

        let evaluated = run(&root, CachePolicy::Use);

        assert!(evaluated.notes.is_empty());
        assert!(!crate::cache_path(&root).exists());
    }

    #[test]
    fn a_configuration_that_reads_files_writes_one() {
        let (_directory, root) = repository(READS_FILES);

        let evaluated = run(&root, CachePolicy::Use);

        assert!(evaluated.notes.is_empty(), "{:?}", evaluated.notes);
        assert!(crate::cache_path(&root).exists());
    }

    #[test]
    fn refusing_the_cache_neither_reads_nor_writes_one() {
        let (_directory, root) = repository(READS_FILES);

        let evaluated = run(&root, CachePolicy::Ignore);

        assert!(evaluated.notes.is_empty());
        assert!(!crate::cache_path(&root).exists());
        assert_eq!(evaluated.report.findings.len(), 0);
    }

    /// A cache is a rebuildable artefact, so a damaged one degrades the run
    /// instead of ending it. That is a decision the *surface* must be told
    /// about rather than one this can make silently: a note is a value here,
    /// which the CLI prints to stderr and MCP will put in an array.
    ///
    /// Before this, `check` reported it and `baseline` discarded it with
    /// `let _ = cache.flush()` — two copies of one orchestration disagreeing
    /// about whether a user hears that their next run will be slow.
    #[test]
    fn a_cache_that_cannot_be_opened_is_a_note_and_not_a_failure() {
        let (_directory, root) = repository(READS_FILES);

        // A directory where the database file goes. `Cache::open` recreates an
        // unusable cache, and cannot recreate over this.
        std::fs::create_dir_all(crate::cache_path(&root)).unwrap();

        let evaluated = run(&root, CachePolicy::Use);

        assert!(
            matches!(evaluated.notes.as_slice(), [Note::CacheUnavailable { .. }]),
            "{:?}",
            evaluated.notes
        );
        assert_eq!(
            evaluated.notes[0].to_string(),
            format!(
                "running without a cache — cannot open the cache at `{}`",
                crate::cache_path(&root)
            )
        );
    }

    /// The run still happened. A note that came at the cost of the answer
    /// would be a failure wearing a softer word.
    #[test]
    fn the_findings_survive_a_cache_that_could_not_be_opened() {
        let (_directory, root) = repository(READS_FILES);
        std::fs::create_dir_all(crate::cache_path(&root)).unwrap();

        let evaluated = run(&root, CachePolicy::Use);

        assert_eq!(evaluated.report.findings.len(), 0);
        assert_eq!(evaluated.report.files_parsed, 1);
    }

    /// A flush that worked says nothing. The `None` arm is the one every
    /// healthy run takes, and asserting it is what stops the note from
    /// becoming a line on every successful check.
    #[test]
    fn a_cache_that_persisted_is_worth_no_words() {
        assert!(crate::evaluate::persisted(Ok(())).is_none());
    }

    /// And a flush that did not is reported, in the words that say the run was
    /// still correct — the findings are right, the next one will be slow.
    #[test]
    fn a_cache_that_did_not_persist_says_the_next_run_pays_for_it() {
        let (_directory, root) = repository(READS_FILES);
        std::fs::create_dir_all(crate::cache_path(&root)).unwrap();
        let failure = archwarden_cache::store::Cache::open(&crate::cache_path(&root))
            .expect_err("a directory is not a database");

        let note = crate::evaluate::persisted(Err(failure)).expect("a failure is worth saying");

        assert!(
            note.to_string().starts_with("the cache was not written — "),
            "{note}"
        );
    }

    /// The second run reads what the first one wrote, which is the only
    /// reason the cache exists.
    #[test]
    fn a_second_run_reuses_the_facts_the_first_one_stored() {
        let (_directory, root) = repository(READS_FILES);

        let first = run(&root, CachePolicy::Use);
        let second = run(&root, CachePolicy::Use);

        assert_eq!(first.report.files_parsed, 1);
        assert_eq!(first.report.facts_reused, 0);
        assert_eq!(second.report.files_parsed, 0);
        assert_eq!(second.report.facts_reused, 1);
    }
}
