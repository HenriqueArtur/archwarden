//! A filesystem that answers *"is there a `foo.ts` here?"* from one directory
//! listing instead of one probe per candidate.
//!
//! # The cost this exists to remove
//!
//! Node resolution is a ladder. `./order` means try `order.ts`, then
//! `order.tsx`, `order.js`, `order.mjs`, `order/index.ts`, and so on until one
//! lands. Every rung that misses is a `statx` that returns nothing.
//!
//! Measured on a synthetic repository of 3 030 files and 15 000 import
//! specifiers — the shape of the one on issue #82 — warm, on a shared mount:
//!
//! | syscall | without boundary rules | with | resolution |
//! |---|---|---|---|
//! | `statx` | 98 | 3 242 *(1 709 failed)* | +3 144 |
//! | `getdents64` | 64 | 128 | +64 |
//!
//! **53% of resolution's `statx` calls fail.** A failed `stat` is the worst
//! thing to do on a filesystem that is really a network: a full round trip
//! that returns nothing. `getdents64`, which answers every rung of the ladder
//! at once, is the cheapest line in the table and barely grows.
//!
//! # What is cached, and what deliberately is not
//!
//! **Absence only.** A name the listing does not hold is reported missing with
//! no syscall. A name it does hold is asked of the filesystem as before,
//! because the answer includes whether it is a file, a directory or a symlink
//! — and a directory entry's own type is unavailable on some filesystems
//! (`DT_UNKNOWN`) and wrong for a symlink on all of them.
//!
//! That is where the cost was. Absence needs no type at all, so the half of
//! the calls that returned nothing now cost nothing.
//!
//! # Why it waits before listing
//!
//! Listing every directory the moment it is asked about was the first version
//! of this, and it barely helped: 186 ms of resolution became 131 ms, where
//! waiting takes it to 58 ms. Listing a directory that is probed once costs
//! more than the probe it replaces, and on a shared mount that cost is the
//! same round trip being avoided elsewhere.
//!
//! So a directory is listed only after `LADDER` probes have already missed
//! in it. That adapts to the access *pattern* rather than to the filesystem,
//! which is what actually decides: one specifier's extension ladder hits the
//! same directory several times, so a directory anything resolves against pays
//! for its listing at once, and a directory probed once never pays at all.
//!
//! # What it costs where it does not help
//!
//! On a local disk this is **slower**, and the issue that asked for it
//! predicted otherwise. Resolution over the same repository:
//!
//! | | before | after | |
//! |---|---|---|---|
//! | shared mount (virtiofs) | 186.4 ms | **58.2 ms** | 3.2x faster |
//! | local disk (ext4) | 13.2 ms | **14.0 ms** | 6% slower |
//!
//! A failed `statx` on ext4 is a page-cache lookup, so the listings buy
//! nothing and their bookkeeping is not free. 0.8 ms against 128 ms is the
//! trade, and it is worth making — but it is a regression on the fast path and
//! is written down as one rather than rounded to "no effect".
//!
//! # Why one run is the right lifetime
//!
//! A persistent resolution cache was refused in `run.rs` for a good reason:
//! resolution depends on files no content hash covers — `tsconfig`, lockfiles
//! — so a stored answer serves a stale path the day somebody edits an alias.
//!
//! None of that applies inside a single run. The walk has already taken its
//! snapshot of the tree before any rule resolves anything, so a file created
//! while the run is in progress is one this run was never going to report on.

use std::{
    collections::{HashMap, HashSet},
    ffi::OsString,
    io,
    path::{Path, PathBuf},
    sync::RwLock,
};

use oxc_resolver::{FileMetadata, FileSystem, FileSystemOs};

/// Directory listings, taken once each and held for one run.
///
/// `RwLock` rather than `RefCell` because [`FileSystem`] is `Send + Sync`.
/// Resolution is single-threaded today and the lock costs nothing measurable;
/// the alternative is a trait bound this cannot satisfy.
#[derive(Debug, Default)]
pub struct Listings {
    /// What each directory holds, by name. Absent from the map means "not
    /// listed yet"; an empty set means listed and holding nothing, which is
    /// also what an unreadable directory looks like — for the only question
    /// asked here, "is this name present", the two are the same answer.
    seen: RwLock<HashMap<PathBuf, HashSet<OsString>>>,
    /// Probes that have already missed in each directory, until it is listed.
    misses: RwLock<HashMap<PathBuf, usize>>,
    /// How many directories were listed, for the test that pins the
    /// arithmetic this rests on.
    taken: RwLock<usize>,
}

/// How many probes must miss in one directory before it is worth listing.
///
/// Two, because the extension ladder for a single specifier tries at least
/// `.ts`, `.tsx`, `.js` and `/index.ts` in the same directory: a directory any
/// import resolves against pays for its listing on the first specifier, and
/// one probed once never pays.
///
/// Measured at 2, 4, 8, 16, 32 and 64 against both filesystems. Everything
/// from 2 to 16 lands in the same band on a shared mount — 56 to 75 ms of
/// resolution against 162 before — and 32 upwards loses the effect entirely,
/// because a directory is abandoned before it is ever listed. On a local disk
/// the cost rises monotonically with the threshold, 13.6 ms at 2 against
/// 15.4 ms at 64. So the low end of the band that works is the whole
/// argument.
const LADDER: usize = 2;

impl Listings {
    /// How many directories have been listed.
    #[must_use]
    pub fn listings_taken(&self) -> usize {
        self.taken.read().map_or(0, |taken| *taken)
    }

    /// Whether a listing already answers this path, and what it says.
    ///
    /// `None` means "no listing covers this, ask the filesystem". A wrong
    /// `Some(true)` costs nothing — the syscall still runs — but a wrong
    /// `Some(false)` would report a file that exists as missing, which turns a
    /// resolved import into an unresolved one and a boundary rule into a blind
    /// spot. So every uncertainty here answers `None`: no parent, a poisoned
    /// lock, a directory nothing has listed.
    fn known(&self, path: &Path) -> Option<bool> {
        let (parent, name) = (path.parent()?, path.file_name()?);
        let seen = self.seen.read().ok()?;
        seen.get(parent).map(|names| names.contains(name))
    }

    /// Records a probe that found nothing, and lists the directory once enough
    /// of them have.
    fn missed(&self, path: &Path) {
        let Some(parent) = path.parent() else { return };

        let due = match self.misses.write() {
            Ok(mut misses) => {
                let count = misses.entry(parent.to_owned()).or_default();
                *count += 1;
                *count >= LADDER
            }
            Err(_) => false,
        };
        if !due {
            return;
        }
        if self.seen.read().is_ok_and(|seen| seen.contains_key(parent)) {
            return;
        }

        let names: HashSet<OsString> = std::fs::read_dir(parent)
            .map(|entries| {
                entries
                    .filter_map(Result::ok)
                    .map(|entry| entry.file_name())
                    .collect()
            })
            .unwrap_or_default();

        if let Ok(mut seen) = self.seen.write() {
            seen.insert(parent.to_owned(), names);
        }
        if let Ok(mut taken) = self.taken.write() {
            *taken += 1;
        }
    }

    /// One existence question, answered from a listing when there is one and
    /// from the filesystem otherwise.
    fn ask(
        &self,
        path: &Path,
        of_filesystem: impl Fn(&Path) -> io::Result<FileMetadata>,
    ) -> io::Result<FileMetadata> {
        if self.known(path) == Some(false) {
            return Err(absent(path));
        }

        let answer = of_filesystem(path);
        if answer.is_err() {
            self.missed(path);
        }
        answer
    }
}

/// What the resolver is told about a name no directory holds.
fn absent(path: &Path) -> io::Error {
    io::Error::new(
        io::ErrorKind::NotFound,
        format!("no such file or directory: {}", path.display()),
    )
}

impl FileSystem for Listings {
    fn new() -> Self {
        Self::default()
    }

    fn read(&self, path: &Path) -> io::Result<Vec<u8>> {
        FileSystemOs.read(path)
    }

    fn read_to_string(&self, path: &Path) -> io::Result<String> {
        FileSystemOs.read_to_string(path)
    }

    fn metadata(&self, path: &Path) -> io::Result<FileMetadata> {
        self.ask(path, |at| FileSystemOs.metadata(at))
    }

    fn symlink_metadata(&self, path: &Path) -> io::Result<FileMetadata> {
        self.ask(path, |at| FileSystemOs.symlink_metadata(at))
    }

    fn read_link(&self, path: &Path) -> Result<PathBuf, oxc_resolver::ResolveError> {
        FileSystemOs.read_link(path)
    }

    fn canonicalize(&self, path: &Path) -> io::Result<PathBuf> {
        FileSystemOs.canonicalize(path)
    }
}

#[cfg(test)]
mod tests {
    use super::Listings;
    use oxc_resolver::FileSystem;
    use std::io::ErrorKind;

    fn repository(files: &[&str]) -> (tempfile::TempDir, std::path::PathBuf) {
        let directory = tempfile::tempdir().expect("temp dir");
        let root = directory.path().to_path_buf();
        for name in files {
            let path = root.join(name);
            std::fs::create_dir_all(path.parent().expect("a file has a parent")).expect("dirs");
            std::fs::write(&path, "export const x = 1;\n").expect("write");
        }
        (directory, root)
    }

    /// The one that matters. `./order` makes the resolver try `order.ts`,
    /// `order.tsx`, `order.js`, `order/index.ts` and more, and every miss is a
    /// full round trip that returns nothing. One listing answers all of them.
    #[test]
    fn a_name_the_directory_does_not_hold_is_absent() {
        let (_guard, root) = repository(&["src/order.ts"]);
        let fs = Listings::default();

        assert!(fs.metadata(&root.join("src/order.ts")).is_ok());
        assert_eq!(
            fs.metadata(&root.join("src/order.tsx")).unwrap_err().kind(),
            ErrorKind::NotFound
        );
        assert_eq!(
            fs.metadata(&root.join("src/order.js")).unwrap_err().kind(),
            ErrorKind::NotFound
        );
    }

    /// Once a directory has been listed the answer comes from the listing, and
    /// that is also the contract: **this is a cache for the duration of one
    /// run.** A file appearing under a directory already listed stays
    /// invisible until the next run.
    ///
    /// That is safe here for the reason a *persistent* resolution cache was
    /// not (see the note on `run.rs`): the walk has already taken its snapshot
    /// of the tree before any rule resolves anything, so a file created
    /// mid-run is one this run was never going to report on anyway.
    #[test]
    fn a_file_that_appears_after_the_listing_is_not_seen_by_this_run() {
        let (_guard, root) = repository(&["src/order.ts"]);
        let fs = Listings::default();

        // Two misses is what it takes; the second one lists the directory.
        assert!(fs.metadata(&root.join("src/order.tsx")).is_err());
        assert!(fs.metadata(&root.join("src/order.js")).is_err());
        assert_eq!(fs.listings_taken(), 1, "listed once the ladder had missed");

        std::fs::write(root.join("src/order.mjs"), "later\n").expect("write");

        assert_eq!(
            fs.metadata(&root.join("src/order.mjs")).unwrap_err().kind(),
            ErrorKind::NotFound,
            "the listing was taken before the file existed"
        );
    }

    /// And a directory probed only once is never listed at all, which is what
    /// keeps a local disk from paying for a listing it does not need.
    #[test]
    fn a_directory_probed_once_is_never_listed() {
        let (_guard, root) = repository(&["src/order.ts"]);
        let fs = Listings::default();

        assert!(fs.metadata(&root.join("src/nowhere.ts")).is_err());

        assert_eq!(fs.listings_taken(), 0);
    }

    /// Presence is *not* answered from the listing: a name that is there still
    /// costs its `statx`, because the answer includes whether it is a file, a
    /// directory or a symlink, and a directory entry's own type is unavailable
    /// on some filesystems and wrong for a symlink on all of them.
    ///
    /// Absence is where the cost was — 53% of resolution's calls in the
    /// measurement on issue #82 — and absence needs no type at all.
    #[test]
    fn what_is_there_is_still_described_correctly() {
        let (_guard, root) = repository(&["src/order.ts"]);
        std::fs::create_dir_all(root.join("src/nested")).expect("dirs");

        let fs = Listings::default();

        let file = fs
            .metadata(&root.join("src/order.ts"))
            .expect("it is there");
        assert!(file.is_file() && !file.is_dir());

        let directory = fs.metadata(&root.join("src/nested")).expect("it is there");
        assert!(directory.is_dir() && !directory.is_file());
    }

    /// A directory that is not there makes every name under it absent, and
    /// costs one failed listing rather than one failed probe per candidate.
    #[test]
    fn nothing_is_under_a_directory_that_does_not_exist() {
        let (_guard, root) = repository(&["src/order.ts"]);
        let fs = Listings::default();

        for name in ["a.ts", "b.ts", "index.ts"] {
            assert_eq!(
                fs.metadata(&root.join("nowhere").join(name))
                    .unwrap_err()
                    .kind(),
                ErrorKind::NotFound
            );
        }
    }

    /// `symlink_metadata` gets the same shortcut, and must: the resolver uses
    /// it to decide whether to follow a link, and a name that is not there is
    /// not a link either.
    #[test]
    fn the_shortcut_applies_to_symlink_metadata_too() {
        let (_guard, root) = repository(&["src/order.ts"]);
        let fs = Listings::default();

        assert!(fs.symlink_metadata(&root.join("src/order.ts")).is_ok());
        assert_eq!(
            fs.symlink_metadata(&root.join("src/order.tsx"))
                .unwrap_err()
                .kind(),
            ErrorKind::NotFound
        );
    }

    /// A path with no parent has no listing to consult and is asked of the
    /// filesystem, rather than being reported absent.
    #[test]
    fn a_path_with_no_parent_is_asked_of_the_filesystem() {
        let fs = Listings::default();

        assert!(fs.metadata(std::path::Path::new("/")).is_ok());
    }

    /// Everything that is not an existence question is the operating system's,
    /// unchanged. Caching file *contents* is what the facts cache already
    /// does, keyed on a hash; a second copy here would be a second thing to
    /// invalidate.
    #[test]
    fn reading_is_left_to_the_operating_system() {
        let (_guard, root) = repository(&["src/order.ts"]);
        let fs = Listings::default();

        assert_eq!(
            fs.read_to_string(&root.join("src/order.ts"))
                .expect("reads"),
            "export const x = 1;\n"
        );
        assert!(fs.read(&root.join("src/order.ts")).is_ok());
        assert!(fs.canonicalize(&root.join("src/order.ts")).is_ok());
    }

    /// The listing is taken once per directory however many names are asked
    /// of it, which is the arithmetic the whole thing rests on.
    #[test]
    fn one_directory_is_listed_once_however_many_names_are_asked() {
        let (_guard, root) = repository(&["src/order.ts", "src/client.ts"]);
        let fs = Listings::default();

        for name in ["a.ts", "b.ts", "c.ts", "d.tsx", "e.js"] {
            let _ = fs.metadata(&root.join("src").join(name));
        }

        assert_eq!(fs.listings_taken(), 1);
    }
}
