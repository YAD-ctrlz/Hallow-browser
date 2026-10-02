#!/usr/bin/env python3
"""End-to-end test of Hallow's built-in updates on Linux.

Run on a machine (CI runner) where the Hallow package under test is
installed; needs sudo, Xvfb's DISPLAY, apt-utils and gpg. It serves local
copies of the stable channel, laid out like the GitHub release assets and
signed with throwaway keys, points the installed Hallow at them and checks
that Hallow

  - rejects a channel signed with another key, or not signed at all,
  - never offers an older version,
  - refuses a package whose contents changed after the channel was signed,
  - finds a newer stable version and installs it from the About dialog
    (pkexec + the update helper + APT), then asks for a restart.

    tools/test_updates.py --deb dist/hallow_X_amd64.deb --out update-test
"""

import argparse
import os
import shutil
import subprocess
import sys
import tempfile
import time
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
from marionette import Marionette  # noqa: E402

ROOT = Path(__file__).resolve().parent.parent
SOURCES = "/etc/apt/sources.list.d/hallow.sources"
KEYRING = "/usr/share/keyrings/hallow-archive-keyring.asc"
PORT = 8765


def sh(*cmd, check=True, **kwargs):
    print("+", " ".join(str(c) for c in cmd), flush=True)
    return subprocess.run([str(c) for c in cmd], check=check, text=True, **kwargs)


def root(*cmd, **kwargs):
    return sh(*(["sudo"] if os.geteuid() != 0 else []), *cmd, **kwargs)


def write_root(path, content):
    tmp = tempfile.NamedTemporaryFile("w", delete=False)
    tmp.write(content)
    tmp.close()
    root("install", "-m", "644", tmp.name, path)
    os.unlink(tmp.name)


def dpkg_version():
    return sh(
        "dpkg-query", "-W", "-f=${Version}", "hallow", capture_output=True
    ).stdout.strip()


def repack(deb, version, out, marker):
    """The package `deb` as `version`, with a marker file to tell it apart."""
    work = Path(tempfile.mkdtemp())
    sh("dpkg-deb", "-R", deb, work / "pkg")
    control = (work / "pkg/DEBIAN/control").read_text()
    control = "\n".join(
        f"Version: {version}" if line.startswith("Version:") else line
        for line in control.splitlines()
    )
    (work / "pkg/DEBIAN/control").write_text(control + "\n")
    (work / "pkg/usr/lib/hallow/update-test-marker").write_text(marker + "\n")
    out.mkdir(parents=True, exist_ok=True)
    target = out / f"hallow_{version}_amd64.deb"
    sh("dpkg-deb", "--root-owner-group", "-Zxz", "-b", work / "pkg", target)
    shutil.rmtree(work)
    return target


def make_repo(deb, dest, key):
    """A channel at <served root>/<name>/latest/download/, like GitHub's.
    `key` None leaves it unsigned."""
    sh(ROOT / "tools/make-apt-repo.sh", deb, dest, key or "-")


def gen_key(name):
    sh(
        "gpg", "--batch", "--pinentry-mode", "loopback", "--passphrase", "",
        "--quick-gen-key", name, "ed25519", "sign", "never",
    )
    return name


def set_channel(scenario):
    write_root(
        SOURCES,
        "Types: deb\n"
        f"URIs: http://127.0.0.1:{PORT}/{scenario}/latest/\n"
        "Suites: download/\n"
        f"Signed-By: {KEYRING}\n",
    )


class Browser:
    def __init__(self, module_dir, shots):
        self.profile = tempfile.mkdtemp()
        self.shots = shots
        self.proc = subprocess.Popen(
            [
                "/usr/lib/hallow/hallow", "--new-instance", "--profile",
                self.profile, "--marionette", "--remote-allow-system-access",
                "about:blank",
            ],
            stdout=subprocess.DEVNULL,
            stderr=subprocess.STDOUT,
        )
        self.m = Marionette()
        self.m.chrome()
        if module_dir:
            # Development: load the module from a directory instead of the
            # build (used to test it against an older Hallow build).
            self.module = "resource://hallowtest/LinuxPackageUpdater.sys.mjs"
            self.m.run(
                """
                const dir = Cc["@mozilla.org/file/local;1"].createInstance(Ci.nsIFile);
                dir.initWithPath(args[0]);
                Services.io.getProtocolHandler("resource")
                  .QueryInterface(Ci.nsIResProtocolHandler)
                  .setSubstitution("hallowtest", Services.io.newFileURI(dir));
                """,
                [str(Path(module_dir).resolve())],
            )
        else:
            self.module = "resource://gre/modules/LinuxPackageUpdater.sys.mjs"

    def call(self, method):
        """LinuxPackageUpdater.<method>(); {ok, value} or {ok: false, error}."""
        return self.m.run(
            """
            const { LinuxPackageUpdater } = ChromeUtils.importESModule(args[0]);
            try {
              return { ok: true, value: await LinuxPackageUpdater[args[1]]() };
            } catch (e) {
              return { ok: false, error: String(e) };
            }
            """,
            [self.module, method],
        )

    def screenshot(self, name):
        path = self.shots / f"{name}.png"
        sh("import", "-window", "root", path, check=False)

    def close(self):
        self.m.quit()
        try:
            self.proc.wait(timeout=30)
        except subprocess.TimeoutExpired:
            self.proc.kill()
        shutil.rmtree(self.profile, ignore_errors=True)


def wait_panel(browser, panels, timeout=900):
    """Wait until the About dialog shows one of `panels`; return
    (panel id, its button label)."""
    return browser.m.run(
        """
        const [wanted, timeout] = args;
        const deadline = Date.now() + timeout * 1000;
        for (;;) {
          const win = Services.wm.getMostRecentWindow("Browser:About");
          const panel = win?.document.getElementById("updateDeck")?.selectedPanel;
          if (panel && wanted.includes(panel.id)) {
            return [panel.id, panel.querySelector("button")?.label ?? ""];
          }
          if (Date.now() > deadline) {
            throw new Error(`About dialog stuck at ${panel?.id}`);
          }
          await new Promise(r => setTimeout(r, 250));
        }
        """,
        [panels, timeout],
    )


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--deb", required=True, type=Path)
    parser.add_argument("--out", required=True, type=Path)
    parser.add_argument("--module-dir", help="load LinuxPackageUpdater from here")
    parser.add_argument(
        "--no-ui", action="store_true", help="skip the About dialog (old builds)"
    )
    args = parser.parse_args()

    out = args.out.resolve()
    shots = out / "screenshots"
    shots.mkdir(parents=True, exist_ok=True)
    os.environ["GNUPGHOME"] = tempfile.mkdtemp()
    os.chmod(os.environ["GNUPGHOME"], 0o700)

    current = dpkg_version()
    print(f"installed: hallow {current}")
    nxt = f"{current}+updatetest"
    older = f"{current}~older"

    stable_key = gen_key("Hallow update test (stable)")
    rogue_key = gen_key("Hallow update test (rogue)")
    public = sh("gpg", "--armor", "--export", stable_key, capture_output=True).stdout
    write_root(KEYRING, public)

    debs = out / "debs"
    next_deb = repack(args.deb, nxt, debs, "next")
    older_deb = repack(args.deb, older, debs, "older")
    evil_deb = repack(args.deb, nxt, debs / "evil", "tampered")

    served = out / "served"
    shutil.rmtree(served, ignore_errors=True)
    channel = lambda name: served / name / "latest/download"  # noqa: E731
    make_repo(next_deb, channel("stable"), stable_key)
    make_repo(next_deb, channel("rogue"), rogue_key)
    make_repo(next_deb, channel("unsigned"), None)
    make_repo(older_deb, channel("older"), stable_key)
    make_repo(next_deb, channel("tampered"), stable_key)
    # Replace the package after the channel was signed.
    shutil.copy(evil_deb, channel("tampered") / next_deb.name)

    server = subprocess.Popen(
        [sys.executable, "-m", "http.server", str(PORT), "--bind", "127.0.0.1",
         "--directory", str(served)],
        stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL,
    )
    failures = []

    def expect(name, condition, detail):
        print(f"{'PASS' if condition else 'FAIL'} {name}: {detail}", flush=True)
        if not condition:
            failures.append(name)

    browser = Browser(args.module_dir, shots)
    try:
        for scenario in ("rogue", "unsigned"):
            set_channel(scenario)
            result = browser.call("check")
            expect(
                f"{scenario} channel is rejected",
                not result["ok"],
                result.get("error") or result.get("value"),
            )

        set_channel("older")
        result = browser.call("check")
        expect(
            "older version is not offered",
            result["ok"] and result["value"]["status"] == "up-to-date",
            result,
        )

        set_channel("tampered")
        result = browser.call("check")
        expect(
            "tampered channel still lists the update",
            result["ok"] and result["value"]["status"] == "available",
            result,
        )
        result = browser.call("install")
        expect(
            "tampered package is refused",
            not result["ok"] and dpkg_version() == current,
            result.get("error") or result.get("value"),
        )

        set_channel("stable")
        result = browser.call("check")
        expect(
            "stable channel offers the newer version",
            result["ok"]
            and result["value"] == {"status": "available", "version": nxt},
            result,
        )

        if args.no_ui:
            result = browser.call("install")
            expect("update installs", result == {"ok": True, "value": "installed"}, result)
        else:
            browser.m.run(
                "Services.wm.getMostRecentWindow('navigator:browser').openAboutDialog();"
            )
            panel, label = wait_panel(browser, ["downloadAndInstall", "checkingFailed",
                                                "noUpdatesFound", "noUpdater"])
            browser.screenshot("about-update-available")
            expect(
                "About dialog offers the update",
                panel == "downloadAndInstall" and nxt in label,
                f"{panel}: {label}",
            )
            browser.m.run(
                "Services.wm.getMostRecentWindow('Browser:About').gAppUpdater.startDownload();"
            )
            panel, label = wait_panel(browser, ["apply", "downloadFailed", "internalError"])
            browser.screenshot("about-update-installed")
            expect(
                "About dialog installs it and asks to restart",
                panel == "apply",
                f"{panel}: {label}",
            )
        marker = Path("/usr/lib/hallow/update-test-marker")
        expect(
            "the new package is installed",
            dpkg_version() == nxt and marker.read_text().strip() == "next",
            dpkg_version(),
        )
    finally:
        browser.close()
        server.terminate()

    if failures:
        print(f"{len(failures)} update test(s) failed: {', '.join(failures)}")
        sys.exit(1)
    print("all update tests passed")


if __name__ == "__main__":
    main()
