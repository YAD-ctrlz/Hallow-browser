<p align="center">
  <img src="branding/logo.png" width="128" alt="Hallow logo: a pixel-art globe with a halo">
</p>

# Hallow

Hallow is a clean, lightweight web browser built from the newest Firefox
release. It ships as a `.deb` on the
[Releases page](https://github.com/YAD-ctrlz/Hallow-browser/releases).

- **Newest Gecko.** Every build starts from the latest Firefox source release
  (currently **157.0**). A daily workflow picks up new Firefox releases and
  builds them automatically.
- **Clean UI.** Compact toolbars, a full-width address bar and no Firefox View,
  account or bookmarks-bar clutter. The new tab page has no sponsored tiles,
  stories or weather. No onboarding tours, promos or "what's new" pages.
- **Fast.** Compiled the way Mozilla compiles Firefox releases: profile-guided
  optimization (trained on Mozilla's PGO corpus: Speedometer, layout and style
  benchmarks), link-time optimization across the Rust/C++ boundary, Rust at
  `opt-level=2`, release configuration. Everything else (networking, HTTP/2
  and HTTP/3, caches, WebRender, the JavaScript and WebAssembly JITs) runs
  with Firefox's tuned defaults rather than unmeasured pref tweaks.
- **Efficient video.** Hardware video decoding (VA-API on Linux) as Firefox's
  own driver checks allow, with automatic fallback to software. Streaming
  sites are steered to the codec your GPU decodes: AV1 only where it decodes
  in hardware, otherwise VP9, otherwise H.264.
- **Updates itself.** Hallow checks its stable channel daily and in *About
  Hallow*, and installs new versions through the system package manager
  after you confirm with your password. Only the `release` branch can
  publish to that channel, and every update is signature-checked.
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
- **Proper Linux app.** Icons at every standard size (16-512 px plus SVG)
  sized like other apps' in menus, panels, docks (Plank) and Alt-Tab; the
  window, desktop file and icon belong together (`WM_CLASS` / app ID
  `hallow`).

## Install

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
                   0001 chrome stylesheet       0005 Space Grotesk home page
                   0002 Firefox UA token        0006 hardware codec preference
                   0003 Startpage default       0007 package updates in the UI
                   0004 IBM Plex Sans UI font   0008 no Mozilla update server
gecko/             files Hallow adds to the Firefox tree (copied by
                   `cargo hb prepare`): LinuxPackageUpdater, the Linux
                   backend of the update UI
branding/          logo.png (every icon size is rendered from it),
                   wordmark.svg, fonts/, brand strings in overlay/
prefs/hallow.js    default prefs (UI, privacy, updates, codecs)
ui/                chrome and home page stylesheets
packaging/linux/   .deb contents: .desktop file, AppStream metadata,
                   maintainer scripts, update channel (APT source, archive
                   key), update helper and its polkit policy
tools/             the build tool (hallow-build), the update channel
                   builder and the package/browser/update tests
docs/RELEASING.md  branches, release pipeline, signing key setup
```

Everything except `packaging/linux` and `gecko/.../LinuxPackageUpdater` is
shared, platform-independent Hallow: UI, prefs, branding, patches, the update
UI and channel model. Linux-specific code is limited to packaging, desktop
integration and how an update is installed. Platform features (VA-API,
window identity, icon lookup) come from Firefox's own platform layers, which
Hallow configures rather than replaces, so a Windows build can reuse the
rest with its own packaging and installer.

The build tool runs these steps (`cargo hb` is a Cargo alias for it):

```sh
cargo hb fetch          # download + verify Firefox source (SHA-512)
cargo hb prepare        # patches, branding, prefs, mozconfig
cargo hb build --bootstrap   # mach bootstrap, mach build, mach package
cargo hb deb            # dist/hallow_<version>_amd64.deb
```

Other commands:

- `cargo hb lint-prefs` checks that every pref in `prefs/hallow.js` still
  exists in the Firefox source, since Firefox renames prefs over time.
- `cargo hb bump` points `hallow.toml` at the newest Firefox release.
- `cargo hb preview <firefox-dir>` applies Hallow's prefs and stylesheet to an
  official Firefox build, so you can check UI changes in minutes.
- `cargo hb icons <dir>` renders the icon set.
- `cargo hb profile <instrumented.tar.xz> --out <dir>` runs the PGO training
  corpus with an instrumented build (`HALLOW_PGO=generate`); building with
  `HALLOW_PGO=use HALLOW_PGO_DIR=<dir>` then applies the profile.

### Building locally

You need Linux x86_64, about 40 GB of free disk, 16 GB of RAM (or swap) and
Rust 1.95 or newer (`rustup target add wasm32-wasip1`). The compilers,
sysroot and other toolchains are downloaded from Mozilla by the build
(`--enable-bootstrap`), so the binary runs on older distributions too. A full
build takes a few hours on 4 cores.

### Branches, builds and releases (GitHub Actions)

`release` is the production branch: what it holds is what stable Hallow
users run. Work happens on `development` and feature branches. See
[docs/RELEASING.md](docs/RELEASING.md) for the full model and the one-time
signing key setup.

| Workflow | When | What |
| --- | --- | --- |
| `release.yml` | push to `release` only | builds Hallow in three PGO stages, tests the package (install, icons and desktop integration, browser checks, the update mechanism), signs the stable update channel in the `production` environment and publishes `v<version>` as the latest release (about 4 hours) |
| `build.yml` | push to any other branch, pull requests | LTO build (no PGO) with a `~devN` version and the same package tests; the `.deb` is a CI artifact, never published |
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
- amd64 only for now.

## License

[MPL-2.0](LICENSE), like Firefox. Hallow is built from Mozilla Firefox source
code. Firefox is a trademark of the Mozilla Foundation; Hallow is not affiliated
with or endorsed by Mozilla.
