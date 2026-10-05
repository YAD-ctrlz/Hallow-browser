#!/usr/bin/env bash
# One-time setup of Hallow's release signing keys (docs/RELEASING.md,
# "One-time setup"), run once by the repository owner on their own computer:
#
#   curl -fsSL https://raw.githubusercontent.com/YAD-ctrlz/Hallow-browser/development/tools/setup-release-keys.sh | bash
#
# Needs gh (logged in as the repository owner: `gh auth login`), gpg and
# openssl; on Ubuntu or Linux Mint: sudo apt install gh gnupg openssl.
#
#   1. Creates the Linux archive signing key (GPG, ed25519), which signs the
#      APT repository, and the Windows update signing key (RSA 4096 with a
#      self-signed certificate), which signs Windows update packages.
#   2. Creates the `production` environment, usable only by workflows on the
#      `release` branch, and stores both private keys in it as secrets
#      (HALLOW_SIGNING_KEY, HALLOW_MAR_SIGNING_KEY).
#   3. Commits both public keys to the development branch, where the builds
#      pick them up (installed Hallows trust only these keys).
#   4. Leaves a backup of the private keys in ~/hallow-release-keys-<date>.
#
# The private keys go nowhere but GitHub's encrypted secret store and that
# backup. It refuses to run once the keys exist: new keys would cut installed
# Hallows off from updates.
set -euo pipefail

REPO=${HALLOW_REPO:-YAD-ctrlz/Hallow-browser}
BRANCH=development
ENVIRONMENT=production
ARCHIVE_KEY=packaging/linux/hallow-archive-keyring.asc
UPDATE_CERT=packaging/windows/hallow-update-signing.der

die() { echo "error: $*" >&2; exit 1; }
step() { echo "==> $*"; }

for tool in gh gpg gpgconf openssl base64; do
  command -v "$tool" >/dev/null || die "needs $tool (sudo apt install gh gnupg openssl)"
done
gh auth status --hostname github.com >/dev/null 2>&1 || die "log in to GitHub first: gh auth login"
[ "$(gh api "repos/$REPO" -q .permissions.admin)" = true ] ||
  die "your GitHub account must be an admin of $REPO"
for path in "$ARCHIVE_KEY" "$UPDATE_CERT"; do
  if gh api "repos/$REPO/contents/$path?ref=$BRANCH" >/dev/null 2>&1; then
    die "$path is already on $BRANCH: the keys are set up. Replacing them would cut installed Hallows off from updates (see docs/RELEASING.md)."
  fi
done
# Commits carry the account's private no-reply address, never a real one.
login=$(gh api user -q .login)
email="$(gh api user -q .id)+$login@users.noreply.github.com"

umask 077
backup="$HOME/hallow-release-keys-$(date +%Y%m%d-%H%M%S)"
mkdir "$backup"

step "Creating the Linux archive signing key"
# A throwaway keyring with a short path (gpg-agent's socket lives in it).
export GNUPGHOME
GNUPGHOME=$(mktemp -d)
trap 'gpgconf --kill all 2>/dev/null; rm -rf "$GNUPGHOME"' EXIT
gpg --batch --quiet --pinentry-mode loopback --passphrase '' \
  --quick-gen-key "Hallow archive signing key" ed25519 sign never
gpg --batch --armor --export "Hallow archive signing key" > "$backup/hallow-archive-keyring.asc"
gpg --batch --armor --pinentry-mode loopback --passphrase '' \
  --export-secret-keys "Hallow archive signing key" > "$backup/hallow-signing-key.asc"
fingerprint=$(gpg --batch --with-colons --list-secret-keys | awk -F: '/^fpr:/ {print $10; exit}')
gpgconf --kill all
rm -rf "$GNUPGHOME"
trap - EXIT
unset GNUPGHOME

step "Creating the Windows update signing key"
openssl req -x509 -newkey rsa:4096 -sha384 -days 7300 -nodes \
  -subj "/CN=Hallow update signing key" \
  -keyout "$backup/hallow-update-signing.key" \
  -outform DER -out "$backup/hallow-update-signing.der" 2>/dev/null
# The certificate must verify what the key signs, as the updater will check.
probe="$backup/.probe"
echo "Hallow" > "$probe"
openssl dgst -sha384 -sign "$backup/hallow-update-signing.key" -out "$probe.sig" "$probe"
openssl x509 -inform DER -in "$backup/hallow-update-signing.der" -pubkey -noout > "$probe.pub"
openssl dgst -sha384 -verify "$probe.pub" -signature "$probe.sig" "$probe" >/dev/null ||
  die "the new Windows key does not match its certificate"
rm -f "$probe" "$probe.sig" "$probe.pub"
cert_sha256=$(openssl x509 -inform DER -in "$backup/hallow-update-signing.der" -noout \
  -fingerprint -sha256 | cut -d= -f2)

step "Creating the $ENVIRONMENT environment (only the release branch may use it)"
policy=$(gh api "repos/$REPO/environments/$ENVIRONMENT" \
  -q .deployment_branch_policy.custom_branch_policies 2>/dev/null || true)
if [ "$policy" != true ]; then
  gh api -X PUT "repos/$REPO/environments/$ENVIRONMENT" --input - >/dev/null <<'EOF'
{"deployment_branch_policy": {"protected_branches": false, "custom_branch_policies": true}}
EOF
fi
if ! gh api "repos/$REPO/environments/$ENVIRONMENT/deployment-branch-policies" \
    -q '.branch_policies[].name' | grep -qx release; then
  gh api -X POST "repos/$REPO/environments/$ENVIRONMENT/deployment-branch-policies" \
    -f name=release -f type=branch >/dev/null
fi

step "Storing the private keys as $ENVIRONMENT secrets"
gh secret set HALLOW_SIGNING_KEY --env "$ENVIRONMENT" --repo "$REPO" \
  < "$backup/hallow-signing-key.asc"
gh secret set HALLOW_MAR_SIGNING_KEY --env "$ENVIRONMENT" --repo "$REPO" \
  < "$backup/hallow-update-signing.key"
secrets=$(gh secret list --env "$ENVIRONMENT" --repo "$REPO")
for name in HALLOW_SIGNING_KEY HALLOW_MAR_SIGNING_KEY; do
  grep -q "^$name[[:space:]]" <<< "$secrets" || die "$name did not reach GitHub"
done

step "Committing the public keys to $BRANCH"
commit() {  # commit <path in the repository> <file> <message>
  gh api -X PUT "repos/$REPO/contents/$1" \
    -f message="$3" -f branch="$BRANCH" \
    -f content="$(base64 < "$2" | tr -d '\n')" \
    -f "author[name]=$login" -f "author[email]=$email" \
    -f "committer[name]=$login" -f "committer[email]=$email" >/dev/null
}
commit "$UPDATE_CERT" "$backup/hallow-update-signing.der" \
  "Hallow's Windows update signing certificate"
commit "$ARCHIVE_KEY" "$backup/hallow-archive-keyring.asc" \
  "Hallow's Linux archive signing key (public)"

cat <<EOF

Done. Hallow's release signing keys are set up.

  Linux archive key:         $fingerprint
  Windows update cert (SHA-256): $cert_sha256

The private keys are in GitHub, in the "$ENVIRONMENT" environment, which
only the release branch can use. A backup is in

  $backup

Copy that folder somewhere safe and offline (a USB stick or a password
manager), then delete it here: rm -r "$backup"
Should GitHub's copy ever be lost, only this backup lets you keep shipping
updates that installed Hallows accept.
EOF
