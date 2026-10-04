# 43. Sign Windows releases with Azure Artifact Signing

## Status

Proposed — 2026-10-04. It becomes Accepted when the owner merges it. Signing
goes live once Microsoft completes the owner's identity validation (see
**What the owner does**).

It replaces the signing route of [ADR-0032](0032-ship-as-open-core-product.md)
and [ADR-0033](0033-focus-on-windows-for-v1.md) (SignPath Foundation). The first
signed release closes the unsigned exception of
[ADR-0039](0039-ship-v1-unsigned-windows-only.md).

## Context

`.claude/rules/security.md` says never ship a release installer without code
signing. v0.1.0, v1.0.0 and v1.1.0 shipped unsigned as recorded exceptions
([ADR-0006](0006-release-packaging-and-signing.md), ADR-0039). The paid build
of the open-core plan (ADR-0032) cannot ship unsigned at all.

**What has to be signed is every PE image we ship**, not just the installer:

- SmartScreen checks only the file that was downloaded: the installer, or the
  exe extracted from the portable zip.
- Smart App Control checks every module the loader maps: exes, DLLs, and
  Python's `.pyd` extension modules. New Windows 11 installs start it in
  evaluation mode, and it may then switch on. An unsigned module that
  Microsoft's cloud doesn't already know is blocked.

A free build has about 175 PE images, and about 130 of them need our
signature. The frozen STT sidecar alone has 171, of which 51 already carry a
Microsoft, NVIDIA or Intel signature. The other 120 are unsigned, and 86 of
those are `.pyd` modules. The full edition adds `llama-server` and its DLLs.

Three facts of 2026 frame the choice:

- Since June 2023, code-signing keys must live in an HSM. A CI build therefore
  signs through a cloud service; there is no `.pfx` to drop into a secret.
- Since 2024, EV certificates no longer get instant SmartScreen reputation.
  OV, EV and Artifact Signing all build reputation the same way: over time,
  for a publisher that keeps signing with the same identity.
- Since 2026-03-01, a code-signing certificate lasts at most 460 days
  (CA/B Forum ballot CSC-31).

The publisher is an individual in Israel who sells a build of the same GPL
source.

## Options (researched 2026-10-04)

| Option | Cost | Identity checks | Time to first signature | CI | Verdict |
|---|---|---|---|---|---|
| **Azure Artifact Signing** (formerly Trusted Signing) | $9.99/month (Basic: 5,000 signatures/month), on a paid Azure subscription | Organizations only, outside the US and Canada. **Israel has been listed since 2026-07-23.** Needs: legal name, business ID, address, a website and a mailbox on the business's own domain, documents under 12 months old, and an ID-plus-face check of the representative (AU10TIX). Individual developers: US/Canada only. | 1–20 business days | signtool + Microsoft's dlib. GitHub OIDC, so no secret. Certificates last 3 days, so a timestamp is mandatory. | **Chosen** |
| **SignPath Foundation** | Free | None personal. Needs an OSI license "without commercial dual-licensing". | Days, after approval | Their GitHub action. Manual approval per release. | No. The certificate names "SignPath Foundation" as the publisher of a product we sell. Only our own artifacts may be signed, so upstream modules (llama-server, the sidecar's DLLs) stay unsigned, which fails Smart App Control. Selling builds isn't addressed in their terms. |
| **SSL.com IV + eSigner** | Certificate about $129/year. eSigner: $180/year for 240 signings (Tier 1), $765/year for 1,200 (Tier 2), overage $1.00–$0.25 per signing. | Individual: government photo ID and a selfie | Days | CodeSignTool with a TOTP secret (officially supported), plus a GitHub action | **Fallback.** Confirmed for individuals, but with about 130 signatures a build, Tier 1 lasts one or two releases. Realistically about $900/year. The password and TOTP seed in GitHub would also be a portable signing credential. |
| **Certum** | Open Source certificate from €49, but Certum revokes it "if … used to sign software distributed commercially". Standard Code Signing in the cloud (natural person) from €209/year, with 5,000 signatures/month. | ID verification and a utility bill | 3+ business days | SimplySign needs a phone OTP per session. It is unattended only through unofficial tools. | No. The cheap certificate forbids our use, and the other one doesn't run in CI. |
| **OV certificate in our own cloud HSM** (Azure Key Vault + DigiCert/GlobalSign), or DigiCert KeyLocker | About $450–1,000/year | Mostly sold to organizations | Days | AzureSignTool / KeyLocker tools | No. It costs more and gives no SmartScreen advantage. |
| **EV certificate** | $359+/year (SSL.com sole-proprietor EV) | Notarized forms and a business listing | Days–weeks | Cloud HSM | No. No SmartScreen benefit since 2024. |
| **Microsoft Store (MSIX)** | Free; Microsoft re-signs the package | Free individual account | — | — | Not now. It only covers Store-delivered MSIX packages. Ottid ships an NSIS installer by direct download (GitHub, the paid feed), which we would still have to sign ourselves. |

Prices are list prices from the vendors' pages as of 2026-10. SSL.com's
certificate price is from resellers.

## Decision

**Sign with Azure Artifact Signing**: the Basic tier, a Public Trust
certificate profile, validated as an organization — the owner's registered
Israeli business (עוסק). Selling the paid build needs that registration
anyway.

- **The cheapest path, by far:** about $120 a year. Its 5,000 signatures a
  month cover every PE image of about 38 builds a month, so nothing has to be
  skipped to save quota.
- **No signing secret anywhere.** GitHub OIDC logs the workflow in to Azure.
  Azure trusts only the `release` environment of `bustrama/ottid`, so a leaked
  repository secret cannot sign anything.
- **Microsoft's own tooling**, on Windows' own trust root.
- **One identity signs both editions**, so SmartScreen reputation goes to one
  publisher.

**The risk:** Microsoft has not said whether an Israeli sole proprietorship
counts as an organization. Some EU sole proprietors report a failed validation
without a reason. Trying costs a month's fee and a few hours. **If Microsoft
rejects the validation, the fallback is SSL.com IV + eSigner.** That would be a
new ADR, and only the signing call inside `scripts/sign-windows.ps1` changes.

A second, smaller risk: since Microsoft rotated its intermediate CAs in March
2026, some Artifact Signing publishers report SmartScreen warnings on each new
release. Every option builds reputation over time. A warning in a release's
first days is expected, and the release notes say so.

### What gets signed, and by what

| PE image | Signed by |
|---|---|
| Sidecar `ottid-stt.exe` and every unsigned `.dll` / `.pyd` under `_internal/` | `scripts/sign-windows.ps1 -Tree`, before `tauri build` |
| `llama-server.exe` and its DLLs (full edition) | The same step, whenever they are staged |
| `ottid.exe` inside the installer | Tauri's `signCommand` |
| The NSIS plugin DLLs, the installer | Tauri's `signCommand` |
| The uninstaller | Tauri's `signCommand`, from inside makensis (`!uninstfinalize`) |
| `ottid.exe` in the portable zip | `release.yml`, on the staged copy |

Modules that already carry a valid signature (Microsoft, NVIDIA, Intel) are
left as they are. Third-party modules without one (llama.cpp, the Python
extension modules) are signed with our certificate. That says we vouch for the
exact files we ship, which is what Smart App Control asks of a publisher.

### How the build signs (Tauri 2.11.2)

- **The signing config is merged only in CI.** `tauri.signing.conf.json` sets
  `bundle.windows.signCommand` to the signing script. `release.yml` passes it
  with `--config`, so local builds and anyone building the GPL source need no
  signing tool.
- **The tools are pinned.** signtool and the dlib come from nuget.org
  (`Microsoft.Windows.SDK.BuildTools` 10.0.28000.2705,
  `Microsoft.ArtifactSigning.Client` 1.0.128). Each is pinned to its version and
  to the SHA-512 nuget.org records for it. Files get a SHA-256 digest and an
  RFC 3161 timestamp from `http://timestamp.acs.microsoft.com`.
- **The dlib authenticates through the Azure CLI's session only.** In CI,
  `azure/login` opens it over OIDC. The login hands over an assertion that lives
  for minutes; the access token it buys lives about an hour, cached for every
  signtool process. So the workflow fetches that token immediately before the
  signing steps.
- **CI checks the result.** It silently installs the finished installer into a
  scratch folder and fails unless every PE image there is validly signed. That
  includes the uninstaller, which NSIS writes only at install time. The portable
  tree is checked too.

What we read in the Tauri source (tag `tauri-cli-v2.11.2`) shaped this:

- The bundler signs resources too, but only `*.exe` / `*.dll` that aren't
  signed yet. It signs them in place, one process per file. `.pyd` modules are
  skipped, hence our own pass over the tree.
- After bundling, it restores `target/release/ottid.exe` to the *unsigned*
  build. The portable zip therefore signs its own copy.
- `--no-sign` skips both the Authenticode signatures and the updater's minisign
  `.sig`.
- A failing sign command fails the build.
- The updater `.sig` is computed after `bundle_project` returns, so it covers
  the final, Authenticode-signed installer.

### The release gate

- **Where the configuration lives.** The signing secrets and variables live in
  a GitHub environment, `release`, whose deployment rules admit tags `v*` (and
  `main`, for manual runs). Azure's federated credential trusts only that
  environment's OIDC subject, `repo:bustrama/ottid:environment:release`.
- **When a run signs.** A run signs when both signatures are configured:
  Authenticode, and the updater's minisign key. If either is missing, the run
  fails before the build starts.
- **The only way to build unsigned.** The one exception is an explicitly
  unsigned pre-release: a tag containing `-unsigned` (`v1.2.0-unsigned.1`), or a
  manual run with `allow_unsigned` ticked. Such a build passes `--no-sign` and
  is released as a GitHub pre-release, which keeps it out of
  `/releases/latest`, the updater's feed. Its release notes say it is unsigned.
- **Nothing is published from CI.** The run creates a draft. The owner
  smoke-tests a real install of the draft's installer, then publishes it by
  hand. A draft from a red run is never published.

### The updater

- **The minisign key is independent of Authenticode, and a release needs both.**
  The public key is committed in `tauri.conf.json` (key ID
  `DC3C2A529F34339A`), and every v1.x install carries it.
- **v1.x installs can update in-app.** Signed with the matching private key, the
  next release reaches v1.0.0 and v1.1.0 installs through their update check.
  Those two were built with `--no-sign`, so no `latest.json` exists yet. The
  first signed release is the first one the updater can serve.
- **If the private key is lost,** generate a new keypair and commit its public
  key. v1.x installs then need one manual install.
- **The endpoint works under the new name.** It is
  `https://github.com/bustrama/ottid/releases/latest/download/latest.json`.
  Older builds still ask for `bustrama/lashon/...`; on 2026-10-04 GitHub
  answered that with a 301 to the new name, which the updater follows.
  tauri-action writes `latest.json` with asset URLs under the current
  repository name. As ADR-0042 says, never create a new `bustrama/lashon`
  repository: it would cancel the redirect.

### macOS (a note only)

macOS is paused (ADR-0033). When it resumes:

- **Signing and notarization.** It needs the Apple Developer Program ($99/year;
  individuals can enroll from Israel), a Developer ID Application certificate,
  and notarization. tauri-action takes these from `APPLE_CERTIFICATE`,
  `APPLE_CERTIFICATE_PASSWORD` and `APPLE_SIGNING_IDENTITY`, plus either
  `APPLE_API_ISSUER` / `APPLE_API_KEY` / `APPLE_API_KEY_PATH` or `APPLE_ID` /
  `APPLE_PASSWORD` / `APPLE_TEAM_ID`.
- **The frozen sidecar.** Its Mach-O files each need signing with the hardened
  runtime before bundling. That is the same tree-signing problem as on Windows,
  done with `codesign`.

## What the owner does

These steps need the owner's identity, money or accounts. They can't be
automated:

1. **Register the business** (עוסק). Its name and address are what the
   certificate shows and what every document must match.
2. **Prepare the domain.** You need a domain registered to the business, with a
   website and a monitored mailbox on it. You also need a second address on the
   same domain.
3. **Set up Azure:**
   - Create a pay-as-you-go subscription (free and trial subscriptions are
     refused).
   - Register the `Microsoft.CodeSigning` provider.
   - Create an Artifact Signing account on the Basic tier, in a region such as
     West Europe. Its endpoint, e.g. `https://weu.codesigning.azure.net`, must
     match the account's region.
   - Give yourself the *Artifact Signing Identity Verifier* role.
4. **Request the identity validation:** an **Organization → Public** validation.
   Complete the ID check in Microsoft Authenticator, then wait for
   **Completed** (1–20 business days).
5. **Create a Public Trust certificate profile.**
6. **Create an app registration** in Entra ID, without a secret. Add a
   federated credential with these values:
   - issuer `https://token.actions.githubusercontent.com`
   - subject `repo:bustrama/ottid:environment:release`
   - audience `api://AzureADTokenExchange`

   Then give the app the *Artifact Signing Certificate Profile Signer* role on
   the certificate profile.
7. **Create the GitHub environment** `release`:
   - Deployment rules: tags `v*`, and `main`.
   - Secrets: `AZURE_CLIENT_ID`, `AZURE_TENANT_ID`, `AZURE_SUBSCRIPTION_ID`.
   - Variables: `ARTIFACT_SIGNING_ENDPOINT`, `ARTIFACT_SIGNING_ACCOUNT`,
     `ARTIFACT_SIGNING_PROFILE`.
8. **Store the updater key.** Put the private key that matches key ID
   `DC3C2A529F34339A` into the same environment, as the secrets
   `TAURI_SIGNING_PRIVATE_KEY` and `TAURI_SIGNING_PRIVATE_KEY_PASSWORD`.
9. **Ship the first signed release.** Push the tag, then smoke-test the draft's
   installer, both fresh and over a v1.1.0 install. Check the publisher in the
   UAC prompt. Then publish.

## Consequences

- **Cost.** A signed release costs about $120 a year, plus a domain if the
  business doesn't have one.
- **Timeline.** The first signed release waits on the business registration
  and Microsoft's validation, realistically two to five weeks.
- **Signed builds come only from CI.** Anyone building the GPL source gets an
  unsigned build, as before. The signing config is never merged locally.
- **A release run takes longer.** Signing about 130 images adds a few minutes.
- **CI can't publish unsigned by accident.** An unsigned build happens only on
  an explicitly marked pre-release, and the updater never serves it.
- **Changing the signing provider later** means a new publisher identity, and
  SmartScreen reputation starts over.
- **The full edition reuses all of this.** Its paid build stages
  `llama-server`, and the tree step signs it with no further change.
