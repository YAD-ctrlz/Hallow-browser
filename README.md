<p align="center">
  <img src="branding/logo.png" width="128" alt="Hallow logo: a pixel-art globe with a halo">
</p>

# Hallow

Hallow is a clean, lightweight web browser built from the newest Firefox
release, for Windows and Linux. It ships as a Windows installer and a `.deb`
on the [Releases page](https://github.com/YAD-ctrlz/Hallow-browser/releases).

- **Newest Gecko.** Every build starts from the latest Firefox source release
  (currently **157.0**). A daily workflow picks up new Firefox releases and
  builds them automatically.
- **Clean UI.** Compact toolbars, a full-width address bar and no Firefox View,
  account or bookmarks-bar clutter. The new tab page has no sponsored tiles,
  stories or weather. No onboarding tours, promos or "what's new" pages.
- **Fast.** Compiled the way Mozilla compiles Firefox releases:
  profile-guided optimization (trained on Mozilla's PGO corpus: Speedometer,
  layout and style benchmarks, on each platform), link-time optimization
  across the Rust/C++ boundary, Rust at `opt-level=2`, release
  configuration. Everything else (networking, HTTP/2
  and HTTP/3, caches, WebRender, the JavaScript and WebAssembly JITs) runs
  with Firefox's tuned defaults rather than unmeasured pref tweaks.
- **Efficient video.** Hardware video decoding (VA-API on Linux, D3D11 on
  Windows) as Firefox's own driver checks allow, with automatic fallback to
  software. Streaming sites are steered to the codec your GPU decodes: AV1
  only where it decodes in hardware, otherwise VP9, otherwise H.264.
- **Updates itself.** Hallow checks its stable channel daily and in *About
  Hallow*. On Windows it downloads and installs new versions in the
  background, without administrator prompts; on Linux it installs them
  through the system package manager after you confirm with your password.
  Only the `release` branch can publish to that channel, and every update is
  signature-checked against Hallow's own keys.
- **Corporate identity.** The interface (menus, tabs, toolbars, Settings) is
  set in IBM Plex Sans and the home page in Space Grotesk. Both fonts ship
  inside Hallow, so nothing is installed system-wide.
- **Lightweight.** Built without the crash reporter, tests or debug symbols.
  AI features are blocked by default, so their models never download.
  Nothing Hallow adds runs at startup. Long-idle background tabs unload only
  when the system runs low on memory.
- **Private search.** Startpage is the default search engine in every region
  (Google results without tracking or profiling), with plain startpage.com
  URLs and no partner codes. Other engines are one click away in Settings.
- **No telemetry.** Hallow is an unofficial build, so Firefox's telemetry
  upload is not compiled in, and studies and experiments are off.
- **Rust-first.** See [Rust in Hallow](#rust-in-hallow).
- **Proper Windows app.** A per-user installer that needs no administrator
  rights; an icon with every size Windows uses for the taskbar, title bars,
  Start, Explorer and the desktop at each display scale (16-256 px, small
  sizes sharpened); its own private browsing and document icons, Start tile
  and installer artwork; an entry in *Settings > Apps* with a clean
  uninstaller.
- **Proper Linux app.** Icons at every standard size (16-512 px plus SVG)
  sized like other apps' in menus, panels, docks (Plank) and Alt-Tab; the
  window, desktop file and icon belong together (`WM_CLASS` / app ID
  `hallow`).

## Install

### Windows

Download `hallow-<version>-win64-setup.exe` from the latest release and run
it. Windows 10 and 11, 64-bit. Hallow installs for your user account into
`%LOCALAPPDATA%\Hallow`, without asking for administrator rights, and adds
Start menu and desktop shortcuts and an entry in *Settings > Apps* to
uninstall it. Profiles are kept apart from Firefox's, in
`%APPDATA%\Mozilla\Hallow`.

The installer is not code-signed (that needs a paid certificate), so
Microsoft Defender SmartScreen may say "Windows protected your PC" the first
time: choose *More info > Run anyway*. Hallow's updates do not go through
SmartScreen; they are verified with Hallow's own signing key instead.

Hallow keeps itself up to date: it checks its stable channel daily and
whenever you open *Help > About Hallow*, downloads a new version in the
background and switches to it the next time Hallow starts (or right away
with *Restart to update*).

### Linux

Download `hallow_<version>_amd64.deb` from the latest release, then:

```sh
sudo apt install ./hallow_*_amd64.deb
```

The package needs Debian 12+, Ubuntu 22.04+ or Linux Mint 21+ on amd64. It
installs to `/usr/lib/hallow`, adds `hallow` to your `PATH`, adds an app-menu
entry with its icons and registers Hallow as an `x-www-browser` alternative.
Profiles are kept apart from Firefox's, in `~/.config/mozilla/hallow`.

That is the last time you download Hallow by hand. The package adds Hallow's
stable update channel (`/etc/apt/sources.list.d/hallow.sources`, a signed APT
repository) and Hallow checks it once a day and whenever you open *Help >
About Hallow*: when a new version is out, *Update to …* installs it (your
system asks for your password) and *Restart to update* switches to it. Your
system's update manager offers Hallow updates as well. Delete
`hallow.sources` to opt out. (Versions before 157.0-7 had no updater; install
157.0-7 or newer once by hand.)

## Rust in Hallow

Gecko is written in C++, Rust and JavaScript, and a fork cannot rewrite it.
Hallow leans on Rust where Gecko allows it:

| Where | What |
| --- | --- |
| Engine | Stylo (CSS), WebRender (graphics), the AV1 decoder dav1d's Rust glue, encoding_rs and many more Gecko components are Rust; Hallow builds all of it with Mozilla's own Rust release. WebGPU (wgpu) and JPEG XL (jxl-rs) follow Firefox's release schedule: Hallow no longer turns them on early, because Firefox does not ship them on Linux yet. |
| Build | `--enable-rust-simd` adds explicit SIMD to Rust crates. `--enable-lto=cross` applies ThinLTO across the Rust/C++ boundary, so Stylo, WebRender and the other Rust components are optimized together with the C++ that calls them. |
| Tooling | Hallow's build tool is Rust ([`tools/hallow-build`](tools/hallow-build)): fetching and verifying Firefox, patching, branding, rendering every icon size from the logo, linting prefs, and writing the `.deb` itself (ar, tar and xz in pure Rust, no `dpkg-deb`). |

## How it works

This repository holds only what Hallow changes. The Firefox source is
downloaded at build time:

```
hallow.toml        Firefox version + SHA-512 to build, Hallow revision
mozconfig          build options (identity, release/PGO/LTO, updates)
patches/           small source patches, applied strictly (no fuzz):
                   0001 chrome stylesheet       0008 Hallow update channel
                   0002 Firefox UA token        0009 Settings feedback to Hallow
                   0003 Startpage default       0010 bundled fonts
                   0004 IBM Plex Sans UI font   0011 IBM Plex Sans UI font (Windows)
                   0005 Space Grotesk home page 0012 no older or non-HTTPS updates
                   0006 hardware codec choice   0013 per-user Windows installer
                   0007 package updates in UI   0014 Windows Start tile
                                                0015 Hallow in file properties
gecko/             files Hallow adds to the Firefox tree (copied by
                   `cargo hb prepare`): LinuxPackageUpdater, the Linux
                   backend of the update UI
branding/          logo.png (every icon size is rendered from it),
                   wordmark.svg, fonts/, brand strings in overlay/,
                   Windows installer defines and Start tiles in windows/
prefs/hallow.js    default prefs (UI, privacy, updates, codecs)
ui/                chrome and home page stylesheets
packaging/linux/   .deb contents: .desktop file, AppStream metadata,
                   maintainer scripts, update channel (APT source, archive
                   key), update helper and its polkit policy
packaging/windows/ the certificate of Hallow's update signing key
tools/             the build tool (hallow-build), the update channel
                   builders and the Linux/Windows package, browser and
                   update tests
docs/RELEASING.md  branches, release pipeline, signing key setup
```

Most of Hallow is shared and platform-independent: UI, prefs, branding,
fonts, patches, the update UI, the release-branch-only update channel and
its signing model. What differs per platform is small and kept apart:

| | Linux | Windows |
| --- | --- | --- |
| Package | `.deb` (`packaging/linux`, `deb.rs`) | per-user NSIS installer (`patches/0013`, `branding/windows`, `winpkg.rs`) |
| Updates installed by | APT from a signed repository (`LinuxPackageUpdater`, pkexec helper) | Gecko's updater from MAR packages signed with Hallow's key (`mar.rs`) |
| Icons | hicolor theme, window icons (`icons.rs`) | `.ico` with every Windows size, tiles, installer art (`windows.rs`) |
| Interface font hook | `patches/0004` (GTK) | `patches/0011` |
| PGO training run | Linux runner (Xvfb) | Windows runner (`tools/pgo_windows.py`) |

Platform features (VA-API and D3D11 video, window identity, icon lookup)
come from Firefox's own platform layers, which Hallow configures rather
than replaces.

The build tool runs these steps (`cargo hb` is a Cargo alias for it):

```sh
cargo hb fetch          # download + verify Firefox source (SHA-512)
cargo hb prepare        # patches, branding, prefs, mozconfig
cargo hb build --bootstrap   # mach bootstrap, mach build, mach package
cargo hb deb            # dist/hallow_<version>_amd64.deb
```

For Windows, set `HALLOW_TARGET=windows` for all of them and finish with
`cargo hb windows-dist` (installer, zip and the unsigned update package)
instead of `cargo hb deb`.

Other commands:

- `cargo hb lint-prefs` checks that every pref in `prefs/hallow.js` still
  exists in the Firefox source, since Firefox renames prefs over time.
- `cargo hb bump` points `hallow.toml` at the newest Firefox release.
- `cargo hb preview <firefox-dir>` applies Hallow's prefs and stylesheet to an
  official Firefox build, so you can check UI changes in minutes.
- `cargo hb icons <dir>` renders the Linux and Windows icon sets.
- `cargo hb mar sign|verify|info` signs, verifies and inspects Windows update
  packages; `cargo hb update-xml` writes the update manifest offering one.
- `cargo hb profile <instrumented.tar.xz> --out <dir>` runs the PGO training
  corpus with an instrumented build (`HALLOW_PGO=generate`); building with
  `HALLOW_PGO=use HALLOW_PGO_DIR=<dir>` then applies the profile. For
  Windows, `tools/pgo_windows.py` runs the training on Windows (with the kit
  from `tools/pgo-kit.sh`) and `cargo hb pgo-merge` merges its raw profiles.

### Building locally

You need Linux x86_64, about 40 GB of free disk, 16 GB of RAM (or swap) and
Rust 1.95 or newer (`rustup target add wasm32-wasip1`). The compilers,
sysroot and other toolchains are downloaded from Mozilla by the build
(`--enable-bootstrap`), so the binary runs on older distributions too. A full
build takes a few hours on 4 cores.

Windows builds are cross-compiled on Linux, like Firefox's own: also
`rustup target add x86_64-pc-windows-msvc` and `apt install msitools
libc6-i386 lib32gcc-s1 lib32stdc++6 lib32z1`. The build fetches Microsoft's
Windows SDK and MSVC libraries itself. Its updater must trust a signing
certificate: `packaging/windows/hallow-update-signing.der`, or your own in
`HALLOW_UPDATE_CERTS`.

### Branches, builds and releases (GitHub Actions)

`release` is the production branch: what it holds is what stable Hallow
users run. Work happens on `development` and feature branches. See
[docs/RELEASING.md](docs/RELEASING.md) for the full model and the one-time
signing key setup.

| Workflow | When | What |
| --- | --- | --- |
| `release.yml` | push to `release` only | builds Hallow for Linux and Windows (three PGO stages each), tests both (install, icons and desktop integration, browser checks, the update mechanism), signs the stable update channels in the `production` environment, installs the signed Windows update once on a clean machine and then publishes `v<version>` as the latest release (about 4 hours) |
| `build.yml` | push to any other branch, pull requests | Linux and Windows LTO builds (no PGO) with a `~devN` version and the same tests; the packages are CI artifacts, never published |
| `windows.yml` | called by `build.yml` and `release.yml`; by hand to try Windows PGO | the Windows build (optionally with PGO) and its tests on a Windows runner |
| `package-tests.yml`, `windows-tests.yml` | changes to the tests only | run the Linux or Windows tests against the newest development build, without rebuilding |
| `upstream.yml` | daily | runs `cargo hb bump` on `development` when Firefox ships a release |
| `ci.yml` | every push / PR | rustfmt, clippy, unit tests; applies the patches and checks the prefs against the real Firefox source |
| `ui-preview.yml` | UI or pref changes | screenshots of the official Firefox build with Hallow's prefs and stylesheet |

Each version is built and released once. To ship changes without a new
Firefox release, increase `[hallow].revision` in `hallow.toml`.

## Limitations

- English (en-US) only. Firefox language packs for the same version from
  addons.mozilla.org can be installed.
- Like other unofficial Firefox builds, Hallow has no Google API keys. Google
  Safe Browsing lists and Google-based geolocation do not work. Location
  falls back to GeoClue.
- 64-bit x86 only for now (no Windows on Arm or 32-bit Windows).
- The Windows installer and executables are not Authenticode-signed (see
  [Install](#windows)). For the same reason Mozilla's maintenance service is
  not used: Hallow installs per user, which needs no privileged service.

## License

[MPL-2.0](LICENSE), like Firefox. Hallow is built from Mozilla Firefox source
code. Firefox is a trademark of the Mozilla Foundation; Hallow is not affiliated
with or endorsed by Mozilla.
