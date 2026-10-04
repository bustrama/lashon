# 42. Rename the product to Ottid

## Status

Accepted — 2026-10-04. Decided by the product owner. The rename itself lands in
its own follow-up PR. Its release is gated on the clearance steps listed under
**Before it ships**.

## Context

*Lashon* (לשון, "tongue" and "language") named a dictation widget drawn as a
tongue of flame. That widget is now a creature that anyone can reshape
([ADR-0040](0040-the-overlay-becomes-a-living-creature.md),
[ADR-0041](0041-user-authored-creatures-are-data.md)). The product also does
more than dictation: command mode, recipes and, next, agents. The owner wants a
name that belongs to the creature.

**Ottid** is *ditto* spelled backwards. *Ditto* is the English word for "the
same again", which suits a creature that takes whatever form you give it. To a
Hebrew ear it also echoes עתיד ("future").

On 2026-10-04 we checked informally whether the name was free. This is not
legal clearance:

| Where | Result |
|---|---|
| `ottid.app`, `ottid.dev`, `ottid.io`, `ottid.ai` | unregistered (RDAP) |
| `ottid.com` | parked and listed for sale |
| `ottid.co.il` | no DNS records |
| npm, crates.io, PyPI | the name is free |
| GitHub organisation `ottid` | taken by a dormant account, so the repo becomes `bustrama/ottid` |
| Web search | no trademark found |

## Decision

- **The name and its forms:**
  - *Ottid* in prose
  - *ottid* in lowercase as the wordmark
  - *אוטיד* in Hebrew

  The default creature is also called Ottid. User-made creatures carry their own
  names in `creature.json`.
- **Everything is renamed, in one pass.** We don't keep two names side by side:
  - the product name (`productName` and the window titles), and every
    user-facing string in English and Hebrew
  - the installer and artifact names
  - the README, the developer docs, the rules and the website
  - the repository name, `bustrama/lashon` → `bustrama/ottid`
  - the app icon: a new one drawn from the creature (our own art), replacing the
    Lashon mark. `--peach` stays the locked brand tone.
  - the internal code names:
    - the crates and binaries: `lashon-core`, `lashon-mcp`, `lashon-recipe`
    - the Python package `lashon_stt`
    - the `LASHON_*` environment variables
    - the recipe schema and the prompt files
    - the sidecar handshake lines and the `x-lashon-auth` metadata key. This is
      a cross-language contract, so the Rust and Python sides change together.
  - **the bundle identifier**, `dev.lashon.desktop` → `app.ottid.desktop`, the
    reverse-DNS form of the recommended domain
- **Existing installs carry over:**
  - On its first launch, the app moves the old identifier's data directories to
    the new ones, but only when the new ones don't exist yet. These are renames
    in the same parent directory, not copies, because the models take several
    gigabytes.
  - The installer removes an old *Lashon* install and keeps its user data, so
    the machine never ends up with two apps.
- **What stays:**
  - git history, and the tags and assets of past releases
  - the wake-word model `hey_lashon`, until a model for the new name is trained.
    It is trained on a spoken phrase, so renaming the file would not change what
    it hears.
- **The public story uses the English word "ditto".** Public material does not
  tie the name to any third-party character or franchise.

## Before it ships

These are the owner's tasks, done before the rename reaches a release:

- Register the domain. `ottid.app` is recommended, and `ottid.co.il` is
  optional.
- Run the official trademark searches (USPTO, EUIPO/TMview and the Israel
  Patent Office) in the relevant classes, at least 9 and 42.
- Get advice from an IP lawyer.

## Consequences

- **MCP hosts need a config change.** Users who wired `lashon-mcp` into an MCP
  host (such as Claude Desktop) have to point it at `ottid-mcp`. The release
  notes say so.
- **Repository URLs keep working.** GitHub redirects git and web URLs for a
  renamed repository, including the updater endpoint
  (`github.com/bustrama/lashon/releases/latest/download/latest.json`). The
  rename PR still points the endpoint at the new name. **Never create a new
  repository called `lashon` under `bustrama`**: doing so cancels the redirect.
- **The Pages site does not redirect.** `bustrama.github.io/lashon/` stops
  resolving after the rename. Links that v1.x builds open there (the wake-word
  tutorial and the MCP guide) will break. The rename release updates those
  links, and the site moves to the custom domain once it is registered, which
  makes future renames safe.
- **The installer upgrades in place.** The rename release is smoke-tested over a
  real v1.1.0 install: the old app must be gone, and its settings, history and
  models must be kept.
- **The wake word.** The old phrase keeps working until the new model lands.
