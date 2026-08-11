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

/// What regenerating the baseline would do to it.
///
/// The count on its own -- "accepting 106 findings" -- is what the command
/// said before, and it cannot answer the question a reviewer has to ask:
/// *did debt get paid, or did debt get added?* Accepting a new finding by
/// accident is permanent and silent, which makes it the worst thing a
/// baseline can do. Issue #23.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Changes<'a> {
    /// Accepted by the new file and not by the old one. The line a reviewer
    /// has to justify.
    pub added: Vec<&'a Entry>,
    /// Accepted by the old file and not by the new one: debt that was paid.
    pub removed: Vec<&'a Entry>,
    /// The same finding at a new path, paired rather than reported twice.
    ///
    /// A move of 724 files turned 41 accepted findings into 41 removals and
    /// 41 additions, none of which was a decision anybody made. Left as two
    /// lists it is 82 lines to read and a script to write; paired, it is one
    /// sentence with a prefix in it.
    pub moved: Vec<Move<'a>>,
}

/// One accepted finding that changed path without changing anything else.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Move<'a> {
    /// Where it was accepted before.
    pub from: &'a Entry,
    /// Where it is accepted now.
    pub to: &'a Entry,
}

impl Changes<'_> {
    /// Whether regenerating would leave the file byte-identical.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.added.is_empty() && self.removed.is_empty() && self.moved.is_empty()
    }
}

/// The directories a move went between, when two paths differ only by a
/// leading prefix.
///
/// The file name has to match, and that is what keeps this from pairing
/// unrelated findings: a move keeps the name and changes the directory. Two
/// findings of one rule at two files that were never the same file share no
/// trailing component, so they are never paired.
fn rename_between(from: &str, to: &str) -> Option<(String, String)> {
    let shared = from
        .split('/')
        .rev()
        .zip(to.split('/').rev())
        .take_while(|(before, after)| before == after)
        .count();

    // No trailing component in common: two different files, not one moved.
    if shared == 0 {
        return None;
    }

    let from_prefix = leading(from, shared);
    let to_prefix = leading(to, shared);
    // Identical prefixes cannot happen for two entries of one rule -- the
    // paths would be equal and neither would be in a change list.
    (from_prefix != to_prefix).then_some((from_prefix, to_prefix))
}

/// `path` with its last `dropped` components removed.
fn leading(path: &str, dropped: usize) -> String {
    let total = path.split('/').count();
    path.split('/')
        .take(total.saturating_sub(dropped))
        .collect::<Vec<_>>()
        .join("/")
}

impl Baseline {
    /// What writing `next` over this baseline would change.
    ///
    /// Moves are paired first, by the prefix mapping that explains the most of
    /// them: a refactor moves many findings the same way.
    ///
    /// A mapping has to explain **at least two** pairs to count as one, and
    /// that threshold is the whole safety of this. Two paths that merely end
    /// in the same component -- `Domain/user/handlers` deleted and
    /// `domain/invoice/handlers` appearing -- describe a prefix mapping too,
    /// and pairing them would report debt paid plus debt added as one
    /// harmless-looking move. That is the failure this command exists to
    /// prevent, arriving through the feature meant to prevent it. A
    /// coincidence between two unrelated paths is possible; the same
    /// coincidence twice, under one mapping, is a directory that moved.
    ///
    /// The cost is that a single file moving on its own reports as one
    /// removal and one addition. That is what the command did before this
    /// existed, it is not wrong, and it is the right direction to fail in:
    /// an addition shown as an addition costs a reader a second look, and an
    /// addition hidden inside a move costs them the review.
    #[must_use]
    pub fn changes<'a>(&'a self, next: &'a Self) -> Changes<'a> {
        let mine: BTreeSet<(&str, &str)> = self.accepted.iter().map(Entry::identity).collect();
        let theirs: BTreeSet<(&str, &str)> = next.accepted.iter().map(Entry::identity).collect();

        let mut removed: Vec<&Entry> = self
            .accepted
            .iter()
            .filter(|entry| !theirs.contains(&entry.identity()))
            .collect();
        let mut added: Vec<&Entry> = next
            .accepted
            .iter()
            .filter(|entry| !mine.contains(&entry.identity()))
            .collect();

        // How many pairs each prefix mapping would explain. A `BTreeMap` so
        // equal votes break by the mapping itself rather than by hash order:
        // the same two files must always produce the same output.
        let mut votes: std::collections::BTreeMap<(String, String), usize> =
            std::collections::BTreeMap::new();
        for gone in &removed {
            for arrived in &added {
                if gone.rule == arrived.rule
                    && let Some(mapping) = rename_between(&gone.path, &arrived.path)
                {
                    *votes.entry(mapping).or_default() += 1;
                }
            }
        }

        // Two pairs or it is not a rename. See the note on this method: one
        // coincidental shared component would otherwise launder a new
        // acceptance into a move.
        let mut ranked: Vec<((String, String), usize)> =
            votes.into_iter().filter(|(_, votes)| *votes >= 2).collect();
        ranked.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));

        let mut moved = Vec::new();
        for (mapping, _) in ranked {
            let mut still_gone = Vec::new();
            for gone in removed {
                let matched = added.iter().position(|arrived| {
                    gone.rule == arrived.rule
                        && rename_between(&gone.path, &arrived.path).as_ref() == Some(&mapping)
                });
                match matched {
                    Some(index) => moved.push(Move {
                        from: gone,
                        to: added.remove(index),
                    }),
                    None => still_gone.push(gone),
                }
            }
            removed = still_gone;
        }

        moved.sort_by(|a, b| (&a.from.rule, &a.from.path).cmp(&(&b.from.rule, &b.from.path)));
        Changes {
            added,
            removed,
            moved,
        }
    }

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

    /// Every accepted entry, in file order.
    ///
    /// For a caller that has to show the file's contents rather than ask it a
    /// question -- `baseline --dry-run` against a repository that has none yet,
    /// where what would be accepted *is* the answer.
    pub fn entries(&self) -> impl Iterator<Item = &Entry> {
        self.accepted.iter()
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
        note: crate::describe::describe_observed(&finding.observed),
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
                patterns: Vec::new(),
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

    /// The question a reviewer has to answer about a regenerated baseline,
    /// and the count could not: was debt paid, or was debt added? Issue #23.
    #[test]
    fn a_dry_run_says_what_was_paid_and_what_would_be_accepted() {
        let committed = Baseline::of(&debt());
        let next = Baseline::of(&[
            // The first is still there.
            debt()[0].clone(),
            // The second was fixed, and something new appeared.
            finding("shape", "packages/app/src/orders", Level::Error, "ctrl"),
        ]);

        let changes = committed.changes(&next);

        assert_eq!(changes.added.len(), 1);
        assert_eq!(changes.added[0].path, "packages/app/src/orders");
        assert_eq!(changes.removed.len(), 1);
        assert_eq!(changes.removed[0].path, "packages/app/src/billing");
        assert!(changes.moved.is_empty(), "nothing moved");
    }

    /// A regeneration that changes nothing says nothing, which is the common
    /// case and the one that should be quiet.
    #[test]
    fn an_unchanged_baseline_has_no_changes() {
        let committed = Baseline::of(&debt());

        assert!(committed.changes(&Baseline::of(&debt())).is_empty());
    }

    /// The case that made the issue: 724 files moved, and 41 accepted
    /// findings turned into 41 removals and 41 additions, none of which was a
    /// decision anybody made. Paired, and by the prefix that explains them.
    #[test]
    fn a_move_is_one_change_rather_than_a_removal_and_an_addition() {
        let committed = Baseline::of(&[
            finding(
                "shape",
                "apps/api/src/Domain/order",
                Level::Error,
                "handlers",
            ),
            finding(
                "shape",
                "apps/api/src/Domain/user",
                Level::Error,
                "handlers",
            ),
        ]);
        let next = Baseline::of(&[
            finding("shape", "packages/domain/order", Level::Error, "handlers"),
            finding("shape", "packages/domain/user", Level::Error, "handlers"),
        ]);

        let changes = committed.changes(&next);

        assert!(changes.added.is_empty(), "{:?}", changes.added);
        assert!(changes.removed.is_empty(), "{:?}", changes.removed);
        assert_eq!(changes.moved.len(), 2);
        assert_eq!(changes.moved[0].from.path, "apps/api/src/Domain/order");
        assert_eq!(changes.moved[0].to.path, "packages/domain/order");
    }

    /// A move and a genuine addition in one regeneration. The pairing must not
    /// swallow the addition -- that is the line the whole feature exists to
    /// put in front of a reviewer.
    #[test]
    fn an_addition_alongside_a_move_is_still_an_addition() {
        let committed = Baseline::of(&[
            finding(
                "shape",
                "apps/api/src/Domain/order",
                Level::Error,
                "handlers",
            ),
            finding(
                "shape",
                "apps/api/src/Domain/user",
                Level::Error,
                "handlers",
            ),
        ]);
        let next = Baseline::of(&[
            finding("shape", "packages/domain/order", Level::Error, "handlers"),
            finding("shape", "packages/domain/user", Level::Error, "handlers"),
            finding("shape", "packages/domain/invoice", Level::Error, "handlers"),
        ]);

        let changes = committed.changes(&next);

        assert_eq!(changes.moved.len(), 2);
        assert_eq!(changes.added.len(), 1, "{:?}", changes.added);
        assert_eq!(changes.added[0].path, "packages/domain/invoice");
        assert!(changes.removed.is_empty());
    }

    /// The other half of the pairing, and the one nothing asserted. A
    /// directory moved *and* a violation elsewhere was fixed, in the same
    /// regeneration. The mapping explains two of the three departures; the
    /// third is a real fix and has to survive the pairing as a removal.
    ///
    /// Losing it would be the cheerful direction to fail in and still wrong:
    /// the count of debt paid is the only encouraging number archwarden has,
    /// and one silently absorbed into a rename is one nobody is told about.
    #[test]
    fn a_fix_alongside_a_move_is_still_a_fix() {
        let committed = Baseline::of(&[
            finding(
                "shape",
                "apps/api/src/Domain/order",
                Level::Error,
                "handlers",
            ),
            finding(
                "shape",
                "apps/api/src/Domain/user",
                Level::Error,
                "handlers",
            ),
            finding("shape", "apps/web/src/legacy", Level::Error, "handlers"),
        ]);
        let next = Baseline::of(&[
            finding("shape", "packages/domain/order", Level::Error, "handlers"),
            finding("shape", "packages/domain/user", Level::Error, "handlers"),
        ]);

        let changes = committed.changes(&next);

        assert_eq!(changes.moved.len(), 2);
        assert!(changes.added.is_empty(), "{:?}", changes.added);
        assert_eq!(changes.removed.len(), 1, "{:?}", changes.removed);
        assert_eq!(changes.removed[0].path, "apps/web/src/legacy");
    }

    /// The file's own contents, for the caller that has to show them rather
    /// than ask a question of them — `baseline --dry-run` against a repository
    /// that has none yet, where what *would* be accepted is the whole answer.
    #[test]
    fn the_accepted_entries_can_be_read_back_in_file_order() {
        let baseline = Baseline::of(&debt());

        let paths: Vec<&str> = baseline
            .entries()
            .map(|entry| entry.path.as_str())
            .collect();

        assert_eq!(paths.len(), debt().len());
        assert!(paths.windows(2).all(|pair| pair[0] <= pair[1]), "{paths:?}");
    }

    /// A baseline that is there and unreadable is refused, and the refusal
    /// names the file. Absence is not an error — most projects have none — but
    /// a file that exists and cannot be read must never be taken for an empty
    /// one: that would accept nothing and fail a build for a reason the user
    /// cannot see.
    #[test]
    fn a_baseline_that_cannot_be_read_is_refused_by_name() {
        let directory = tempfile::tempdir().expect("temp dir");
        let root = camino::Utf8PathBuf::from_path_buf(directory.path().to_path_buf())
            .expect("temp path is UTF-8");
        // A directory where the file goes: present, and not readable as text.
        std::fs::create_dir_all(root.join(BASELINE_PATH)).expect("create");

        let error = Baseline::load(&root).expect_err("a directory is not a baseline");

        assert!(error.starts_with("cannot read `"), "{error}");
        assert!(error.contains(BASELINE_PATH), "{error}");
    }

    /// And a baseline that cannot be written says which directory it could not
    /// make. `baseline` is a command a user runs deliberately; failing it in
    /// silence would leave them believing debt was accepted when nothing was
    /// recorded.
    #[test]
    fn a_baseline_whose_directory_cannot_be_made_is_refused_by_name() {
        let directory = tempfile::tempdir().expect("temp dir");
        let root = camino::Utf8PathBuf::from_path_buf(directory.path().to_path_buf())
            .expect("temp path is UTF-8");
        // A file where the directory goes.
        std::fs::write(root.join(".archwarden"), "not a directory").expect("write");

        let error = Baseline::of(&debt())
            .write(&root)
            .expect_err("a file is not a directory");

        assert!(error.starts_with("cannot create `"), "{error}");
    }

    /// Two directories moved in one regeneration, each explaining two pairs.
    /// With equal votes the order has to come from the mappings themselves,
    /// not from whatever order they were counted in — which is the reason the
    /// votes are held in a `BTreeMap` and then tie-broken explicitly.
    ///
    /// The property is that running this twice on the same input produces the
    /// same output. A `baseline --dry-run` whose report reshuffled between
    /// runs would make every diff of it unreadable, and the instability would
    /// only show up on somebody else's machine.
    #[test]
    fn two_moves_with_equal_votes_are_ordered_the_same_way_every_time() {
        let committed = Baseline::of(&[
            finding("shape", "old/api/order", Level::Error, "handlers"),
            finding("shape", "old/api/user", Level::Error, "handlers"),
            finding("shape", "legacy/web/cart", Level::Error, "handlers"),
            finding("shape", "legacy/web/checkout", Level::Error, "handlers"),
        ]);
        let next = Baseline::of(&[
            finding("shape", "new/api/order", Level::Error, "handlers"),
            finding("shape", "new/api/user", Level::Error, "handlers"),
            finding("shape", "apps/web/cart", Level::Error, "handlers"),
            finding("shape", "apps/web/checkout", Level::Error, "handlers"),
        ]);

        let once = committed.changes(&next);
        let again = committed.changes(&next);

        assert_eq!(once.moved.len(), 4, "{:?}", once.moved);
        assert!(once.added.is_empty(), "{:?}", once.added);
        assert!(once.removed.is_empty(), "{:?}", once.removed);

        let order = |changes: &Changes<'_>| -> Vec<String> {
            changes
                .moved
                .iter()
                .map(|moved| format!("{} -> {}", moved.from.path, moved.to.path))
                .collect()
        };
        assert_eq!(order(&once), order(&again));
    }

    /// And a baseline whose directory exists but whose file cannot be written
    /// says which file. Same reason as the directory case: a `baseline` that
    /// failed in silence leaves a user believing debt was accepted when
    /// nothing was recorded.
    #[test]
    fn a_baseline_that_cannot_be_written_is_refused_by_name() {
        let directory = tempfile::tempdir().expect("temp dir");
        let root = camino::Utf8PathBuf::from_path_buf(directory.path().to_path_buf())
            .expect("temp path is UTF-8");
        // A directory where the file goes: the parent is fine, the write is not.
        std::fs::create_dir_all(root.join(BASELINE_PATH)).expect("create");

        let error = Baseline::of(&debt())
            .write(&root)
            .expect_err("a directory cannot be overwritten with a file");

        assert!(error.starts_with("cannot write `"), "{error}");
        assert!(error.contains(BASELINE_PATH), "{error}");
    }

    /// The trap the pairing itself creates, found by running the command
    /// rather than by reading it. One finding was fixed and a different one
    /// appeared; both paths end in `handlers`, so a prefix mapping exists
    /// between them and pairing it reported debt paid plus **new debt** as one
    /// innocent-looking move. That is the exact failure `--dry-run` was built
    /// to prevent, arriving through the feature meant to prevent it.
    ///
    /// One mapping, one pair, no move. A coincidence between two unrelated
    /// paths is possible; the same coincidence twice is a directory.
    #[test]
    fn a_fix_and_a_new_finding_are_never_laundered_into_a_move() {
        let committed = Baseline::of(&[finding(
            "shape",
            "apps/api/src/Domain/user/handlers",
            Level::Error,
            "x",
        )]);
        let next = Baseline::of(&[finding(
            "shape",
            "packages/domain/invoice/handlers",
            Level::Error,
            "x",
        )]);

        let changes = committed.changes(&next);

        assert!(
            changes.moved.is_empty(),
            "these were never the same finding: {:?}",
            changes.moved
        );
        assert_eq!(changes.added.len(), 1, "the new debt stays visible");
        assert_eq!(changes.removed.len(), 1, "and the paid debt stays visible");
    }

    /// Two findings of one rule at files that were never the same file are not
    /// a move. The trailing component is what says "this is that file
    /// somewhere else", and without it there is nothing to pair on at all --
    /// before the vote threshold is even consulted.
    #[test]
    fn two_different_files_are_not_paired_as_a_move() {
        let committed = Baseline::of(&[finding("shape", "src/a/order", Level::Error, "handlers")]);
        let next = Baseline::of(&[finding("shape", "src/b/invoice", Level::Error, "handlers")]);

        let changes = committed.changes(&next);

        assert!(changes.moved.is_empty());
        assert_eq!(changes.added.len(), 1);
        assert_eq!(changes.removed.len(), 1);
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
