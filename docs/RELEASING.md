# Releasing Hallow

## Branches

| Branch | Role |
| --- | --- |
| `release` | Production. Exactly what stable Hallow users run. Every commit on it is built, tested and published by the Release workflow. Only finished, tested versions are merged into it. |
| `development` | Integration of finished work. Every push is built and tested by the Development build workflow; the packages are CI artifacts only. |
| feature branches, pull requests | Work in progress. Also built and tested by the Development build workflow, never published. |

Nothing but the `release` branch can reach installed Hallows:

- Only `release.yml` publishes, and its first job fails unless it runs for
  `refs/heads/release` and the commit is on that branch.
- The stable update channels must be signed with Hallow's keys: the Linux
  channel (an APT repository) with the archive key, Windows update packages
  (MAR files) with the update signing key. Both keys are secrets of the
  `production` environment, which only admits the `release` branch (see
  setup below), so a workflow on any other branch, even a modified one,
  cannot sign anything installed Hallows would accept.
- Development builds use a read-only token, no environment, and a `~devN`
  version suffix, which sorts before the release it leads up to. Windows
  development builds sign their test updates with a throwaway key made for
  the CI run; release builds do not trust it.

## The release pipeline

```
development (tested)  ──merge──>  release
                                    │  .github/workflows/release.yml
                                    ├─ plan: release branch only, version not yet released,
                                    │        archive public key and update certificate present
                                    ├─ Linux: PGO 1/3 instrumented → 2/3 training → 3/3 optimized build
                                    ├─ Windows: cross-compiled LTO build (installer, zip, update package)
                                    ├─ test (Linux): install the .deb on a clean runner, desktop
                                    │        integration, browser checks, built-in update mechanism
                                    ├─ test (Windows): install, files, registration, icons, browser
                                    │        checks, refusal of foreign/unsigned updates, uninstall
                                    ├─ publish (environment: production)
                                    │    ├─ sign the Linux channel (APT repository) with the archive key
                                    │    │  and verify it with APT and only the public key users have
                                    │    ├─ sign the Windows update package with the update signing key,
                                    │    │  verify it against the certificate in Windows Hallow's updater,
                                    │    │  write update-win64.xml
                                    │    ├─ attest the packages' build provenance
                                    │    └─ GitHub release v<version> as a draft
                                    ├─ verify-windows-update: a Windows runner installs this build
                                    │        and updates it with the signed package, from the About dialog
                                    └─ go-live (environment: production): the release → latest
                                                │
                     installed Hallows ◄────────┘  daily check / About Hallow / system update manager
```

The stable channel is the latest GitHub release, read at
`https://github.com/YAD-ctrlz/Hallow-browser/releases/latest/download/`:

- Linux: a flat APT repository made of the release's assets (`InRelease`,
  `Release`, `Release.gpg`, `Packages`, `Packages.xz`, the `.deb`), listed in
  `/etc/apt/sources.list.d/hallow.sources`, which the package installs.
- Windows: `update-win64.xml` (the URL is built into Hallow, see
  `patches/0008`), which offers the signed `hallow-<version>-win64.complete.mar`.
  Gecko's updater installs it only if it is signed with Hallow's update
  signing key, is for Hallow's MAR channel (`hallow-release`) and is not an
  older version; the update service also refuses older or equal builds and
  non-HTTPS downloads (`patches/0012`). Hallow is installed per user, so
  updates never ask for administrator rights.

The release is created as a draft, completed, its Windows update installed
once on a clean machine, and only then published and marked latest, so the
channels switch to the new version in one step.

## Shipping a new version

1. On `development`, increase `[hallow].revision` in `hallow.toml` (or let
   `upstream.yml` / `cargo hb bump` move to a new Firefox release, which
   resets it to 1).
2. Push and wait for the Development build workflow: the Linux and Windows
   builds, then *Test the package* and *Test on Windows* (installation, icons
   and desktop integration, browser checks, the update mechanism). Their
   screenshots are in the job logs and the `test-results` /
   `test-results-windows` artifacts; install the `hallow-dev-deb` or
   `hallow-dev-windows` artifact to try a build.
3. When it is ready, merge `development` into `release` (a pull request from
   `development` to `release` is the intended way, see branch protection
   below).
4. The Release workflow builds that commit (Linux with PGO, about 4 hours;
   Windows alongside), tests both and publishes `v<version>`. Installed
   Hallows offer the update within a day, or right away from *Help > About
   Hallow*.

Each version is released once, from one commit. To change a release, ship a
new revision.

## One-time setup

### 1. The archive signing key

Create the key on a trusted machine (any Linux with `gpg`):

```sh
export GNUPGHOME=$(mktemp -d)
gpg --batch --pinentry-mode loopback --passphrase '' \
    --quick-gen-key "Hallow archive signing key" ed25519 sign never
gpg --armor --export "Hallow archive signing key" > hallow-archive-keyring.asc
gpg --armor --export-secret-keys "Hallow archive signing key" > hallow-signing-key.asc
gpg --fingerprint "Hallow archive signing key"
```

- Commit `hallow-archive-keyring.asc` (the public key) as
  `packaging/linux/hallow-archive-keyring.asc`. Every package ships it as
  `/usr/share/keyrings/hallow-archive-keyring.asc`, and APT only accepts the
  stable channel when it is signed with the matching private key.
- Store the private key (`hallow-signing-key.asc`) as described next, keep an
  offline backup, and delete the working copy (`rm -r "$GNUPGHOME"`).

Changing the key later means shipping the new public key in a release signed
with the old key first.

### 2. The Windows update signing key

Create it on a trusted machine with OpenSSL (an RSA key, which is what
Gecko's updater verifies MAR signatures with):

```sh
umask 077
openssl req -x509 -newkey rsa:4096 -sha384 -days 7300 -nodes \
    -subj "/CN=Hallow update signing key" \
    -keyout hallow-update-signing.key \
    -outform DER -out hallow-update-signing.der
```

- Commit `hallow-update-signing.der` (the certificate, i.e. the public key)
  as `packaging/windows/hallow-update-signing.der`. `cargo hb prepare` builds
  it into the Windows updater in place of Mozilla's certificates, and the
  updater installs only update packages signed with the matching key.
- Store the private key (`hallow-update-signing.key`) as described next, keep
  an offline backup, and delete the working copy.

Changing the key later means shipping the new certificate in a release
whose update package is signed with the old key first.

### 3. The `production` environment

*Settings > Environments > New environment*, name it `production`:

- **Deployment branches and tags:** *Selected branches and tags*, add
  `release`. This is what keeps the signing keys away from every other
  branch.
- **Environment secrets:** add `HALLOW_SIGNING_KEY` with the contents of
  `hallow-signing-key.asc`, and `HALLOW_MAR_SIGNING_KEY` with the contents of
  `hallow-update-signing.key`.
- Optionally **Required reviewers**, to approve each release before it is
  signed and published.

Do not store the key as a repository secret: those are available to
workflows on every branch.

### 4. Protect the release branch

*Settings > Rules > Rulesets* (or *Branches > Branch protection rules*), for
`release`:

- Require a pull request before merging (promotions come from
  `development`), and require the *Development build* and *CI* checks to
  pass.
- Block force pushes and deletion.

## Checking a release

- The release notes name the release-branch commit and the workflow run
  that built it; the tag points at that commit.
- `gh attestation verify <file> --repo YAD-ctrlz/Hallow-browser` (the
  `.deb`, the Windows installer, zip or update package) proves the file was
  built by this repository's Release workflow.
- `cargo hb mar verify hallow-<version>-win64.complete.mar --cert
  packaging/windows/hallow-update-signing.der` checks a Windows update
  package's signature the way the updater does.
- `apt-get update` on any installed Hallow verifies the channel's signature;
  `apt-cache policy hallow` shows the version it offers.
