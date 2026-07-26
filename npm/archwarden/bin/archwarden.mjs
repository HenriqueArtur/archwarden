#!/usr/bin/env node
// Hands control to the platform binary, and gets out of the way.
//
// `stdio: "inherit"` rather than reading the output: the hook protocol is
// stdin-to-stdout, `check` is read by CI, and anything buffered here is
// something this could get wrong. The exit code is the interface -- 0 clean,
// 1 findings, 2 setup -- so it is passed straight through.
import { spawnSync } from "node:child_process";
import { createRequire } from "node:module";
import {
  detectLibc,
  missingPackageMessage,
  packageFor,
  specifierFor,
  unsupportedMessage,
} from "../resolve.mjs";

const { platform, arch } = process;
const libc = platform === "linux" ? detectLibc(process.report?.getReport?.()) : null;

const specifier = specifierFor(platform, arch, libc);
if (!specifier) {
  console.error(unsupportedMessage(platform, arch));
  process.exit(2);
}

let binary;
try {
  binary = createRequire(import.meta.url).resolve(specifier);
} catch {
  console.error(missingPackageMessage(packageFor(platform, arch, libc)));
  process.exit(2);
}

const result = spawnSync(binary, process.argv.slice(2), { stdio: "inherit" });
if (result.error) {
  console.error(`archwarden: could not run ${binary} — ${result.error.message}`);
  process.exit(2);
}
process.exit(result.status ?? 2);
