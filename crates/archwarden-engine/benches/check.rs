//! What a run costs, cold and warm.
//!
//! Three measurements, because they answer three different questions:
//!
//! - `walk` — what the tree itself costs, before any rule looks at anything.
//! - `check/cold` — a run with no cache: every source file is read and parsed.
//! - `check/warm` — the same run against a populated cache: every file is read
//!   and hashed, but nothing is parsed.
//!
//! The gap between cold and warm is what the cache buys. `warm` still reads
//! every file, because a content hash is the only honest way to know a file did
//! not change; the cache saves the parse, not the read.
//!
//! Neither figure includes the flush, which happens once per run rather than
//! per file and is measured by nothing here. See `docs/PLAN-V0.md`, M4.

// Benches are not library code: an `expect` that fails here fails the bench,
// which is exactly the desired behaviour.
#![allow(clippy::expect_used, clippy::missing_panics_doc)]

use archwarden_cache::store::Cache;
use archwarden_core::{
    compiled::{CompiledConfig, CompiledRule, CompiledRuleKind, SkipDirs, SkipScope},
    facts::{ExportKind, ExportTags, KindFilter},
    glob::PathSet,
    hash::ContentHash,
    ids::RuleId,
    level::Level,
    pattern::Pattern,
    scope::Scope,
};
use archwarden_engine::{
    run::{self, Run},
    walk::{self, RepoTree},
};
use camino::Utf8PathBuf;
use criterion::{BenchmarkId, Criterion, criterion_group, criterion_main};

/// How many source files the synthetic repository holds.
///
/// Two sizes: one that fits a single package, one that is a large monorepo.
/// The pair is what shows whether the cost is linear.
const SIZES: [usize; 2] = [1_000, 10_000];

/// A synthetic repository: `MODULES` directories of use cases and their specs.
///
/// Deliberately uniform. A benchmark over real TypeScript would be a better
/// estimate of wall-clock and a worse instrument, because every change to the
/// fixture would move the number for reasons unrelated to archwarden.
struct Repository {
    _guard: tempfile::TempDir,
    root: Utf8PathBuf,
    config: CompiledConfig,
    tree: RepoTree,
}

const MODULES: usize = 40;

fn build(files: usize) -> Repository {
    let guard = tempfile::tempdir().expect("temp dir");
    let root = Utf8PathBuf::from_path_buf(guard.path().to_path_buf()).expect("temp path is UTF-8");

    for index in 0..files {
        let module = index % MODULES;
        let directory = root.join(format!("src/module-{module}"));
        std::fs::create_dir_all(&directory).expect("create dirs");
        std::fs::write(
            directory.join(format!("action-{index}.use-case.ts")),
            source(index),
        )
        .expect("write file");
    }

    let config = config();
    let tree = walk::walk(&root, &config).expect("walks");

    Repository {
        _guard: guard,
        root,
        config,
        tree,
    }
}

/// A file with enough in it to be worth parsing: imports, a type, a class and
/// the exported function the rule is looking for.
fn source(index: usize) -> String {
    format!(
        "import {{ Repository }} from '@org/domain';\n\
         import type {{ Logger }} from '@org/logging';\n\
         \n\
         export interface Action{index}Input {{ id: string; count: number; }}\n\
         \n\
         class Helper{index} {{\n\
         \x20 constructor(private readonly logger: Logger) {{}}\n\
         \x20 run(input: Action{index}Input): number {{ return input.count + {index}; }}\n\
         }}\n\
         \n\
         export function Action{index}(input: Action{index}Input): number {{\n\
         \x20 const helper = new Helper{index}(console as unknown as Logger);\n\
         \x20 Repository.save(input);\n\
         \x20 return helper.run(input);\n\
         }}\n"
    )
}

fn config() -> CompiledConfig {
    let rule = CompiledRule {
        id: RuleId::new("usecase-name").expect("valid id"),
        module: None,
        level: Level::Error,
        scope: Scope::compile(["src/*"]).expect("valid scope"),
        kind: CompiledRuleKind::Naming {
            file_pattern: Pattern::compile(r"^(?<name>[a-z0-9-]+)\.use-case\.ts$")
                .expect("valid pattern"),
            dir_pattern: None,
            name_template: "{{pascal(name)}}".to_owned(),
            kind: KindFilter::OneOf(ExportTags::only(ExportKind::Function)),
            annotation: Vec::new(),
            signature_hint: None,
        },
    };

    CompiledConfig::new(
        vec![rule],
        PathSet::default(),
        SkipDirs {
            prefixes: vec!["_".to_owned()],
            globs: PathSet::default(),
            scope: SkipScope::Structure,
        },
        ContentHash::of(b"bench"),
    )
}

fn benchmarks(criterion: &mut Criterion) {
    for files in SIZES {
        let repository = build(files);

        let mut group = criterion.benchmark_group("engine");
        group.sample_size(20);
        group.throughput(criterion::Throughput::Elements(files as u64));

        group.bench_with_input(BenchmarkId::new("walk", files), &files, |bencher, _| {
            bencher.iter(|| walk::walk(&repository.root, &repository.config).expect("walks"));
        });

        group.bench_with_input(
            BenchmarkId::new("check/cold", files),
            &files,
            |bencher, _| {
                bencher.iter(|| {
                    run::check(Run {
                        root: &repository.root,
                        config: &repository.config,
                        tree: &repository.tree,
                        cache: None,
                    })
                });
            },
        );

        // Populated once, outside the loop: a warm run is by definition one
        // whose cache someone else already filled.
        let mut cache =
            Cache::open(&repository.root.join(".archwarden/cache/bench.redb")).expect("opens");
        let _ = run::check(Run {
            root: &repository.root,
            config: &repository.config,
            tree: &repository.tree,
            cache: Some(&mut cache),
        });
        cache.flush().expect("flushes");

        group.bench_with_input(
            BenchmarkId::new("check/warm", files),
            &files,
            |bencher, _| {
                bencher.iter(|| {
                    run::check(Run {
                        root: &repository.root,
                        config: &repository.config,
                        tree: &repository.tree,
                        cache: Some(&mut cache),
                    })
                });
            },
        );

        group.finish();
    }
}

criterion_group!(benches, benchmarks);
criterion_main!(benches);
