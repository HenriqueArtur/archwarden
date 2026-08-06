//! The on-disk cache.
//!
//! Two tables, because facts and findings become stale for different reasons.
//! Facts depend only on a file's bytes; findings depend on those *and* on the
//! rules and on how imports resolve. Keeping them apart means editing one rule
//! in a config does not throw away every parse result in the repository.
//!
//! Random access is the shape that matters, not bulk load: `check --file` runs
//! once per agent write and has to read a single entry out of tens of
//! thousands inside a ~20 ms budget. See `docs/ARCHITECTURE.md`.
//!
//! Values are `MessagePack` rather than the postcard the plan named. A `Finding`
//! holds internally tagged enums -- `#[serde(tag = "type")]`, chosen because
//! the JSON report is a contract with agents -- and serde cannot write one of
//! those into a format that is not self-describing. postcard and bincode are
//! both out; `MessagePack` is self-describing and still compact.

use archwarden_core::{facts::FileFacts, finding::Finding, hash::ContentHash};
use camino::{Utf8Path, Utf8PathBuf};
use redb::{Database, ReadableDatabase, TableDefinition};

/// Bumped when a stored value's shape changes.
///
/// A mismatch wipes the cache rather than trying to read the old shape. A
/// cache is a rebuildable artefact, and migration code for one is a liability
/// nobody is paid back for. See decision 3.
pub const FORMAT_VERSION: u32 = 3;

const META: TableDefinition<'_, &str, u32> = TableDefinition::new("meta");
const FACTS: TableDefinition<'_, &[u8], &[u8]> = TableDefinition::new("facts");
const FINDINGS: TableDefinition<'_, &[u8], &[u8]> = TableDefinition::new("findings");

const VERSION_KEY: &str = "format_version";

/// Why the cache could not be used.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum CacheError {
    /// The database could not be opened or created.
    #[error("cannot open the cache at `{path}`")]
    Unopenable {
        /// Where the cache lives.
        path: Utf8PathBuf,
        /// What redb said.
        #[source]
        source: Box<redb::Error>,
    },

    /// A read or write failed.
    #[error("cache operation failed")]
    Failed {
        /// What redb said.
        #[source]
        source: Box<redb::Error>,
    },
}

/// A content-addressed cache of facts and findings.
#[derive(Debug)]
pub struct Cache {
    database: Database,
    path: Utf8PathBuf,
    pending_facts: Vec<(ContentHash, Vec<u8>)>,
    pending_findings: Vec<(ContentHash, Vec<u8>)>,
}

impl Cache {
    /// Opens the cache at `path`, creating it if absent.
    ///
    /// A cache written by a different format version is discarded rather than
    /// migrated. A corrupt or unreadable file is also discarded: refusing to
    /// run because a *rebuildable* artefact is damaged would be the wrong
    /// trade for a linter.
    ///
    /// # Errors
    /// [`CacheError::Unopenable`] when even a fresh cache cannot be created,
    /// which means the directory is not writable.
    pub fn open(path: &Utf8Path) -> Result<Self, CacheError> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|error| CacheError::Unopenable {
                path: path.to_owned(),
                source: Box::new(redb::Error::from(redb::StorageError::Io(error))),
            })?;
        }

        let database = if let Some(database) = Self::open_compatible(path) {
            database
        } else {
            // Whatever was there is unusable. Removing it and starting over is
            // always safe for a cache.
            let _ = std::fs::remove_file(path);
            Database::create(path).map_err(|error| CacheError::Unopenable {
                path: path.to_owned(),
                source: Box::new(error.into()),
            })?
        };

        let cache = Self {
            database,
            path: path.to_owned(),
            pending_facts: Vec::new(),
            pending_findings: Vec::new(),
        };
        cache.stamp_version()?;
        Ok(cache)
    }

    /// Opens an existing database if it is readable and the right version.
    fn open_compatible(path: &Utf8Path) -> Option<Database> {
        let database = Database::open(path).ok()?;
        let transaction = database.begin_read().ok()?;

        let stored = match transaction.open_table(META) {
            Ok(table) => table.get(VERSION_KEY).ok()?.map(|value| value.value()),
            // No meta table means a cache from before versioning, which is a
            // shape we cannot read.
            Err(_) => None,
        };

        drop(transaction);
        (stored == Some(FORMAT_VERSION)).then_some(database)
    }

    fn stamp_version(&self) -> Result<(), CacheError> {
        let transaction = self.database.begin_write().map_err(failed)?;
        {
            let mut table = transaction.open_table(META).map_err(failed)?;
            table.insert(VERSION_KEY, FORMAT_VERSION).map_err(failed)?;
        }
        transaction.commit().map_err(failed)?;
        Ok(())
    }

    /// Where this cache lives.
    #[must_use]
    pub fn path(&self) -> &Utf8Path {
        &self.path
    }

    /// Reads cached facts for a file's content, as facts about `path`.
    ///
    /// A miss and a corrupt entry are the same answer: recompute. A cache that
    /// could return a wrong answer would be worse than no cache.
    ///
    /// `path` is a parameter, and that is the whole point of this signature.
    /// The key is a content hash, which cannot answer "which file is this" --
    /// two files with the same bytes are one entry, and a moved file is the
    /// same bytes under a new name. The stored `FileFacts` carries the path of
    /// whichever file was stamped first, and `resolve_imports` reads that field
    /// to know where a relative specifier points from. Handing the caller that
    /// path resolved one file's imports from another file's directory: on a
    /// warm run a real boundary violation went unreported and an innocent file
    /// was flagged, with nothing changed on disk between the two runs. Issue
    /// #20.
    ///
    /// So the caller has to say which file it is asking about, and gets facts
    /// stamped with that. Everything else in here is a function of the bytes
    /// and is shared correctly.
    #[must_use]
    pub fn facts(
        &self,
        content: ContentHash,
        path: &archwarden_core::path::RepoRelPath,
    ) -> Option<FileFacts> {
        let mut facts: FileFacts = self.read(FACTS, content)?;
        facts.path = path.clone();
        Some(facts)
    }

    /// Reads cached findings for a composite key.
    #[must_use]
    pub fn findings(&self, key: ContentHash) -> Option<Vec<Finding>> {
        self.read(FINDINGS, key)
    }

    fn read<T: serde::de::DeserializeOwned>(
        &self,
        definition: TableDefinition<'_, &[u8], &[u8]>,
        key: ContentHash,
    ) -> Option<T> {
        let transaction = self.database.begin_read().ok()?;
        let table = transaction.open_table(definition).ok()?;
        let stored = table.get(key.as_bytes().as_slice()).ok()??;
        rmp_serde::from_slice(stored.value()).ok()
    }

    /// Queues facts to be written.
    ///
    /// Writes are batched because a run touches thousands of files and one
    /// transaction per file would spend the whole budget on `fsync`.
    pub fn put_facts(&mut self, content: ContentHash, facts: &FileFacts) {
        if let Ok(encoded) = rmp_serde::to_vec_named(facts) {
            self.pending_facts.push((content, encoded));
        }
    }

    /// Queues findings to be written.
    pub fn put_findings(&mut self, key: ContentHash, findings: &[Finding]) {
        if let Ok(encoded) = rmp_serde::to_vec_named(findings) {
            self.pending_findings.push((key, encoded));
        }
    }

    /// How many entries are waiting to be written.
    #[must_use]
    pub fn pending(&self) -> usize {
        self.pending_facts.len() + self.pending_findings.len()
    }

    /// Writes everything queued, in one transaction.
    ///
    /// # Errors
    /// [`CacheError::Failed`] when the write does not commit.
    pub fn flush(&mut self) -> Result<(), CacheError> {
        if self.pending() == 0 {
            return Ok(());
        }

        let transaction = self.database.begin_write().map_err(failed)?;
        {
            let mut facts = transaction.open_table(FACTS).map_err(failed)?;
            for (key, value) in &self.pending_facts {
                facts
                    .insert(key.as_bytes().as_slice(), value.as_slice())
                    .map_err(failed)?;
            }

            let mut findings = transaction.open_table(FINDINGS).map_err(failed)?;
            for (key, value) in &self.pending_findings {
                findings
                    .insert(key.as_bytes().as_slice(), value.as_slice())
                    .map_err(failed)?;
            }
        }
        transaction.commit().map_err(failed)?;

        self.pending_facts.clear();
        self.pending_findings.clear();
        Ok(())
    }
}

fn failed(error: impl Into<redb::Error>) -> CacheError {
    CacheError::Failed {
        source: Box::new(error.into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use archwarden_core::{
        facts::{CallFact, ExportFact, ExportKind, ExportTags, Span},
        finding::{Expectation, Finding, Observed},
        ids::RuleId,
        level::Level,
        path::RepoRelPath,
    };

    fn temp() -> (tempfile::TempDir, Utf8PathBuf) {
        let dir = tempfile::tempdir().expect("temp dir");
        let root = Utf8PathBuf::from_path_buf(dir.path().canonicalize().expect("canonicalise"))
            .expect("temp path is UTF-8");
        let path = root.join(".archwarden/cache/db.redb");
        (dir, path)
    }

    /// The path `facts()` is stamped with. Named rather than inlined because
    /// the tests that pass it have a local `path` of their own.
    fn stamped() -> RepoRelPath {
        path("src/user.ts")
    }

    fn path(p: &str) -> RepoRelPath {
        RepoRelPath::new(p).expect("valid path")
    }

    fn facts() -> FileFacts {
        let mut facts = FileFacts::unparsed(path("src/user.ts"), ContentHash::of(b"source"));
        facts.exports.push(ExportFact {
            name: Some("User".to_owned()),
            tags: ExportTags::only(ExportKind::Class),
            is_default: false,
            reexport_from: None,
            forwards: None,
            span: Span::new(0, 20),
        });
        facts.calls.push(CallFact {
            callee: "Event.save".to_owned(),
            span: Span::new(30, 42),
        });
        facts
    }

    fn finding() -> Finding {
        Finding {
            rule_id: RuleId::new("a-rule").expect("valid"),
            module_id: None,
            level: Level::Error,
            path: path("src/user.ts"),
            span: Some(Span::new(0, 4)),
            observed: Observed::ExportMissing {
                name: "User".to_owned(),
            },
            expected: Expectation::FilenamePattern {
                patterns: vec!["^user$".to_owned()],
            },
        }
    }

    #[test]
    fn facts_round_trip_through_the_cache() {
        let (_guard, path) = temp();
        let key = ContentHash::of(b"source");

        let mut cache = Cache::open(&path).expect("opens");
        assert_eq!(cache.facts(key, &stamped()), None, "cold");

        cache.put_facts(key, &facts());
        assert_eq!(cache.pending(), 1);
        assert_eq!(
            cache.facts(key, &stamped()),
            None,
            "queued is not stored until it is flushed"
        );

        cache.flush().expect("flushes");
        assert_eq!(cache.pending(), 0);
        assert_eq!(cache.facts(key, &stamped()), Some(facts()));
    }

    /// The entry is keyed by content, so two files with the same bytes are one
    /// entry -- and a moved file is the same bytes under a new name. Asking
    /// for one and being told about the other is what made a warm run report
    /// a boundary violation that was not there and miss one that was:
    /// `resolve_imports` reads this field to know which directory a relative
    /// specifier points from. Issue #20.
    #[test]
    fn facts_come_back_stamped_with_the_file_that_was_asked_for() {
        let (_guard, directory) = temp();
        let key = ContentHash::of(b"source");

        let mut cache = Cache::open(&directory).expect("opens");
        cache.put_facts(key, &facts());
        cache.flush().expect("flushes");

        let twin = path("packages/domain/twin.ts");
        let read = cache.facts(key, &twin).expect("a hit");

        assert_eq!(read.path, twin, "the file asked about, not the one stored");
        assert_eq!(
            read.exports,
            facts().exports,
            "everything that is a function of the bytes is still shared"
        );
        assert_eq!(read.imports, facts().imports);
        assert_eq!(read.calls, facts().calls);
        assert_eq!(read.content_hash, facts().content_hash);
    }

    #[test]
    fn findings_round_trip_through_the_cache() {
        let (_guard, path) = temp();
        let key = ContentHash::of(b"composite");

        let mut cache = Cache::open(&path).expect("opens");
        cache.put_findings(key, &[finding()]);
        cache.flush().expect("flushes");

        assert_eq!(cache.findings(key), Some(vec![finding()]));
    }

    /// The point of two tables: a rules change invalidates findings and leaves
    /// facts alone, so editing one `level` does not re-parse the repository.
    #[test]
    fn facts_survive_a_findings_key_change() {
        let (_guard, path) = temp();
        let content = ContentHash::of(b"source");
        let old_key = ContentHash::combine(&[content, ContentHash::of(b"rules v1")]);
        let new_key = ContentHash::combine(&[content, ContentHash::of(b"rules v2")]);

        let mut cache = Cache::open(&path).expect("opens");
        cache.put_facts(content, &facts());
        cache.put_findings(old_key, &[finding()]);
        cache.flush().expect("flushes");

        assert_eq!(cache.findings(new_key), None, "the rules changed");
        assert_eq!(
            cache.facts(content, &stamped()),
            Some(facts()),
            "but the file did not, so its facts stand"
        );
    }

    /// The cache is what makes a warm run warm, so it has to survive the
    /// process that wrote it.
    #[test]
    fn a_cache_survives_being_closed_and_reopened() {
        let (_guard, path) = temp();
        let key = ContentHash::of(b"source");

        {
            let mut cache = Cache::open(&path).expect("opens");
            cache.put_facts(key, &facts());
            cache.flush().expect("flushes");
        }

        let reopened = Cache::open(&path).expect("reopens");
        assert_eq!(reopened.facts(key, &stamped()), Some(facts()));
    }

    /// A cache written by a different version is discarded rather than
    /// migrated. Migration code for a rebuildable artefact is a liability
    /// nobody is paid back for.
    #[test]
    fn a_cache_from_another_format_version_is_discarded() {
        let (_guard, path) = temp();
        let key = ContentHash::of(b"source");

        {
            let mut cache = Cache::open(&path).expect("opens");
            cache.put_facts(key, &facts());
            cache.flush().expect("flushes");
        }

        // Stamp a version this build does not understand.
        {
            let database = Database::open(&path).expect("opens");
            let transaction = database.begin_write().expect("write");
            {
                let mut table = transaction.open_table(META).expect("meta");
                table
                    .insert(VERSION_KEY, FORMAT_VERSION + 1)
                    .expect("insert");
            }
            transaction.commit().expect("commit");
        }

        let reopened = Cache::open(&path).expect("reopens");
        assert_eq!(
            reopened.facts(key, &stamped()),
            None,
            "the old entries are gone"
        );
    }

    /// A damaged cache is thrown away rather than fatal. Refusing to lint
    /// because a rebuildable artefact is corrupt would be the wrong trade.
    #[test]
    fn a_corrupt_cache_is_replaced_rather_than_fatal() {
        let (_guard, path) = temp();
        std::fs::create_dir_all(path.parent().expect("has a parent")).expect("create dirs");
        std::fs::write(&path, b"this is not a database").expect("write junk");

        let mut cache = Cache::open(&path).expect("opens over the junk");
        let key = ContentHash::of(b"source");
        cache.put_facts(key, &facts());
        cache.flush().expect("flushes");

        assert_eq!(cache.facts(key, &stamped()), Some(facts()));
    }

    /// A wrong answer from a cache is worse than no cache, so a value that
    /// does not decode is a miss.
    #[test]
    fn an_entry_that_does_not_decode_is_a_miss() {
        let (_guard, path) = temp();
        let key = ContentHash::of(b"source");

        let cache = Cache::open(&path).expect("opens");
        {
            let transaction = cache.database.begin_write().expect("write");
            {
                let mut table = transaction.open_table(FACTS).expect("facts");
                table
                    .insert(key.as_bytes().as_slice(), b"not messagepack".as_slice())
                    .expect("insert");
            }
            transaction.commit().expect("commit");
        }

        assert_eq!(cache.facts(key, &stamped()), None);
    }

    /// The cache directory is created on demand: a fresh checkout has no
    /// `.archwarden/`, and the first run should not have to be told to make
    /// one.
    #[test]
    fn the_cache_directory_is_created_if_absent() {
        let (_guard, path) = temp();
        assert!(!path.exists());

        let cache = Cache::open(&path).expect("opens");
        assert!(path.exists());
        assert_eq!(cache.path(), path);
    }

    #[test]
    fn flushing_nothing_is_not_an_error() {
        let (_guard, path) = temp();
        let mut cache = Cache::open(&path).expect("opens");

        assert_eq!(cache.pending(), 0);
        cache.flush().expect("flushes");
    }

    /// Writes are batched: a run touches thousands of files, and one
    /// transaction each would spend the whole budget on `fsync`.
    #[test]
    fn many_entries_are_written_in_one_flush() {
        let (_guard, path) = temp();
        let mut cache = Cache::open(&path).expect("opens");

        let keys: Vec<_> = (0..500u32)
            .map(|i| ContentHash::of(&i.to_le_bytes()))
            .collect();
        for key in &keys {
            cache.put_facts(*key, &facts());
        }
        assert_eq!(cache.pending(), 500);

        cache.flush().expect("flushes");
        for key in &keys {
            assert_eq!(cache.facts(*key, &stamped()), Some(facts()));
        }
    }

    /// A directory that cannot be written to is a real error, not something to
    /// carry on from silently: the user asked for a cache and is not getting
    /// one.
    #[test]
    fn an_unwritable_location_is_reported() {
        let result = Cache::open(Utf8Path::new("/proc/nonexistent/cache.redb"));
        assert!(matches!(result, Err(CacheError::Unopenable { .. })));
    }
}
