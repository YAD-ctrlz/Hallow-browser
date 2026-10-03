# Windows packaging

Hallow for Windows is the NSIS installer Firefox's build makes
(`cargo hb windows-dist`), branded by `branding/windows` and made per-user
by `patches/0013`. This directory holds what the Windows build needs from
the repository:

- `hallow-update-signing.der`: the certificate (public key) of Hallow's
  update signing key. `cargo hb prepare` builds it into Gecko's updater in
  place of Mozilla's certificates, so installed Hallows only install update
  packages (MAR files) signed with Hallow's key. Release builds require it;
  docs/RELEASING.md describes how to create the key and where the private
  key is kept (the `production` environment's `HALLOW_MAR_SIGNING_KEY`).
