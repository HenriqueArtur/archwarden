#!/usr/bin/env node
// Hands control to the real binary, and gets out of the way.
//
// `spawnSync` with `stdio: "inherit"` rather than reading the output: the hook
// protocol is stdin-to-stdout, `check` is read by CI, and anything this shim
// buffered would be something it could get wrong.
import { spawnSync } from "node:child_process";
import { existsSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { dirname, join } from "node:path";

const here = dirname(fileURLToPath(import.meta.url));
const binary = join(here, process.platform === "win32" ? "archwarden.exe" : "archwarden");

if (!existsSync(binary)) {
  console.error(
    "archwarden: the binary is not installed. The postinstall step may have " +
      "been skipped or failed; run `npm rebuild @archwarden/cli`, or install " +
      "with `cargo install archwarden-cli`.",
  );
  process.exit(2);
}

const result = spawnSync(binary, process.argv.slice(2), { stdio: "inherit" });
// The exit code is the interface: 0 clean, 1 findings, 2 setup. Losing it
// would turn a failing gate into a passing one.
process.exit(result.status ?? 2);
