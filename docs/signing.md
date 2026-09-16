# Code signing: getting rn's Windows release past Smart App Control

Why this exists, in one measurement: on 2026-09-16, on Windows 11 with Smart
App Control on, v0.1.5's downloaded `install-rn.cmd` was blocked as "a
dangerous file extension", and its `rn.exe` was blocked as well, however it
arrived — CodeIntegrity events 3033, 3077 and 3118. Smart App Control lets a
program run when Microsoft's cloud already trusts that exact file, or when it
is signed with a certificate Windows trusts. A new build of rn never starts
with the first, so it cannot count on it.

**The same day, it got the first anyway.** At 16:10 the unsigned v0.1.6 MSI,
downloaded with Chrome, installed and started rn on that PC, and by 16:15 the
very v0.1.5 `rn.exe` blocked at 13:26 ran too — same bytes, same folder, a
byte-for-byte copy with no cached attributes included, Smart App Control
reporting On throughout. So the cloud verdict on those files changed within
three hours. That is good news for those two files and none for the next
release, whose bytes start with no reputation again; it is why a trusted
signature is still the fix, rather than waiting.

The same machine ran RERAG's `ag.exe`, which is just as unsigned. That was
checked, because it looked like a counter-example: a byte-for-byte copy of it,
with none of the file's cached verdicts, ran too. So Microsoft's cloud trusts
that one build, and its reputation belongs to its exact bytes. It says nothing
about the next build of anything, rn included, so it is not a route.

## The pipeline

RERAG's, carried over: `signtool` on the release workflow's Windows runner, in
the `windows` job of `.github/workflows/release.yml`:

1. **Configure code signing** decodes the `WINDOWS_PFX_BASE64` secret to a
   `.pfx` in the runner's temp directory and puts the Windows SDK's `signtool`
   on PATH. No secret: a notice, and every step below skips. The build stays
   unsigned and nothing fails.
2. **Sign rn.exe**, *before* `package-msi.ps1` builds the MSI, so the copies
   inside the MSI and the zip carry the signature too.
3. **Build the MSI**, then **Sign the MSI**.
4. The decoded `.pfx` is deleted, and the Windows checksums are written after
   signing, since a signature changes the bytes.
5. **windows-test** reports the signature status of the MSI and the installed
   `rn.exe`, and fails if signing was on but either is unsigned.
6. The release notes say whether the Windows files are signed.

Each signature is SHA-256 with an RFC 3161 timestamp from DigiCert
(`/fd SHA256 /tr http://timestamp.digicert.com /td SHA256`), so it stays valid
after the certificate expires.

Checked on 2026-09-16 with a throwaway self-signed certificate, using
`osslsigncode` on Linux while the MSI was still built there: `rn.exe` and the
MSI both signed and timestamped, and Windows' own `Get-AuthenticodeSignature`
read the result as signed and timestamped, with
status `UnknownError` — "a certificate chain processed, but terminated in a
root certificate which is not trusted". That last part is the point of the
next section.

## What the certificate has to be

**Only a certificate that chains to a root Windows trusts gets past Smart App
Control.** Anything else produces a signature that proves the pipeline works
and changes nothing for a user.

| certificate | signs? | Smart App Control |
|---|---|---|
| none (the default) | no | depends on Microsoft's reputation for that exact build: blocked, or not |
| self-signed `.pfx` (`New-SelfSignedCertificate`, `openssl req -x509`) | yes | **no better than none** |
| a code-signing certificate from a public CA, as a `.pfx` | yes | accepts it — but see below |

The catch in the last row: since 1 June 2023, the CA/Browser Forum's baseline
requirements oblige every public CA to generate and keep a code-signing
certificate's private key on hardware — a USB token or HSM, or a cloud key
service. **No public CA issues a code-signing certificate as an exportable
`.pfx` any more**, OV or EV, so there is nothing trusted to put in the secret.
A `.pfx` issued before that date works until it expires; one from a CA that
still offers a file is one to be suspicious of.

What this pipeline is good for, then:

- **Proving the path end to end** with a self-signed certificate: every step
  runs, and the Windows job shows the signature arrives intact through the MSI.
- **A certificate that already exists as a `.pfx`**, issued before June 2023.

Getting a trusted signature for new releases means a key that never leaves a
key service: Azure Artifact Signing (Microsoft's, formerly Trusted Signing), a
CA's cloud HSM, or SignPath Foundation for open-source projects. Each replaces
steps 1–3 above with its own signing call and leaves the rest as it is.

## Setting it up

1. **Make the certificate a single base64 line:**

       base64 -w0 cert.pfx > cert.pfx.b64          # Linux
       [Convert]::ToBase64String([IO.File]::ReadAllBytes("cert.pfx")) | Set-Content cert.pfx.b64   # PowerShell

   A throwaway self-signed one, to try the pipeline:

       openssl req -x509 -newkey rsa:3072 -nodes -keyout key.pem -out cert.pem -days 30 \
           -subj "/CN=rn test signing" -addext "extendedKeyUsage=codeSigning"
       openssl pkcs12 -export -inkey key.pem -in cert.pem -out cert.pfx

2. **Add two repository secrets** (Settings → Secrets and variables → Actions):

   | secret | value |
   |---|---|
   | `WINDOWS_PFX_BASE64` | the contents of `cert.pfx.b64` |
   | `WINDOWS_PFX_PASSWORD` | the `.pfx` password |

   Then delete `cert.pfx.b64`, and keep `cert.pfx` somewhere that is not the
   repository.

3. **Run it without publishing:** `scripts/release.sh --test`. The build log
   (the `windows` job) shows `signed rn.exe -> ...` and `signed the MSI -> ...`, and
   the Windows job prints each file's signature status and signer.

4. **Release** with `scripts/release.sh`. The notes say the files are signed.

5. **With a trusted certificate only:** download the MSI on a machine with Smart
   App Control on, confirm Properties → Digital Signatures shows the
   certificate, install it, and confirm rn starts. No runner has Smart App
   Control, so this is the one check the workflow cannot make. Then update the
   README's code signing section to say releases are signed and by whom.

Pull requests from forks get no secrets, so they always build unsigned; that is
GitHub's rule, and the right one.

## What a signature does not change

- **SmartScreen** may still show "Windows protected your PC" for a while: that
  is reputation, which a certificate starts but does not grant. Smart App
  Control's rule is the trusted certificate, which is the one that blocked rn
  outright.
- **The script route** (`install-rn.cmd`, `install-gui.ps1`, `install.ps1`)
  stays unsigned. A `.cmd` cannot be signed at all. With Smart App Control on,
  the MSI is the only route.
- **The Linux package** has nothing to do with any of this.
