//! A committed record of the findings a project has decided to accept.
//!
//! # The problem it exists for
//!
//! A repository adopting archwarden inherits violations nobody has decided
//! about yet. On the first run of a real one: 32 errors and 46 warnings. That
//! leaves two bad choices — keep it out of CI, where the rules rot, or put it
//! in and teach everyone to ignore red. A baseline is the third: green today,
//! red at the next *new* violation.
//!
//! # This is not a filter, and the difference is the point
//!
//! [`crate::filter`] holds one invariant: a filter changes what is printed and
//! never the exit code. That is what makes `--paths` safe to leave in a
//! command that gates a build.
//!
//! A baseline changes the exit code deliberately. It is not a reading
//! preference; it is a decision the project has made, which is why it lives in
//! a committed file rather than in a flag. Adding a line to it is a visible act
//! in a pull request — the thing an auto-created empty spec would have done in
//! silence (decision 13).
//!
//! # What keeps it from becoming a dump
//!
//! Two things, and they are not optional:
//!
//! - The accepted count is on every run's summary line. A baseline nobody is
//!   reminded of is a suppression file.
//! - Entries that no longer occur are counted and named. Without that, fixing
//!   a violation and reintroducing it later would be hidden by the stale entry
//!   — which is exactly the failure a baseline must not have.
//!
//! # What identifies an accepted finding
//!
//! The rule and the path, and nothing else.
//!
//! Not the observed detail: renaming a disallowed folder from `handlers` to
//! `controllers` would otherwise read as a new violation, and that churn is
//! constant. The cost is a case this deliberately does not catch — fixing a
//! violation and breaking differently *at the same path under the same rule*
//! stays accepted. That is rare, and stated rather than hidden.
//!
//! Not the level either: a rule promoted from `warning` to `error` is the same
//! debt, and the project raising its own bar should not have to regenerate.
//!
//! And no timestamp. The project's outputs are byte-stable for a given input,
//! and a date would make every regeneration a diff. Git already records when
//! each line arrived and who wrote it, which is better than a field nobody
//! maintains.

use std::collections::BTreeSet;

use archwarden_core::finding::Finding;
use camino::Utf8Path;
use serde::{Deserialize, Serialize};

/// Where the baseline lives, relative to the repository root.
///
/// Beside the cache but not inside it: `.archwarden/cache/` is gitignored and
/// this file is meant to be committed.
pub const BASELINE_PATH: &str = ".archwarden/baseline.json";

/// The version of the baseline file's shape.
pub const BASELINE_VERSION: u32 = 0;

/// One accepted finding.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Entry {
    /// The rule that fired.
    pub rule: String,
    /// The path it fired on.
    pub path: String,
    /// What it said, for whoever reviews this file.
    ///
    /// Not part of the identity: the file exists to be read in a pull request,
    /// and a reviewer seeing a new line needs to know what is being accepted
    /// without running anything. If the wording of a message ever changes, a
    /// regeneration shows a diff and invalidates nothing.
    #[serde(default)]
    pub note: String,
}

impl Entry {
    /// What makes two entries the same accepted finding.
    fn identity(&self) -> (&str, &str) {
        (self.rule.as_str(), self.path.as_str())
    }
}

/// The accepted findings, as read from or written to disk.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Baseline {
    version: u32,
    /// Sorted, so regenerating an unchanged repository produces the same
    /// bytes and a pull request diff shows only what actually moved.
    accepted: Vec<Entry>,
}

/// How a run's findings compare to what was accepted.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Standing {
    /// How many accepted entries this run still matched.
    pub accepted: usize,
    /// How many accepted entries no longer occur.
    ///
    /// The ratchet, and the only cheerful number archwarden prints: it is the
    /// refactoring that has actually landed.
    pub gone: usize,
}

impl Baseline {
    /// Builds a baseline covering every finding given.
    ///
    /// Deduplicated by identity rather than by whole entry: two findings of one
    /// rule at one path differing only in what they observed are one accepted
    /// thing, and keeping both would put lines in the file that mean the same.
    /// The first note wins, which sorting makes deterministic.
    #[must_use]
    pub fn of(findings: &[Finding]) -> Self {
        let mut entries: Vec<Entry> = findings.iter().map(entry_for).collect();
        entries.sort();
        entries.dedup_by(|a, b| a.identity() == b.identity());

        Self {
            version: BASELINE_VERSION,
            accepted: entries,
        }
    }

    /// Whether this finding has already been accepted.
    #[must_use]
    pub fn accepts(&self, finding: &Finding) -> bool {
        let wanted = (finding.rule_id.as_str(), finding.path.as_str());
        self.accepted.iter().any(|entry| entry.identity() == wanted)
    }

    /// How this run stands against what was accepted.
    #[must_use]
    pub fn standing(&self, findings: &[Finding]) -> Standing {
        let present: BTreeSet<(&str, &str)> = findings
            .iter()
            .map(|finding| (finding.rule_id.as_str(), finding.path.as_str()))
            .collect();

        let accepted = self
            .accepted
            .iter()
            .filter(|entry| present.contains(&entry.identity()))
            .count();

        Standing {
            accepted,
            gone: self.accepted.len() - accepted,
        }
    }

    /// The entries that no longer occur, in file order.
    ///
    /// Named rather than only counted, because "twelve are gone" without
    /// saying which is a number nobody can act on.
    #[must_use]
    pub fn gone<'a>(&'a self, findings: &[Finding]) -> Vec<&'a Entry> {
        let present: BTreeSet<(&str, &str)> = findings
            .iter()
            .map(|finding| (finding.rule_id.as_str(), finding.path.as_str()))
            .collect();

        self.accepted
            .iter()
            .filter(|entry| !present.contains(&entry.identity()))
            .collect()
    }

    /// How many findings are accepted.
    #[must_use]
    pub fn len(&self) -> usize {
        self.accepted.len()
    }

    /// Whether it accepts nothing.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.accepted.is_empty()
    }

    /// Reads the baseline, or `None` when the project has none.
    ///
    /// # Errors
    /// A message naming the problem when the file is there and unreadable.
    /// Absence is not an error -- most projects have none -- but a file that
    /// exists and will not parse must never be treated as an empty one: that
    /// would silently accept nothing and fail a build for reasons the user
    /// cannot see.
    pub fn load(root: &Utf8Path) -> Result<Option<Self>, String> {
        let path = root.join(BASELINE_PATH);
        let text = match std::fs::read_to_string(&path) {
            Ok(text) => text,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(error) => return Err(format!("cannot read `{path}`: {error}")),
        };

        serde_json::from_str(&text)
            .map(Some)
            .map_err(|error| format!("`{path}` is not a valid baseline: {error}"))
    }

    /// Writes the baseline, creating its directory.
    ///
    /// # Errors
    /// A message naming the problem, when the file cannot be written.
    pub fn write(&self, root: &Utf8Path) -> Result<(), String> {
        let path = root.join(BASELINE_PATH);
        if let Some(parent) = path.parent()
            && let Err(error) = std::fs::create_dir_all(parent)
        {
            return Err(format!("cannot create `{parent}`: {error}"));
        }

        let mut rendered = serde_json::to_string_pretty(self)
            .map_err(|error| format!("cannot render the baseline: {error}"))?;
        rendered.push('\n');

        std::fs::write(&path, rendered).map_err(|error| format!("cannot write `{path}`: {error}"))
    }
}

fn entry_for(finding: &Finding) -> Entry {
    Entry {
        rule: finding.rule_id.as_str().to_owned(),
        path: finding.path.as_str().to_owned(),
        note: crate::report::describe_observed(&finding.observed),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use archwarden_core::{
        finding::{Expectation, Observed},
        ids::RuleId,
        level::Level,
        path::RepoRelPath,
    };

    fn finding(rule: &str, path: &str, level: Level, folder: &str) -> Finding {
        Finding {
            rule_id: RuleId::new(rule).expect("valid id"),
            module_id: None,
            level,
            path: RepoRelPath::new(path).expect("valid path"),
            span: None,
            observed: Observed::UnexpectedSubfolder {
                name: folder.to_owned(),
            },
            expected: Expectation::AllowedSubfolders {
                allowed: vec!["types".to_owned()],
                warn: Vec::new(),
            },
        }
    }

    fn debt() -> Vec<Finding> {
        vec![
            finding(
                "shape",
                "packages/domain/src/order",
                Level::Error,
                "handlers",
            ),
            finding("shape", "packages/app/src/billing", Level::Warning, "ctrl"),
        ]
    }

    /// The whole point: yesterday's findings stop failing the build.
    #[test]
    fn what_was_accepted_is_accepted() {
        let baseline = Baseline::of(&debt());

        for finding in &debt() {
            assert!(baseline.accepts(finding), "{finding:?}");
        }
        assert_eq!(baseline.len(), 2);
    }

    /// And tomorrow's do not. A baseline that swallowed a new violation would
    /// be a suppression file with a nicer name.
    #[test]
    fn a_finding_that_was_not_accepted_is_not() {
        let baseline = Baseline::of(&debt());

        let fresh = finding("shape", "packages/app/src/orders", Level::Error, "ctrl");
        assert!(!baseline.accepts(&fresh));

        // Same path, different rule: also new. A rule is what makes a finding
        // mean something.
        let other_rule = finding(
            "spec",
            "packages/domain/src/order",
            Level::Error,
            "handlers",
        );
        assert!(!baseline.accepts(&other_rule));
    }

    /// The level is not part of the identity. A project promoting a rule from
    /// `warning` to `error` is raising its own bar on debt it already
    /// acknowledged, and should not have to regenerate to do it.
    #[test]
    fn promoting_a_rule_does_not_reopen_its_debt() {
        let baseline = Baseline::of(&debt());

        let promoted = finding("shape", "packages/app/src/billing", Level::Error, "ctrl");
        assert!(baseline.accepts(&promoted));
    }

    /// Neither is the observed detail. Renaming a disallowed folder is not a
    /// new violation, and treating it as one would make the file churn on
    /// every rename.
    #[test]
    fn changing_the_detail_at_one_path_stays_accepted() {
        let baseline = Baseline::of(&debt());

        let renamed = finding(
            "shape",
            "packages/domain/src/order",
            Level::Error,
            "controllers",
        );
        assert!(
            baseline.accepts(&renamed),
            "the stated limitation: same rule, same path, different detail"
        );
    }

    /// The ratchet. Fixing a violation is visible, and the entry is reported
    /// as removable -- without which a later reintroduction would be hidden by
    /// the stale entry, which is the one failure a baseline must not have.
    #[test]
    fn a_fixed_violation_is_reported_as_gone() {
        let baseline = Baseline::of(&debt());
        let after = vec![debt()[0].clone()];

        let standing = baseline.standing(&after);
        assert_eq!(standing.accepted, 1);
        assert_eq!(standing.gone, 1);

        let gone = baseline.gone(&after);
        assert_eq!(gone.len(), 1);
        assert_eq!(gone[0].path, "packages/app/src/billing");
    }

    /// A clean run against a full baseline is all ratchet and no debt.
    #[test]
    fn a_repository_that_fixed_everything_says_so() {
        let baseline = Baseline::of(&debt());

        let standing = baseline.standing(&[]);
        assert_eq!(standing.accepted, 0);
        assert_eq!(standing.gone, 2);
    }

    /// An empty baseline is a real state -- `archwarden baseline` on a clean
    /// repository -- and accepts nothing.
    #[test]
    fn an_empty_baseline_accepts_nothing() {
        let baseline = Baseline::of(&[]);

        assert!(baseline.is_empty());
        assert!(!baseline.accepts(&debt()[0]));
        assert_eq!(baseline.standing(&debt()), Standing::default());
    }

    /// Two findings that differ only in what they observed are one accepted
    /// entry, because that is what the identity says. Without the dedup the
    /// file would grow lines that mean the same thing.
    #[test]
    fn one_entry_per_rule_and_path() {
        let baseline = Baseline::of(&[
            finding(
                "shape",
                "packages/domain/src/order",
                Level::Error,
                "handlers",
            ),
            finding("shape", "packages/domain/src/order", Level::Error, "ctrl"),
        ]);

        assert_eq!(baseline.len(), 1);
    }

    /// Byte-stable for a given repository, so regenerating an unchanged one
    /// produces no diff and a pull request shows only what moved.
    #[test]
    fn the_same_findings_give_the_same_file() {
        let one = Baseline::of(&debt());
        let reversed: Vec<Finding> = debt().into_iter().rev().collect();
        let other = Baseline::of(&reversed);

        assert_eq!(
            serde_json::to_string_pretty(&one).expect("renders"),
            serde_json::to_string_pretty(&other).expect("renders"),
        );
    }

    /// It has to survive the round trip it exists for.
    #[test]
    fn it_writes_and_reads_back() {
        let guard = tempfile::tempdir().expect("temp dir");
        let root = camino::Utf8PathBuf::from_path_buf(guard.path().to_path_buf())
            .expect("temp path is UTF-8");

        assert!(
            Baseline::load(&root)
                .expect("absence is not an error")
                .is_none(),
            "most projects have none"
        );

        Baseline::of(&debt()).write(&root).expect("writes");
        let read = Baseline::load(&root).expect("reads").expect("is there");

        assert_eq!(read.len(), 2);
        for finding in &debt() {
            assert!(read.accepts(finding));
        }
        // The note is there for whoever reviews the file.
        assert!(
            read.gone(&[])[0].note.contains("handlers")
                || read.gone(&[])[1].note.contains("handlers"),
            "the reviewer can see what is being accepted"
        );
    }

    /// A file that exists and will not parse is refused. Treating it as empty
    /// would accept nothing and fail the build for a reason nobody can see.
    #[test]
    fn a_broken_baseline_is_refused_not_ignored() {
        let guard = tempfile::tempdir().expect("temp dir");
        let root = camino::Utf8PathBuf::from_path_buf(guard.path().to_path_buf())
            .expect("temp path is UTF-8");
        std::fs::create_dir_all(root.join(".archwarden")).expect("create dirs");
        std::fs::write(root.join(BASELINE_PATH), "{ oops").expect("write");

        let message = Baseline::load(&root).expect_err("not valid");
        assert!(message.contains("is not a valid baseline"), "{message}");
    }

    /// An unknown key is refused for the same reason a config's is: a
    /// misspelled field would be a promise the file silently does not keep.
    #[test]
    fn an_unknown_field_is_refused() {
        let guard = tempfile::tempdir().expect("temp dir");
        let root = camino::Utf8PathBuf::from_path_buf(guard.path().to_path_buf())
            .expect("temp path is UTF-8");
        std::fs::create_dir_all(root.join(".archwarden")).expect("create dirs");
        std::fs::write(
            root.join(BASELINE_PATH),
            r#"{"version":0,"accepted":[],"ignored":[]}"#,
        )
        .expect("write");

        assert!(Baseline::load(&root).is_err());
    }
}
