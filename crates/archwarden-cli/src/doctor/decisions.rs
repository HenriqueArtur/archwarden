//! Checks about the decisions a config declares.

use archwarden_core::{compiled::CompiledConfig, level::Level};
use camino::Utf8Path;

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
