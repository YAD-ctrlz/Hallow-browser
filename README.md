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
  benchmarks) plus link-time optimization across the Rust/C++ boundary. On top
  of that, network, rendering and media limits are raised (see
  `prefs/hallow.js`).
- **Corporate identity.** The interface (menus, tabs, toolbars, Settings) is
  set in IBM Plex Sans and the home page in Space Grotesk. Both fonts ship
  inside Hallow, so nothing is installed system-wide.
- **Lightweight.** Built without the crash reporter, updater, tests or debug
  symbols. AI features are blocked by default, so their models never
  download. Background tabs unload when memory runs low.
- **Private search.** Startpage is the default search engine in every region
  (Google results without tracking or profiling), with plain startpage.com
  URLs and no partner codes. Other engines are one click away in Settings.
- **No telemetry.** Hallow is an unofficial build, so Firefox's telemetry
  upload is not compiled in, and studies and experiments are off.
- **Rust-first.** See [Rust in Hallow](#rust-in-hallow).

## Install

Download `hallow_<version>_amd64.deb` from the latest release, then:

```sh
sudo apt install ./hallow_*_amd64.deb
```

The package needs Debian 12+ or Ubuntu 22.04+ on amd64. It installs to
`/usr/lib/hallow`, adds `hallow` to your `PATH`, adds an app-menu entry and
registers Hallow as an `x-www-browser` alternative. Profiles are kept apart
from Firefox's, in `~/.config/mozilla/hallow`.

Hallow does not update itself. Install a newer `.deb` from Releases to update.

## Rust in Hallow

Gecko is written in C++, Rust and JavaScript, and a fork cannot rewrite it.
Hallow leans on Rust where Gecko allows it:

| Where | What |
| --- | --- |
| WebGPU | `dom.webgpu.enabled` turns on WebGPU, which runs on [wgpu](https://github.com/gfx-rs/wgpu), Mozilla's Rust graphics stack. Firefox Release still has it off on Linux. |
| JPEG XL | `image.jxl.enabled` turns on JPEG XL images, decoded by the Rust `jxl-rs` decoder. Firefox enables this only in Nightly. |
| Build | `--enable-rust-simd` adds explicit SIMD to Rust crates. `--enable-lto=cross` applies ThinLTO across the Rust/C++ boundary, so Stylo, WebRender and the other Rust components are optimized together with the C++ that calls them. |
| Tooling | All of Hallow's own code is Rust ([`tools/hallow-build`](tools/hallow-build)): fetching and verifying Firefox, patching, branding, rendering the icons from SVG (resvg), linting prefs, and writing the `.deb` itself (ar, tar and xz in pure Rust, no `dpkg-deb`). |

## How it works

This repository holds only what Hallow changes. The Firefox source is
downloaded at build time:

```
hallow.toml        Firefox version + SHA-512 to build, Hallow revision
mozconfig          build options (identity, Rust, PGO/LTO, lightweight)
patches/           small source patches, applied strictly (no fuzz):
                   chrome stylesheet, Firefox UA token, Startpage default,
                   IBM Plex Sans interface font, Space Grotesk home page
branding/          logo.png (every icon size is rendered from it),
                   wordmark.svg, fonts/, brand strings in overlay/
prefs/hallow.js    default prefs (UI, speed, privacy, Rust features)
ui/                chrome and home page stylesheets
packaging/         .desktop file, AppStream metadata, maintainer scripts
tools/hallow-build the build tool
```

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

### Releases (GitHub Actions)

| Workflow | When | What |
| --- | --- | --- |
| `release.yml` | push to `main`/`development` touching the build, or manual | builds Hallow in three PGO stages (instrumented build, training run, optimized build), test-installs the `.deb`, takes screenshots and publishes the `v<version>` release (about 4 hours) |
| `upstream.yml` | daily | runs `cargo hb bump` and starts a release when Firefox ships |
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
