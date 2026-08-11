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
        Observed::RequiredImportMissing => "no import satisfies the requirement".to_owned(),
        Observed::RequiredCallMissing { symbol } => {
            format!("`{symbol}` is imported but never called")
        }
        Observed::RequiredImportForCallMissing { symbol, module } => {
            format!("`{symbol}` is not imported from `{module}`")
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
