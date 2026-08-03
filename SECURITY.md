# Security policy

## Reporting a vulnerability

**Do not open a public issue.**

Use GitHub's private vulnerability reporting — the **Security** tab on this
repository, then **Report a vulnerability**. It is private to the maintainers
until an advisory is published, and it gives us a place to work on a fix with
you.

If that is unavailable to you, email <contato@henriqueartur.com> with
`archwarden security` in the subject.

Please include the version (`archwarden --version`), the platform, and enough
to reproduce. If you have a proof of concept, attach it rather than describing
it — a repository archive that demonstrates the problem is worth more than
prose, and it will not be redistributed.

Expect an acknowledgement within 72 hours. This is a small project, so what
follows depends on the finding, but you will get a real answer rather than
silence: either a fix in progress with a rough timeline, or an explanation of
why we do not consider it a vulnerability. If we disagree, we will say so
plainly and you remain free to disclose.

We will credit you in the advisory unless you ask us not to.

## Supported versions

The latest release only. archwarden is pre-1.0 and there are no maintenance
branches; a fix ships as a new patch version.

| Version | Supported |
| --- | --- |
| latest `0.x` release | yes |
| anything older | no — upgrade |

## Where the risk actually is

archwarden reads a repository and writes a report. That sounds inert, and
mostly is, but three parts of it are not. If you are looking for something,
look here first.

**It runs in privileged places.** archwarden is a CI gate, a `pre-commit` hook
and an agent pre-write hook. In all three it runs automatically, on a developer
machine or a build runner, against code that may have arrived in a pull request
from a stranger. Anything that turns repository *contents* into execution,
resource exhaustion, or a write outside the repository is in scope.

**Config regexes are attacker-controllable in a fork.** This is why the
`regex` crate is a load-bearing choice rather than a preference: it guarantees
linear-time matching, so a catastrophically backtracking pattern cannot be
written into an `arch.config.json`. The cost — no lookaround, no
backreferences — is paid deliberately. A path back to exponential matching is a
denial of service against the user's own commit hook, and we would treat it as
a vulnerability.

**`extends` resolves npm package names.** A config can extend a preset that
lives in `node_modules`, which means config content can arrive through the
dependency tree. Config is data and is never executed — there are no JS or TS
config files, by decision 5, and that is one of the reasons for it. A way to
make config loading execute something, escape the repository root, or read a
file outside it is in scope.

**`impact --apply` writes to your source tree.** It is the only command that
does. Everything is computed and validated before a byte is written, so a
refusal is total and there is no half-applied state. A way to make it write
outside the repository root, follow a symlink out of it, or apply after a
refusal should have fired is in scope.

Also in scope: anything in the distribution. The release binaries, their
`.sha256` files, and the six npm packages. A tampered artifact, a package that
resolves to the wrong binary, or a way to make the wrapper execute something
other than the platform binary it selected.

archwarden contains no `unsafe` — `unsafe_code = "forbid"` at the workspace
level, which cannot be locally overridden — so memory-safety findings would
have to come through a dependency. Those are still worth reporting;
`cargo deny check` runs on every PR against the RustSec advisory database, but
it only knows about advisories that exist.

## What is not a vulnerability

- **A rule that reports something it should not, or misses something it
  should.** That is a correctness bug and belongs in a public issue, where it
  gets fixed faster.
- **A crash on malformed input**, unless it is exploitable beyond stopping the
  run. It is still a bug worth reporting publicly — panics are denied at the
  lint level here precisely because a crash is a bad failure mode for a hook.
- **An advisory against a dependency with no path to archwarden.** Report it
  anyway if you are unsure; we would rather look and say no.
- **Anything requiring an attacker who already has write access to the machine
  running archwarden.** At that point the linter is not the weak part.

## Disclosure

Coordinated. We will agree a date with you, publish a GitHub advisory, ship the
fixed version, and note it in the changelog. If a fix is taking long enough
that users are better served knowing, we will say so before it is ready rather
than sit on it.
