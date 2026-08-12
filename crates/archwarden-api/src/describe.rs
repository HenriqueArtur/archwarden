//! One sentence for what a rule found.
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
}
