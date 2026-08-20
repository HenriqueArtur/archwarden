//! Checks about the decisions a config declares.

use archwarden_core::{compiled::CompiledConfig, level::Level};
use camino::Utf8Path;

use archwarden_engine::walk::RepoTree;

use super::Concern;
use super::config::list;

/// Rules that do not say why they exist, counted rather than listed.
///
/// One line, not one per rule: a config with forty rules and no `why` anywhere
/// would otherwise bury every other concern this command has, and burying them
/// is the same as not reporting them.
///
/// And only once at least one rule *does* say why. A project that has never
/// used the field has not adopted the practice, and nagging it about a
/// convention it never chose is how a command that gives advice becomes one
/// people stop running. Once one rule carries a reason, the ones that do not
/// are an inconsistency worth naming. Issue #46.
pub(super) fn reasons_left_unsaid(config: &CompiledConfig, concerns: &mut Vec<Concern>) {
    let (with, without): (Vec<_>, Vec<_>) = config.rules().partition(|rule| rule.why.is_some());

    if with.is_empty() || without.is_empty() {
        return;
    }

    concerns.push(Concern {
        code: "rules-without-a-reason",
        level: Level::Warning,
        rule_id: None,
        path: None,
        message: format!(
            "{} of {} rules say why they exist; {} {} not",
            with.len(),
            with.len() + without.len(),
            without.len(),
            if without.len() == 1 { "does" } else { "do" },
        ),
        fix: "add `why` to them, or accept the gap -- a rule whose reason is \
              nowhere is one a reader can only obey"
            .to_owned(),
    });
}

/// Rules that name no decision, counted rather than listed.
///
/// The same shape as [`reasons_left_unsaid`] one level up, and for the same
/// two reasons. One line, because forty rules naming no decision would bury
/// every other concern this command has. And only once at least one rule
/// *does* name one: every configuration in the world has zero decisions on the
/// day 0.21 ships, and a tool that greets them with a complaint about a
/// feature they have not adopted is one they stop running.
///
/// `check` says nothing about this, deliberately. A repository's build must
/// not fail because its config is under-documented, and a gate that failed for
/// that is a gate somebody turns off. Issue #100.
pub(super) fn decisions_left_unsaid(config: &CompiledConfig, concerns: &mut Vec<Concern>) {
    let (with, without): (Vec<_>, Vec<_>) =
        config.rules().partition(|rule| rule.decision.is_some());

    if with.is_empty() || without.is_empty() {
        return;
    }

    concerns.push(Concern {
        code: "rule-without-a-decision",
        level: Level::Warning,
        rule_id: None,
        path: None,
        message: format!(
            "{} of {} rules name the decision they implement; {} {} not",
            with.len(),
            with.len() + without.len(),
            without.len(),
            if without.len() == 1 { "does" } else { "do" },
        ),
        fix: "add `decision` to them, or accept the gap -- a rule whose \
              decision is nowhere is one a reader can only obey"
            .to_owned(),
    });
}

/// A decision recorded as replaced, with rules still enforcing it.
///
/// The check most worth having here, and the reason `status` is not
/// decoration: this is a config saying two things at once, which is exactly
/// the state `verify-rules` and `coverage` exist to refuse in their own
/// dimensions.
///
/// One concern per decision, naming its rules, because the fix is per decision
/// — either the status is wrong or the rules should go, and both are one
/// edit. `proposed` is silent: a decision under trial with rules already
/// running is how one is trialled. Issue #100.
pub(super) fn superseded_but_still_enforced(config: &CompiledConfig, concerns: &mut Vec<Concern>) {
    for decision in config
        .decisions()
        .filter(|decision| decision.status == archwarden_core::compiled::DecisionStatus::Superseded)
    {
        let serving: Vec<String> = config
            .rules()
            .filter(|rule| rule.decision.as_ref() == Some(&decision.id))
            .map(|rule| rule.id.to_string())
            .collect();

        if serving.is_empty() {
            continue;
        }

        concerns.push(Concern {
            code: "superseded-decision-still-enforced",
            level: Level::Error,
            rule_id: None,
            path: None,
            message: format!(
                "decision `{}` is superseded{}, and {} still {} it: {}",
                decision.id,
                // Naming the replacement is what makes this actionable, and it
                // is only possible now that supersession is an edge rather
                // than a flag. Issue #115.
                match decision.superseded_by.first() {
                    Some(by) => format!(" by `{by}`"),
                    None => String::new(),
                },
                count(serving.len(), "rule"),
                if serving.len() == 1 {
                    "enforces"
                } else {
                    "enforce"
                },
                list(&serving),
            ),
            fix: match decision.superseded_by.first() {
                Some(by) => format!(
                    "point those rules at `{by}`, or the config renamed a \
                     decision rather than replacing it"
                ),
                None => "either the decision still holds, and its status is \
                         wrong, or it does not, and those rules are enforcing \
                         a choice this project has replaced"
                    .to_owned(),
            },
        });
    }
}

/// A decision nothing implements.
///
/// The mirror of `module-nobody-references`, and it arrives the same way: a
/// preset ships decisions, `disable` takes their rules away, and what is left
/// is a config describing an architecture it does not enforce. At `warning`,
/// because writing a decision down before enforcing it is a legitimate order
/// to do things in. Issue #100.
pub(super) fn decision_nobody_enforces(config: &CompiledConfig, concerns: &mut Vec<Concern>) {
    let orphaned: Vec<String> = config
        .decisions()
        // A decision that said no rule can keep it is not one somebody forgot
        // to enforce. Reporting it anyway is what made a repository declaring
        // everything it decided carry a permanent warning per unenforceable
        // decision -- and a warning that never goes away is paid for by the
        // concerns that *are* actionable. Issue #160.
        .filter(|decision| decision.why_not_enforceable.is_none())
        .filter(|decision| {
            !config
                .rules()
                .any(|rule| rule.decision.as_ref() == Some(&decision.id))
        })
        .map(|decision| decision.id.to_string())
        .collect();

    if orphaned.is_empty() {
        return;
    }

    concerns.push(Concern {
        code: "decision-nobody-enforces",
        level: Level::Warning,
        rule_id: None,
        path: None,
        message: format!(
            "{} {} declared and implemented by no rule: {}",
            count(orphaned.len(), "decision"),
            if orphaned.len() == 1 { "is" } else { "are" },
            list(&orphaned),
        ),
        fix: "point a rule at it, or drop it -- a decision nothing enforces is \
              an architecture this config describes rather than keeps"
            .to_owned(),
    });
}

/// `1 rule` / `3 rules`.
pub(super) fn count(n: usize, noun: &str) -> String {
    if n == 1 {
        format!("1 {noun}")
    } else {
        format!("{n} {noun}s")
    }
}

/// A `frozen` rule whose files nothing in the baseline accepts.
///
/// The rule reports **every** file under its scope, which is the design: the
/// baseline holds the accepted set. Turn one on without running `archwarden
/// baseline` and the first `check` is a wall of errors, one per file that was
/// already there — every one of them a finding about the past.
///
/// `check` still reports them, which is honest: the rule really does say those
/// paths are unaccepted. This is where the missing second step is named, with
/// the command to run. At `warning`, like every check that came before the
/// level existed. Issue #102.
/// A decision document that no longer matches the config it came from.
///
/// The generated half of `.archwarden/decisions/<id>.md` is a rendering, and a
/// rendering that has fallen behind is a file telling a reader something the
/// config no longer says. Advice rather than a gate, deliberately: a team
/// adopting this incrementally must not get a red build because a document
/// needs regenerating, and `doctor` is where advice about the configuration
/// already lives. Issue #116.
///
/// Silent when no document exists at all. Not having started is not drift.
pub(super) fn decision_documents_out_of_date(
    root: &Utf8Path,
    config: &CompiledConfig,
    concerns: &mut Vec<Concern>,
) {
    let changes = crate::decisions::changes(root, config);
    if changes.updated.is_empty() {
        return;
    }

    let stale: Vec<String> = changes
        .updated
        .iter()
        .map(std::string::ToString::to_string)
        .collect();

    concerns.push(Concern {
        code: "decision-document-out-of-date",
        level: Level::Warning,
        rule_id: None,
        path: None,
        message: format!(
            "{} no longer {} the config {} came from: {}",
            count(stale.len(), "decision document"),
            if stale.len() == 1 { "matches" } else { "match" },
            if stale.len() == 1 { "it" } else { "they" },
            list(&stale),
        ),
        fix: "run `archwarden decisions` -- what you wrote between the \
              `archwarden:yours` markers is kept"
            .to_owned(),
    });
}

/// A decision claiming no rule can keep it, kept by a rule.
///
/// The claim made checkable, which is what stops `why_not_enforceable` from
/// being documentation. #160 argues that a written-out claim *"is a sentence
/// the next reader can disagree with — and sometimes they will, and a rule
/// will follow"*. If that is the outcome worth designing for, the config
/// should notice when it arrives: what is there now is a rule keeping a
/// decision that says nothing can.
///
/// An error rather than a warning, on the same terms as a `superseded`
/// decision whose rules still fire: the config is saying two things at once,
/// and neither can be acted on until somebody says which.
pub(super) fn unenforceable_but_a_rule_keeps_it(
    config: &CompiledConfig,
    concerns: &mut Vec<Concern>,
) {
    for decision in config.decisions() {
        if decision.why_not_enforceable.is_none() {
            continue;
        }

        let keepers: Vec<String> = config
            .rules()
            .filter(|rule| rule.decision.as_ref() == Some(&decision.id))
            .map(|rule| rule.id.to_string())
            .collect();

        if keepers.is_empty() {
            continue;
        }

        concerns.push(Concern {
            code: "unenforceable-but-a-rule-keeps-it",
            level: Level::Error,
            rule_id: None,
            path: None,
            message: format!(
                "decision `{}` says no rule can keep it, and {} does: {}",
                decision.id,
                if keepers.len() == 1 { "one" } else { "some" },
                list(&keepers),
            ),
            fix: "drop `enforcement` and `why_not_enforceable` -- the rule is \
                  the better answer, and the reason it could not be enforced \
                  has stopped being true"
                .to_owned(),
        });
    }
}

/// A decision whose scope reaches no directory in the repository.
///
/// What #74 gave a module, one level over: a scope matching nothing is a
/// decision about a place that no longer exists, and it will reach nobody
/// through `describe` while looking like it does.
///
/// A warning rather than an error, unlike a rule's empty scope. A rule with no
/// files enforces nothing and that is a hole; a decision with no files is
/// still written down and still true — what it has lost is the way it arrives
/// unprompted, which is worth saying and not worth failing a build over.
pub(super) fn decision_scope_matches_nothing(
    config: &CompiledConfig,
    tree: &RepoTree,
    concerns: &mut Vec<Concern>,
) {
    for decision in config.decisions() {
        let Some(scope) = &decision.scope else {
            continue;
        };
        if tree
            .directories()
            .any(|(path, _)| scope.matches_dir(path.as_path()))
        {
            continue;
        }

        concerns.push(Concern {
            code: "decision-scope-matches-nothing",
            level: Level::Warning,
            rule_id: None,
            path: None,
            message: format!(
                "decision `{}` has a `scope` that matches no directory here, \
                 so `describe` will never bring it to anybody",
                decision.id,
            ),
            fix: "point it at paths that exist, or drop the scope -- a decision \
                  with none is still declared and still reaches whoever asks \
                  for it by id"
                .to_owned(),
        });
    }
}

/// Whether the first decision is a successor of the second.
///
/// Asked of both fields. `supersedes` is what an author writes and
/// `superseded_by` is computed from it, so in a compiled config they agree --
/// but reading only one would make this check depend on which of the two the
/// compiler happened to fill.
fn succeeds(
    decision: &archwarden_core::compiled::CompiledDecision,
    other: &archwarden_core::compiled::CompiledDecision,
) -> bool {
    decision.supersedes.contains(&other.id) || other.superseded_by.contains(&decision.id)
}

/// Two decisions that appear to say the same thing.
///
/// The push half of issue #162, and the valuable half: it catches the
/// duplicate at the moment it is written, where `decisions find` waits to be
/// asked by somebody who already suspects. `doctor` is already in the gate.
///
/// A warning. Two decisions naming the same option is often deliberate -- one
/// supersedes the other, or they are about different scopes -- and only the
/// author can tell. What the concern is for is the case where nobody knew.
pub(super) fn decision_may_duplicate(config: &CompiledConfig, concerns: &mut Vec<Concern>) {
    for duplicate in archwarden_api::similar::duplicates(config) {
        // Superseding is the sanctioned way to say the same thing twice: one
        // decision is *about* the other, and reporting the pair would punish
        // recording the succession.
        //
        // Either direction, because `earlier` and `later` here are declaration
        // order and a config is free to list the superseding decision first --
        // which a config written newest-first does by default.
        if succeeds(duplicate.later, duplicate.earlier)
            || succeeds(duplicate.earlier, duplicate.later)
        {
            continue;
        }

        concerns.push(Concern {
            code: "decision-may-duplicate",
            level: Level::Warning,
            rule_id: None,
            path: None,
            message: format!(
                "`{}` {} and `{}` {} both say `{}`",
                duplicate.later.id,
                duplicate.later_at.path(),
                duplicate.earlier.id,
                duplicate.earlier_at.path(),
                duplicate.text,
            ),
            fix: "if one supersedes the other, say so with `supersedes` -- \
                  otherwise reword whichever is about something else, because \
                  two decisions under one name is two answers to the same \
                  question"
                .to_owned(),
        });
    }
}
