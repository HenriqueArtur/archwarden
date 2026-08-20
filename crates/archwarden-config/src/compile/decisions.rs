//! Decisions: lowering them, reading a status, and refusing a cycle.

use crate::config::{self, Config};

use super::error::CompileError;

pub(super) fn compile_decisions(
    config: &Config,
) -> Result<Vec<archwarden_core::compiled::CompiledDecision>, CompileError> {
    let declared: std::collections::BTreeSet<&str> = config
        .decisions
        .iter()
        .map(|decision| decision.id.as_str())
        .collect();
    let rules: std::collections::BTreeSet<&str> = config
        .rules()
        .map(|(_, _, rule)| rule.id().as_str())
        .collect();

    // Who replaced whom, so the reverse can be read without a second list and
    // so a decision's status can be told from the edge rather than repeated.
    let mut superseded_by: std::collections::BTreeMap<&str, Vec<archwarden_core::ids::DecisionId>> =
        std::collections::BTreeMap::new();
    for decision in &config.decisions {
        for replaced in &decision.supersedes {
            if !declared.contains(replaced.as_str()) {
                return Err(CompileError::UnknownSuperseded {
                    decision: decision.id.clone(),
                    superseded: replaced.clone(),
                });
            }
            superseded_by
                .entry(replaced.as_str())
                .or_default()
                .push(decision.id.clone());
        }
    }
    refuse_supersession_cycles(config)?;

    config
        .decisions
        .iter()
        .map(|decision| {
            let replaced_by = superseded_by
                .get(decision.id.as_str())
                .cloned()
                .unwrap_or_default();

            Ok(archwarden_core::compiled::CompiledDecision {
                id: decision.id.clone(),
                title: decision.title.clone(),
                why: decision.why.clone(),
                link: decision.link.clone(),
                status: status_of(decision, replaced_by.first())?,
                supersedes: decision.supersedes.iter().cloned().collect(),
                superseded_by: replaced_by,
                alternatives: decision
                    .alternatives
                    .iter()
                    .map(|alternative| {
                        if let Some(rule) = &alternative.refused_by
                            && !rules.contains(rule.as_str())
                        {
                            return Err(CompileError::UnknownRefusingRule {
                                decision: decision.id.clone(),
                                option: alternative.option.clone(),
                                rule: rule.clone(),
                            });
                        }
                        Ok(archwarden_core::compiled::CompiledAlternative {
                            option: alternative.option.clone(),
                            why_not: alternative.why_not.clone(),
                            refused_by: alternative.refused_by.clone(),
                        })
                    })
                    .collect::<Result<_, CompileError>>()?,
            })
        })
        .collect()
}

/// A decision's status, with supersession deciding it when there is any.
///
/// Inferred rather than repeated: somebody who writes `supersedes` and forgets
/// to edit the old decision leaves a config that says two things, and disarms
/// `superseded-decision-still-enforced` — which is the check with the most
/// value here. Saying it out loud agrees with the edge and is allowed; saying
/// the opposite is refused, not silently overridden.
pub(super) fn status_of(
    decision: &config::Decision,
    replaced_by: Option<&archwarden_core::ids::DecisionId>,
) -> Result<archwarden_core::compiled::DecisionStatus, CompileError> {
    let written = decision.status.map(|status| match status {
        config::DecisionStatus::Accepted => archwarden_core::compiled::DecisionStatus::Accepted,
        config::DecisionStatus::Proposed => archwarden_core::compiled::DecisionStatus::Proposed,
        config::DecisionStatus::Superseded => archwarden_core::compiled::DecisionStatus::Superseded,
    });

    let Some(by) = replaced_by else {
        return Ok(written.unwrap_or(archwarden_core::compiled::DecisionStatus::Accepted));
    };

    match written {
        None | Some(archwarden_core::compiled::DecisionStatus::Superseded) => {
            Ok(archwarden_core::compiled::DecisionStatus::Superseded)
        }
        Some(other) => Err(CompileError::StatusContradictsSupersession {
            decision: decision.id.clone(),
            status: other.as_str(),
            by: by.clone(),
        }),
    }
}

/// Refuses a decision that replaces itself, directly or around a loop.
///
/// A cycle leaves a chain with no end, and every surface that draws one would
/// walk it forever. Reported with the ids in it, because "there is a cycle" is
/// a sentence nobody can act on.
pub(super) fn refuse_supersession_cycles(config: &Config) -> Result<(), CompileError> {
    let edges: std::collections::BTreeMap<&str, Vec<&str>> = config
        .decisions
        .iter()
        .map(|decision| {
            (
                decision.id.as_str(),
                decision
                    .supersedes
                    .iter()
                    .map(archwarden_core::ids::DecisionId::as_str)
                    .collect(),
            )
        })
        .collect();

    for decision in &config.decisions {
        let start = decision.id.as_str();
        let mut walked = vec![start];
        let mut seen: std::collections::BTreeSet<&str> = [start].into_iter().collect();
        let mut frontier = edges.get(start).cloned().unwrap_or_default();

        while let Some(next) = frontier.pop() {
            if next == start {
                walked.push(start);
                return Err(CompileError::SupersessionCycle {
                    decisions: walked.join(" → "),
                });
            }
            if seen.insert(next) {
                walked.push(next);
                frontier.extend(edges.get(next).cloned().unwrap_or_default());
            }
        }
    }

    Ok(())
}
