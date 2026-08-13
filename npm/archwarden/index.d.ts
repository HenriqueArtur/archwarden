// Types for the programmatic binding.
//
// Hand-written rather than generated: the module is one function and three
// exports, and a generator would need a build step in a package whose whole
// argument is that it does not have one.
//
// `every_export_is_typed` in `test/check.test.mjs` is what keeps this from
// drifting away from `index.mjs` — the two are checked against each other on
// every run, because a type declaration nobody verifies is worse than none.

/** The JSON report shape this binding understands. */
export const REPORT_VERSION: 0;

/** Something archwarden could not do, as opposed to something it found. */
export class ArchwardenError extends Error {
  name: "ArchwardenError";
  /** The binary's exit code, when it ran at all. */
  exitCode: number | null;
  /** What it wrote to stderr. */
  stderr: string;
}

/** How seriously a rule takes what it found. */
export type Level = "error" | "warning";

/** One thing the rules object to. */
export interface Finding {
  /** The rule that fired. */
  rule_id: string;
  /** The module it belongs to, when it belongs to one. */
  module_id?: string;
  level: Level;
  /** The offending file or directory, repository-relative. */
  path: string;
  /** What was found, in words. */
  said: string;
  /** Why the rule exists, when its author said. */
  why?: string;
  /** Why its module exists, when its author said. */
  module_why?: string;
}

/** The counts beside the findings. */
export interface Summary {
  errors: number;
  warnings: number;
  files: number;
  duration_ms?: number;
}

/** What a run found. */
export interface Report {
  /** Always {@link REPORT_VERSION}; a mismatch is refused before you see it. */
  version: number;
  summary: Summary;
  /** An array even when the report carried none. */
  findings: Finding[];
  /** Files no rule could read, which is not the same as files that passed. */
  unreadable_files?: Array<{ path: string; reason: string }>;
}

/** What to ask, and where. */
export interface CheckOptions {
  /** Where to run, and where the config is discovered from. */
  cwd?: string;
  /**
   * Only report findings from these rule ids.
   *
   * An id no rule has is an error rather than an empty result: a typo that
   * came back clean would be a test that passes for the wrong reason.
   */
  rules?: string[];
  /** Only report findings under these globs. */
  paths?: string[];
  level?: Level;
  /** An explicit config path. */
  config?: string;
  /** An explicit binary. Mostly for this package's own tests. */
  binary?: string;
}

/**
 * Runs `archwarden check` and hands back what it found.
 *
 * Every rule still runs whatever the filters say — they decide what is
 * reported, so narrowing an assertion does not narrow what was checked.
 *
 * Rejects with an {@link ArchwardenError} when archwarden could not answer.
 * Findings are never a rejection.
 */
export function check(options?: CheckOptions): Promise<Report>;
