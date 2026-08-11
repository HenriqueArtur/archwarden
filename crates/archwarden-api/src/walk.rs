//! Walk: the repository as a tree of files a rule can be asked about.

use archwarden_core::compiled::CompiledConfig;
use archwarden_engine::walk::RepoTree;
use camino::Utf8Path;

use crate::Error;

/// Walk: reads the repository, and refuses a root nobody chose.
///
/// The walk itself rarely fails. What the second half exists for is the case
/// that succeeds and means nothing: `--config /tmp/stricter.json` takes `/tmp`
/// to be the repository, because a config file's directory is where globs
/// resolve from. It walks `/tmp`, finds no TypeScript, and reports a clean
/// run. No findings — and the question that was asked, *"how many findings
/// would this stricter rule produce?"*, answered with the one wrong answer a
/// reader takes as good news.
///
/// The refusal is narrow on purpose, because "no source files" on its own is a
/// legitimate state: a repository that has just run `archwarden init` is
/// empty, and failing on it would make the tool look broken on its first run.
/// What is never legitimate is an empty root that the caller is not standing
/// in. Standing somewhere is choosing it; a root reached only through a config
/// file's own location was chosen by nobody.
///
/// # Errors
/// [`Error::Walk`], [`Error::RootHoldsNoSource`].
pub fn walk(
    root: &Utf8Path,
    working_directory: &Utf8Path,
    compiled: &CompiledConfig,
) -> Result<RepoTree, Error> {
    let tree = archwarden_engine::walk::walk(root, compiled)?;

    let stood_in = working_directory.starts_with(root);
    let has_source = tree
        .files()
        .any(|file| file.class == archwarden_core::path::FileClass::Source);

    if !stood_in && !has_source {
        return Err(Error::RootHoldsNoSource {
            root: root.to_owned(),
        });
    }

    Ok(tree)
}

#[cfg(test)]
mod tests {
    use crate::{Error, walk};
    use camino::{Utf8Path, Utf8PathBuf};

    /// A structural rule, which needs no file contents — the tree is the whole
    /// input, which is what these tests are about.
    const A_RULE: &str = r#"{"version":0,"rules":[
        {"type":"structure","id":"shape","level":"error",
         "roots":"src/*","allowed_subfolders":["domain"]}]}"#;

    fn repository(files: &[(&str, &str)]) -> (tempfile::TempDir, Utf8PathBuf) {
        let directory = tempfile::tempdir().unwrap();
        let root = Utf8PathBuf::from_path_buf(directory.path().canonicalize().unwrap()).unwrap();

        for (name, contents) in files {
            let path = root.join(name);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(&path, contents).unwrap();
        }

        (directory, root)
    }

    /// How much JavaScript or TypeScript the walk found. Not `files()`, which
    /// also counts `arch.config.json` — the refusal below turns on source, so
    /// the assertions have to as well.
    fn source_files(tree: &archwarden_engine::walk::RepoTree) -> usize {
        tree.files()
            .filter(|file| file.class == archwarden_core::path::FileClass::Source)
            .count()
    }

    fn compiled(root: &Utf8Path) -> archwarden_core::compiled::CompiledConfig {
        crate::prepare(
            crate::Location {
                config: None,
                root: None,
            },
            root,
        )
        .unwrap()
        .compiled
    }

    /// A repository that has just run `archwarden init` has no TypeScript in
    /// it, and walking it is a clean empty run rather than a failure. Exiting
    /// non-zero here would make the tool look broken on its first use.
    #[test]
    fn an_empty_repository_you_are_standing_in_walks_clean() {
        let (_directory, root) = repository(&[("arch.config.json", A_RULE)]);

        let tree = walk(&root, &root, &compiled(&root)).unwrap();

        assert_eq!(source_files(&tree), 0);
    }

    /// The case this refusal exists for. `--config /tmp/stricter.json` takes
    /// `/tmp` to be the repository, because a config file's directory is where
    /// globs resolve from. It walks `/tmp`, finds no TypeScript, and reports a
    /// clean run — answering "how many findings would this stricter rule
    /// produce?" with the one wrong answer a reader takes as good news.
    #[test]
    fn a_root_nobody_chose_and_holding_no_source_is_refused() {
        let (_directory, root) = repository(&[("arch.config.json", A_RULE)]);
        let (_elsewhere, standing_in) = repository(&[]);

        let error = walk(&root, &standing_in, &compiled(&root)).err();

        assert!(matches!(error, Some(Error::RootHoldsNoSource { .. })));
    }

    /// And the refusal is narrow. A root the caller is not standing in is
    /// fine as long as it holds source — that is the legitimate shape of a
    /// config kept outside the repository it describes.
    #[test]
    fn a_root_nobody_chose_but_holding_source_is_walked() {
        let (_directory, root) = repository(&[
            ("arch.config.json", A_RULE),
            ("src/domain/order.ts", "export const order = 1;"),
        ]);
        let (_elsewhere, standing_in) = repository(&[]);

        let tree = walk(&root, &standing_in, &compiled(&root)).unwrap();

        assert_eq!(source_files(&tree), 1);
    }

    /// Standing in a root is choosing it, so an empty one is never refused
    /// however it was reached. This is the pair to the test above: together
    /// they say the refusal turns on *chosen*, not on *empty*.
    #[test]
    fn standing_in_an_empty_root_is_choosing_it() {
        let (_directory, root) = repository(&[("arch.config.json", A_RULE)]);
        let below = root.join("packages/web");
        std::fs::create_dir_all(&below).unwrap();

        assert!(walk(&root, &below, &compiled(&root)).is_ok());
    }

    #[test]
    fn a_root_that_is_not_there_is_a_walk_error() {
        let (_directory, root) = repository(&[("arch.config.json", A_RULE)]);
        let missing = root.join("nowhere");

        let error = walk(&missing, &missing, &compiled(&root)).err();

        assert!(matches!(error, Some(Error::Walk(_))));
    }

    /// The sentence a reader gets, pinned. The help that follows it is the
    /// CLI's, because `--root` is a flag and MCP has no flags; the fact is
    /// this crate's.
    #[test]
    fn the_refusal_says_both_halves_of_why() {
        let (_directory, root) = repository(&[("arch.config.json", A_RULE)]);
        let (_elsewhere, standing_in) = repository(&[]);

        assert_eq!(
            walk(&root, &standing_in, &compiled(&root))
                .err()
                .map(|error| error.to_string()),
            Some(format!(
                "`{root}` holds no JavaScript or TypeScript, and is not where you are standing"
            ))
        );
    }
}
