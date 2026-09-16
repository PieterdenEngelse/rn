# Code signing: getting rn's Windows release past Smart App Control

Why this exists, in one measurement: on 2026-09-16, on Windows 11 with Smart
App Control on, v0.1.5's downloaded `install-rn.cmd` was blocked as "a
dangerous file extension", and its `rn.exe` was blocked as well, however it
arrived — CodeIntegrity events 3033, 3077 and 3118. Smart App Control lets a
program run when Microsoft's cloud already trusts that exact file, or when it
is signed with a certificate Windows trusts. A new build of rn is never the
first, so it has to be the second.

The same machine ran RERAG's `ag.exe`, which is just as unsigned. That was
checked, because it looked like a counter-example: a byte-for-byte copy of it,
with none of the file's cached verdicts, ran too. So Microsoft's cloud trusts
that one build, and its reputation belongs to its exact bytes. It says nothing
about the next build of anything, rn included, so it is not a route.

## What is in place, and what is not

| piece | state |
|---|---|
| An MSI, which can be signed where a `.cmd` never can | `scripts/package-msi.sh`, `scripts/rn.wxs` |
| Version information in `rn.exe`, which SignPath requires | `launcher/build.rs`; `package.sh` refuses to ship without it |
| An OSI license with no commercial dual-licensing | MIT OR Apache-2.0, `LICENSE-MIT` and `LICENSE-APACHE` |
| Builds on GitHub-hosted runners from a tagged commit | `.github/workflows/release.yml` |
| The MSI installed and uninstalled on real Windows before publishing | the `windows-test` job |
| The signing step, off until configured | the `sign` job |
| A "Code signing policy" section on the project page | README.md |
| **A SignPath Foundation certificate** | **not yet — steps below** |

## The order it has to happen in

SignPath Foundation's terms require the project to be "already released in the
form that should be signed". So the first MSI release goes out unsigned, and
the application comes after it.

1. **Cut an unsigned release with the MSI in it.** Bump `launcher/Cargo.toml`,
   commit, push, then `scripts/release.sh`. With no SignPath settings present
   the `sign` job skips itself, and the release notes say the Windows files are
   not signed. `scripts/release.sh --test` runs the same build and Windows
   install test first, without publishing anything.

2. **Turn on multi-factor authentication** on the GitHub account, if it is not
   already. The terms require it of everyone in a role, on GitHub and on
   SignPath.

3. **Apply** at <https://signpath.org/apply>. The page it will want to see is
   the repository: the README says what rn does, carries the code signing
   policy, the roles and the privacy statement, and links the license.
   Things worth being ready to answer, because they are the ones rn is closest
   to the line on:
   - *Privacy.* rn sends mail and can track clicks on links in mail it sends
     (`docs/link-tracking.md`). The terms bar features that compromise users'
     privacy; the honest description is that tracking is off unless the person
     running rn sets it up, and it applies to that person's own outgoing mail.
   - *System changes.* The MSI adds a Run entry so rn starts at logon, and
     removes it on uninstall. That is stated in the README and in `rn.wxs`.
   - *Third-party binaries.* `node.exe` ships inside the package unmodified and
     already signed by the OpenJS Foundation. It is not submitted for signing;
     the artifact configuration below names `rn.exe` and the MSI only.

4. **Once accepted, set up the SignPath project** as SignPath's onboarding
   describes, with:
   - a **Trusted Build System** link to GitHub.com for this repository;
   - a **signing policy** (the Foundation's is usually called
     `release-signing`) requiring manual approval, with yourself as approver;
   - the **artifact configuration** below;
   - a **CI user** with submitter rights, whose API token goes into GitHub.

5. **Add the settings to the repository** (Settings → Secrets and variables →
   Actions):

   | kind | name | value |
   |---|---|---|
   | secret | `SIGNPATH_API_TOKEN` | the CI user's API token |
   | variable | `SIGNPATH_ORGANIZATION_ID` | the organization ID from SignPath |
   | variable | `SIGNPATH_PROJECT_SLUG` | e.g. `rn` |
   | variable | `SIGNPATH_SIGNING_POLICY_SLUG` | e.g. `release-signing` |
   | variable | `SIGNPATH_ARTIFACT_CONFIGURATION_SLUG` | the configuration's slug; empty uses the project default |

   `SIGNPATH_ORGANIZATION_ID` is the switch: the `sign` job runs only when it
   is set.

6. **Cut the next release.** The workflow now stops at the `sign` job and waits,
   for up to an hour, for the request to be approved in SignPath. Approved, it
   publishes the signed files and the notes say so. Not approved in time, the
   run fails and nothing is published — deliberately: once signing is on, an
   unsigned Windows release is a regression, not a fallback.

7. **Check the result on a machine with Smart App Control on**, which no runner
   has. Download the MSI, confirm Properties → Digital Signatures names
   SignPath Foundation, install it, and confirm rn starts. Then change the
   README's code signing status to granted, with the attribution sentence the
   terms require.

## The artifact configuration

What the workflow uploads is the `windows-unsigned` artifact: a zip holding
`rn-windows-x64.msi` and `rn-windows-x64.zip`. This signs `rn.exe` inside both,
then the MSI itself, in one request and one approval:

```xml
<?xml version="1.0" encoding="utf-8"?>
<artifact-configuration xmlns="http://signpath.io/artifact-configuration/v1">
  <parameters>
    <parameter name="version" />
  </parameters>
  <zip-file>
    <msi-file path="rn-windows-x64.msi" subject="rn ${version}" author="Pieter den Engelse">
      <pe-file path="**/rn.exe" product-name="rn" product-version="${version}">
        <authenticode-sign />
      </pe-file>
      <authenticode-sign />
    </msi-file>
    <zip-file path="rn-windows-x64.zip">
      <pe-file path="rn-win/rn.exe" product-name="rn" product-version="${version}">
        <authenticode-sign />
      </pe-file>
    </zip-file>
  </zip-file>
</artifact-configuration>
```

- `version` comes from the workflow's `parameters:` input, read from
  `launcher/Cargo.toml`. `launcher/build.rs` writes the same string into
  `rn.exe`'s ProductVersion and `package-msi.sh` into the MSI's subject, so the
  restrictions hold exactly when the files are the ones this repository built.
- **Not yet checked against SignPath:** how files inside an MSI are addressed.
  SignPath's documentation does not show an example, so `**/rn.exe` matches it
  at any depth rather than guessing the install path. SignPath validates a
  configuration against a sample artifact when it is saved; download a
  `windows-unsigned` artifact from any workflow run to use as that sample, and
  adjust the path if it complains.

## What a signature does not change

- **SmartScreen** may still show "Windows protected your PC" for a while: that
  is reputation, which a certificate starts but does not grant. Smart App
  Control's rule is the certificate, which is the one that blocked rn outright.
- **The script route** (`install-rn.cmd`, `install-gui.ps1`, `install.ps1`)
  stays unsigned. A `.cmd` cannot be signed at all, and the PowerShell scripts
  would run under Smart App Control's restrictions for unsigned scripts. With
  Smart App Control on, the MSI is the only route.
- **The Linux package** has nothing to do with any of this.
