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
- The stable update channel must be signed with the Hallow archive key. The
  key is a secret of the `production` environment, which only admits the
  `release` branch (see setup below), so a workflow on any other branch,
  even a modified one, cannot sign anything installed Hallows would accept.
- Development builds use a read-only token, no environment, and a `~devN`
  version suffix, which sorts before the release it leads up to.

## The release pipeline

```
development (tested)  ──merge──>  release
                                    │  .github/workflows/release.yml
                                    ├─ plan: release branch only, version not yet released,
                                    │        archive public key present
                                    ├─ PGO 1/3 instrumented build → 2/3 training → 3/3 optimized build
                                    ├─ test: install the .deb on a clean runner, desktop integration,
                                    │        browser checks, built-in update mechanism
                                    └─ publish (environment: production)
                                         ├─ sign the stable channel (APT repository) with the archive key
                                         ├─ verify it with APT and only the public key users have
                                         ├─ attest the package's build provenance
                                         └─ GitHub release v<version> → marked latest
                                                │
                     installed Hallows ◄────────┘  daily check / About Hallow / system update manager
```

The stable channel is a flat APT repository made of the assets of the latest
GitHub release (`InRelease`, `Release`, `Release.gpg`, `Packages`,
`Packages.xz`, the `.deb`). Installed Hallows read it at
`https://github.com/YAD-ctrlz/Hallow-browser/releases/latest/download/`
(`/etc/apt/sources.list.d/hallow.sources`, installed by the package). The
release is created as a draft, completed, and only then published and marked
latest, so the channel switches to the new version in one step.

## Shipping a new version

1. On `development`, increase `[hallow].revision` in `hallow.toml` (or let
   `upstream.yml` / `cargo hb bump` move to a new Firefox release, which
   resets it to 1).
2. Push and wait for the Development build workflow: the build, then *Test
   the package* (installation, icons and desktop integration, browser checks,
   the update mechanism). Its screenshots are in the job log and the
   `test-results` artifact; install the `hallow-dev-deb` artifact to try it.
3. When it is ready, merge `development` into `release` (a pull request from
   `development` to `release` is the intended way, see branch protection
   below).
4. The Release workflow builds that commit with PGO (about 4 hours), tests it
   and publishes `v<version>`. Installed Hallows offer the update within a
   day, or right away from *Help > About Hallow*.

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

### 2. The `production` environment

*Settings > Environments > New environment*, name it `production`:

- **Deployment branches and tags:** *Selected branches and tags*, add
  `release`. This is what keeps the signing key away from every other branch.
- **Environment secrets:** add `HALLOW_SIGNING_KEY` with the contents of
  `hallow-signing-key.asc`.
- Optionally **Required reviewers**, to approve each release before it is
  signed and published.

Do not store the key as a repository secret: those are available to
workflows on every branch.

### 3. Protect the release branch

*Settings > Rules > Rulesets* (or *Branches > Branch protection rules*), for
`release`:

- Require a pull request before merging (promotions come from
  `development`), and require the *Development build* and *CI* checks to
  pass.
- Block force pushes and deletion.

## Checking a release

- The release notes name the release-branch commit and the workflow run
  that built it; the tag points at that commit.
- `gh attestation verify hallow_<version>_amd64.deb --repo YAD-ctrlz/Hallow-browser`
  proves the package was built by this repository's Release workflow.
- `apt-get update` on any installed Hallow verifies the channel's signature;
  `apt-cache policy hallow` shows the version it offers.
