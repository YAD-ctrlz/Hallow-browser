#!/usr/bin/env bash
# Build the signed APT repository that is Hallow's update channel.
#
#   tools/make-apt-repo.sh <deb> <out-dir> <gpg-key-id>
#
# Writes a flat repository for `Suites: download/` (see
# packaging/linux/hallow.sources): Packages, Packages.xz, Release, InRelease
# and Release.gpg next to the .deb, ready to be uploaded as release assets,
# which GitHub serves at <repo>/releases/latest/download/<name>.
# `Filename:` entries are relative to the URI, hence the download/ prefix.
#
# Needs apt-utils (apt-ftparchive), xz and gpg with the signing key. A key
# of "-" leaves the repository unsigned, for tests: APT refuses those.
set -euo pipefail

deb=$1
out=$2
key=$3

mkdir -p "$out"
cp "$deb" "$out/"
cd "$out"
name=$(basename "$deb")

apt-ftparchive packages . | sed 's|^Filename: \./|Filename: download/|' > Packages
grep -q "^Filename: download/$name\$" Packages
xz -9 --keep --force Packages

apt-ftparchive \
    -o APT::FTPArchive::Release::Origin=Hallow \
    -o APT::FTPArchive::Release::Label=Hallow \
    -o APT::FTPArchive::Release::Suite=stable \
    -o APT::FTPArchive::Release::Codename=stable \
    -o APT::FTPArchive::Release::Architectures=amd64 \
    -o "APT::FTPArchive::Release::Description=Hallow stable releases" \
    release . > Release.new
mv Release.new Release

if [ "$key" = - ]; then
    echo "unsigned repository for $name in $out"
    exit 0
fi
gpg --batch --yes --local-user "$key" --digest-algo SHA512 --clearsign --output InRelease Release
gpg --batch --yes --local-user "$key" --digest-algo SHA512 --armor --detach-sign --output Release.gpg Release
echo "signed repository for $name in $out"
