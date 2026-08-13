// The programmatic binding: an architecture claim that lives beside the code
// it is about, in the suite a team already runs.
//
// # Why a subprocess and not an N-API module
//
// The CLI already emits versioned JSON, and a subprocess is a smaller promise:
// no native build in the install path, no ABI to match against a Node release,
// and one binary for every consumer. `bin/archwarden.mjs` resolves the same
// platform package this does.
//
// # Why this returns findings instead of asserting
//
// A fluent DSL — `archwarden.noModule("domain").dependsOn("infra")` — would be
// a second way to express a rule, and a second thing that can drift from the
// first. So this returns what was found and the test framework says what it
// thinks about it, in the same output as everything else in the suite.
//
// # Why it reads the repository's own config
//
// Rules inline in a test would be a second place they live, and the one that
// nothing else reads. The test asserts a *subset* of the real configuration:
// `rules` and `paths` narrow what is reported, and the rules themselves stay
// declarative and in one file.
//
// `ROADMAP.md` refuses rules written in JS/TS config files — *"Config is data.
// Executable configs are a bug source and a security concern."* This does not
// cross that line, and it is worth saying because from a distance it looks like
// the same thing: nothing here declares a rule.
import { spawn } from "node:child_process";
import { createRequire } from "node:module";

import { missingPackageMessage, packageFor, specifierFor, unsupportedMessage } from "./resolve.mjs";

/// The JSON report shape this binding understands.
///
/// Checked against what the binary sends on every call. A report from a build
/// whose shape has moved is refused rather than read as though it had not —
/// which is issue #55's defect one layer out: a version nobody checked, parsed
/// into something that looked fine and was not.
export const REPORT_VERSION = 0;

/// Something archwarden could not do, as opposed to something it found.
///
/// The distinction is the whole point. Findings are the answer and come back
/// as data; a broken config, a rule id that does not exist, or a binary that
/// will not start are *not answers*, and a binding that returned an empty
/// findings list for any of them would be a test that passes for the wrong
/// reason.
export class ArchwardenError extends Error {
  constructor(message, { exitCode = null, stderr = "" } = {}) {
    super(message);
    this.name = "ArchwardenError";
    this.exitCode = exitCode;
    this.stderr = stderr;
  }
}

/// Runs `archwarden check` and hands back what it found.
///
/// Options, all optional:
///
/// - `cwd` — where to run, and where the config is discovered from. Defaults
///   to the process's own directory, which in a test run is the repository.
/// - `rules` — only report findings from these rule ids. An id no rule has is
///   an error, because a typo that came back empty would be a test that passes
///   for the wrong reason and goes on passing after the rule is deleted.
/// - `paths` — only report findings under these globs.
/// - `level` — `"error"` or `"warning"`.
/// - `config` — an explicit config path, for asserting against a stricter one
///   than the repository runs in CI.
/// - `binary` — an explicit binary, mostly for this package's own tests.
///
/// Every rule still runs whatever the filters say. They decide what is
/// *reported*, exactly as they do on the command line, so a test that narrows
/// its assertion does not narrow what was checked.
///
/// Rejects with an [`ArchwardenError`] when archwarden could not answer.
/// Findings are never a rejection: a repository that breaks its own rules is
/// the case this is for, and the test decides what that means.
export async function check(options = {}) {
  const { cwd = process.cwd(), rules, paths, level, config, binary } = options;

  const executable = binary ?? locate();
  const args = ["check", "--format", "json"];
  if (rules?.length) args.push("--rules", rules.join(","));
  if (paths?.length) args.push("--paths", paths.join(","));
  if (level) args.push("--level", level);
  if (config) args.push("--config", config);

  const { code, stdout, stderr } = await run(executable, args, cwd);

  // 0 is a clean repository and 1 is a repository with findings. Both are
  // answers. Anything else is archwarden saying it could not run — a missing
  // config, one it cannot read, a rule id that does not exist — and the
  // difference between those two groups is the whole reason this is not just
  // `spawnSync`.
  if (code !== 0 && code !== 1) {
    throw new ArchwardenError(reasonFrom(stderr, code), { exitCode: code, stderr });
  }

  let report;
  try {
    report = JSON.parse(stdout);
  } catch (error) {
    throw new ArchwardenError(
      `archwarden did not emit a JSON report: ${error.message}`,
      { exitCode: code, stderr },
    );
  }

  if (report.version !== REPORT_VERSION) {
    throw new ArchwardenError(
      `this binding reads report version ${REPORT_VERSION}, and archwarden sent ` +
        `version ${report.version}. Upgrade the archwarden package to match the binary.`,
      { exitCode: code, stderr },
    );
  }

  // `findings` is omitted from a summarised report and absent when there are
  // none. An array either way, because a test asserting `deepEqual(findings, [])`
  // should not have to know which.
  return { ...report, findings: report.findings ?? [] };
}

/// The message a caller reads when archwarden refused to run.
///
/// archwarden's own stderr, when there is any: it names the rule id that does
/// not exist, or the config version it cannot read, and a wrapper that replaced
/// it with a sentence of its own would be hiding the useful half.
function reasonFrom(stderr, code) {
  const said = stderr.trim();
  return said.length > 0
    ? said
    : `archwarden exited with code ${code} and said nothing`;
}

/// Where the platform binary is, resolved the same way `bin/archwarden.mjs`
/// resolves it — one answer, so the binding and the command cannot run
/// different builds.
function locate() {
  const { platform, arch } = process;
  const specifier = specifierFor(platform, arch);
  if (!specifier) {
    throw new ArchwardenError(unsupportedMessage(platform, arch));
  }

  try {
    return createRequire(import.meta.url).resolve(specifier);
  } catch {
    throw new ArchwardenError(missingPackageMessage(packageFor(platform, arch)));
  }
}

/// Spawns the binary and collects both streams.
///
/// Not `spawnSync`: a test suite is asynchronous and a synchronous spawn blocks
/// the whole runner, which on a large repository is seconds of a frozen
/// process.
function run(executable, args, cwd) {
  return new Promise((resolve, reject) => {
    const child = spawn(executable, args, { cwd, stdio: ["ignore", "pipe", "pipe"] });

    let stdout = "";
    let stderr = "";
    child.stdout.on("data", (chunk) => {
      stdout += chunk;
    });
    child.stderr.on("data", (chunk) => {
      stderr += chunk;
    });

    child.on("error", (error) => {
      reject(new ArchwardenError(`could not run ${executable} — ${error.message}`));
    });
    child.on("close", (code) => resolve({ code: code ?? 2, stdout, stderr }));
  });
}
