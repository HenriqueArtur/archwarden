//! Reading the repository without changing it.

use archwarden_config::extends::MergedConfig;
use camino::Utf8Path;

use crate::command::{Location, Mode, Output};
use crate::commands::{agent::describe_many, check::walked};
use crate::{diagnostic::ConfigDiagnostic, exit::Exit, report::Format};

/// Loads, merges and compiles a configuration, rendering any failure.
///
/// The orchestration itself lives in [`archwarden_api`] and returns its
/// failures as values. What is left here is the half that is genuinely the
/// CLI's: turning one of those values into a miette report on stderr and exit
/// code 2. That split is issue #63 — before it, the two were one function, and
/// every surface that reports failure differently had to re-implement the path
/// to change the shape of an error rather than reuse it.
///
/// Keeps its tuple return so the eleven callers below are unchanged.
pub(crate) fn prepare(
    location: Location<'_>,
    working_directory: &Utf8Path,
    output: &mut Output<'_>,
) -> Result<(MergedConfig, archwarden_core::compiled::CompiledConfig), Exit> {
    let prepared = archwarden_api::prepare(location, working_directory).map_err(|error| {
        let report = miette::Report::new(ConfigDiagnostic::from_api_error(&error));
        let _ = writeln!(output.err, "{report:?}");
        Exit::ConfigProblem
    })?;

    Ok((prepared.merged, prepared.compiled))
}

/// Says what the rules require of one path.
///
/// Reads no file and parses nothing: every rule's `describe_expectation` is
/// purely lexical, which is what lets this answer about a path that does not
/// exist yet. Exit is clean even when nothing applies -- a query that found no
/// rules is not a failure, and an agent branching on the exit code should see
/// "your setup is wrong" only when it is.
pub(crate) fn describe(
    location: Location<'_>,
    working_directory: &Utf8Path,
    argument: &str,
    format: Format,
    output: &mut Output<'_>,
) -> Exit {
    let (merged, compiled) = match prepare(location, working_directory, output) {
        Ok(prepared) => prepared,
        Err(exit) => return exit,
    };

    // A glob asks about an area rather than a path, which is a different
    // question with a different answer shape. Detected the same way `--paths`
    // does it, so one convention covers both.
    if crate::filter::looks_like_a_glob(argument) {
        return describe_many(
            &merged.root,
            working_directory,
            &compiled,
            argument,
            format,
            output,
        );
    }

    let path = match archwarden_api::describe::repo_relative(
        &merged.root,
        working_directory,
        None,
        argument,
    ) {
        Ok(path) => path,
        Err(message) => {
            let _ = writeln!(output.err, "{message}");
            return Exit::ConfigProblem;
        }
    };

    let applies = archwarden_api::describe::describe(&compiled, &path);
    crate::describe::render(&path, &applies, format, output.out);
    Exit::Clean
}

/// The starter configuration `init` writes.
///
/// No rules. A generated rule is a rule nobody chose, and a linter that starts
/// by reporting things the user never asked for is a linter they turn off. The
/// `$schema` line is the part that earns its place: an editor picks it up and
/// gives completion and, since M7d.1, an error on a misspelled key -- which is
/// why what it points at is decided by [`crate::schema::reference`] rather
/// than being a constant, and why it must be a reference that answers.
pub(crate) fn starter(reference: &str) -> String {
    format!(
        r#"{{
  "$schema": "{reference}",
  "version": 0,
  "rules": []
}}
"#
    )
}

/// Says what moving a file would change.
pub(crate) fn impact(
    location: Location<'_>,
    working_directory: &Utf8Path,
    argument: &str,
    destination: &str,
    mode: Mode,
    format: Format,
    output: &mut Output<'_>,
) -> Exit {
    let Mode { apply, force } = mode;
    let (merged, compiled) = match prepare(location, working_directory, output) {
        Ok(prepared) => prepared,
        Err(exit) => return exit,
    };

    let tree = match walked(&merged.root, working_directory, &compiled, output) {
        Ok(tree) => tree,
        Err(exit) => return exit,
    };

    // A glob or a directory is a batch, and `--to` is then relative to each
    // match. One file keeps the original meaning: `--to` is where it goes.
    let requests = match crate::batch::expand(
        &merged.root,
        working_directory,
        &tree,
        argument,
        destination,
    ) {
        Ok(requests) => requests,
        Err(message) => {
            let _ = writeln!(output.err, "{message}");
            return Exit::ConfigProblem;
        }
    };

    if requests.is_empty() {
        // Never an empty report: a source matching nothing looks exactly like
        // a move with no consequences, which is the one wrong answer a reader
        // takes as good news. The same judgement `--rules` makes about an
        // unknown id.
        let _ = writeln!(output.err, "× `{argument}` matches no file.");
        return Exit::ConfigProblem;
    }

    if apply {
        return carry_out_moves(&merged.root, &compiled, &tree, &requests, force, output);
    }

    let sources: Vec<_> = requests.iter().map(|(from, _)| from.clone()).collect();
    let found =
        archwarden_engine::importers::importers_of_each(&merged.root, &compiled, &tree, &sources);

    let answers: Vec<crate::impact::Impact> = requests
        .iter()
        .map(|(from, to)| {
            let importers = found.get(from).cloned().unwrap_or_default();
            let relative = archwarden_engine::importers::relative_imports(&merged.root, from);
            crate::impact::impact(&compiled, from, to, &importers, relative)
        })
        .collect();

    crate::impact::render_all(&answers, format, output.out);
    Exit::Clean
}

/// Carries out a move, having said what it would do.
///
/// The plan is computed and validated in full before anything is written, so
/// every refusal below happens with the repository untouched.
pub(crate) fn carry_out_moves(
    root: &Utf8Path,
    compiled: &archwarden_core::compiled::CompiledConfig,
    tree: &archwarden_engine::walk::RepoTree,
    requests: &[(
        archwarden_core::path::RepoRelPath,
        archwarden_core::path::RepoRelPath,
    )],
    force: bool,
    output: &mut Output<'_>,
) -> Exit {
    let markers = crate::batch::spec_markers(compiled);
    let plan = crate::apply::plan(root, compiled, tree, requests, &markers);

    if !plan.is_actionable(force) {
        crate::apply::render_refusals(&plan, force, output.err);
        return Exit::ConfigProblem;
    }

    if let Err(message) = crate::apply::carry_out(root, &plan) {
        let _ = writeln!(output.err, "× {message}");
        return Exit::ConfigProblem;
    }

    crate::apply::render_done(&plan, output.out);
    Exit::Clean
}

/// Says where each folder's importers come from.
pub(crate) fn orphans(
    location: Location<'_>,
    working_directory: &Utf8Path,
    scope: Option<&str>,
    by_file: bool,
    format: Format,
    output: &mut Output<'_>,
) -> Exit {
    let (merged, compiled) = match prepare(location, working_directory, output) {
        Ok(prepared) => prepared,
        Err(exit) => return exit,
    };

    let tree = match walked(&merged.root, working_directory, &compiled, output) {
        Ok(tree) => tree,
        Err(exit) => return exit,
    };

    let index = archwarden_engine::importers::reverse_index(&merged.root, &compiled, &tree);
    let mut answer = crate::orphans::orphans(&compiled, &index, by_file);

    if let Some(scope) = scope {
        // The same matcher `--paths` uses, so a plain path selects it and
        // everything under it and a glob is used exactly as written. One
        // convention for narrowing, across every command that narrows.
        let set = match crate::filter::path_set(std::slice::from_ref(&scope.to_owned())) {
            Ok(set) => set,
            Err(message) => {
                let _ = writeln!(output.err, "{message}");
                return Exit::ConfigProblem;
            }
        };
        answer.retain(&set);

        if answer.folders.is_empty() {
            // Never an empty report for a scope that matched nothing: it would
            // read as a repository with no folders worth looking at.
            let _ = writeln!(output.err, "× `{scope}` matches no source file.");
            return Exit::ConfigProblem;
        }
    }

    crate::orphans::render(&answer, by_file, format, output.out);
    Exit::Clean
}
