#!/usr/bin/env python3
"""End-to-end test of Hallow's built-in updates on Windows.

Run on a Windows machine (CI runner) where the Hallow build under test is
installed. It serves update manifests and the packages made by
tools/make-windows-update-tests.sh from this machine, points the installed
Hallow at them (AppUpdateURL policy) and checks, with Gecko's real update
service and updater, that Hallow

  - never offers an older build, the same build, or an update that does not
    say which build it is (patches/0012),
  - never downloads an update package over plain HTTP from another host,
  - downloads but refuses to install a package signed with another key, an
    unsigned or tampered package, a package for another MAR channel and a
    package for an older Firefox version,
  - finds a later build and installs it from the About dialog without any
    prompt, then runs it after the restart.

    tools/test_windows_updates.py --install DIR --packages DIR --out DIR
"""

import argparse
import hashlib
import http.server
import json
import os
import re
import shutil
import subprocess
import sys
import tempfile
import threading
import time
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
from marionette import Marionette  # noqa: E402

PORT = 8765
BASE = f"http://127.0.0.1:{PORT}"


class Handler(http.server.SimpleHTTPRequestHandler):
    """Static files with byte ranges (Gecko and BITS download in ranges)."""

    def log_message(self, fmt, *args):
        print("http:", fmt % args, flush=True)

    def send_head(self):
        path = Path(self.translate_path(self.path))
        if not path.is_file():
            self.send_error(404)
            return None
        size = path.stat().st_size
        start, end = 0, size - 1
        header = self.headers.get("Range")
        if header and header.startswith("bytes="):
            first, _, last = header[6:].split(",")[0].partition("-")
            if first:
                start = int(first)
                end = min(int(last), size - 1) if last else size - 1
            else:
                start = max(0, size - int(last))
            if start >= size:
                self.send_error(416)
                return None
            self.send_response(206)
            self.send_header("Content-Range", f"bytes {start}-{end}/{size}")
        else:
            self.send_response(200)
        ctype = "text/xml" if path.suffix == ".xml" else "application/octet-stream"
        self.send_header("Content-Type", ctype)
        self.send_header("Content-Length", str(end - start + 1))
        self.send_header("Accept-Ranges", "bytes")
        self.send_header("Cache-Control", "no-store")
        self.end_headers()
        f = open(path, "rb")
        f.seek(start)
        self._remaining = end - start + 1
        return f

    def copyfile(self, source, outputfile):
        remaining = getattr(self, "_remaining", None)
        while remaining is None or remaining > 0:
            chunk = source.read(1 << 20 if remaining is None else min(1 << 20, remaining))
            if not chunk:
                break
            outputfile.write(chunk)
            if remaining is not None:
                remaining -= len(chunk)


def update_xml(offers):
    """An update manifest offering `offers` (dicts of attributes)."""
    items = []
    for o in offers:
        patch = ""
        if o.get("mar"):
            patch = (
                f'<patch type="complete" URL="{o["url"]}" size="{o["size"]}" '
                f'hashFunction="sha512" hashValue="{o["sha512"]}"/>'
            )
        attrs = " ".join(
            f'{k}="{v}"'
            for k, v in [
                ("type", "minor"),
                ("displayVersion", o["display"]),
                ("appVersion", o["appVersion"]),
                ("platformVersion", o["appVersion"]),
                ("buildID", o.get("buildID")),
                ("detailsURL", "https://github.com/YAD-ctrlz/Hallow-browser/releases"),
            ]
            if v is not None
        )
        items.append(f"  <update {attrs}>{patch}</update>\n")
    return '<?xml version="1.0" encoding="UTF-8"?>\n<updates>\n' + "".join(items) + "</updates>\n"


def ini_value(path, key):
    section = None
    for line in Path(path).read_text().splitlines():
        line = line.strip()
        if line.startswith("["):
            section = line
        elif section == "[App]" and line.startswith(key + "="):
            return line.split("=", 1)[1]
    return None


def screenshot(shots, name):
    time.sleep(2)  # let the last change paint
    try:
        from PIL import ImageGrab

        ImageGrab.grab().save(shots / f"{name}.png")
    except Exception as e:  # no desktop session
        print(f"screenshot {name} failed: {e}")


class Browser:
    def __init__(self, exe, profile):
        self.exe = exe
        self.profile = profile
        self.proc = subprocess.Popen(
            [str(exe), "-no-remote", "-profile", str(profile), "--marionette",
             "--remote-allow-system-access", "about:blank"],
        )
        self.connect()

    def connect(self):
        self.m = Marionette(timeout=180)
        self.m.chrome()

    def run(self, script, args=None):
        return self.m.run(script, args)

    def close(self):
        self.m.quit()
        try:
            self.proc.wait(timeout=30)
        except subprocess.TimeoutExpired:
            pass
        kill_hallow()


def update_dirs():
    root = Path(os.environ.get("PROGRAMDATA", "C:/ProgramData"))
    return list(root.glob("Mozilla-*/updates/*/updates"))


def last_update_log():
    """The updater's log of the last update it applied or staged."""
    logs = [p for d in update_dirs() for p in (d / "last-update.log", d / "0/update.log")
            if p.exists()]
    if not logs:
        return ""
    newest = max(logs, key=lambda p: p.stat().st_mtime)
    return newest.read_text(errors="replace")


def kill_hallow():
    subprocess.run(["taskkill", "/F", "/T", "/IM", "hallow.exe"], capture_output=True)
    time.sleep(2)


CHECK_AND_INSTALL = """
const [timeoutMs] = args;
const aus = Cc["@mozilla.org/updates/update-service;1"]
  .getService(Ci.nsIApplicationUpdateService);
const um = Cc["@mozilla.org/updates/update-manager;1"]
  .getService(Ci.nsIUpdateManager);
const checker = Cc["@mozilla.org/updates/update-checker;1"]
  .getService(Ci.nsIUpdateChecker);
const result = { canApply: aus.canApplyUpdates, canStage: aus.canStageUpdates,
                 disabled: aus.disabled };
const check = await checker.checkForUpdates(checker.FOREGROUND_CHECK).result;
result.checkSucceeded = check.succeeded;
result.listed = check.updates.length;
const update = check.succeeded ? await aus.selectUpdate(check.updates) : null;
result.offered = !!update;
if (!update) {
  return result;
}
result.displayVersion = update.displayVersion;
result.download = await aus.downloadUpdate(update);
const deadline = Date.now() + timeoutMs;
const stateName = s => aus.getStateName(s);
const settled = [Ci.nsIApplicationUpdateService.STATE_PENDING,
                 Ci.nsIApplicationUpdateService.STATE_IDLE,
                 Ci.nsIApplicationUpdateService.STATE_DOWNLOAD_FAILED];
result.states = [stateName(aus.currentState)];
while (!settled.includes(aus.currentState) && Date.now() < deadline) {
  await Promise.race([aus.stateTransition,
                      new Promise(r => setTimeout(r, 1000))]);
  const name = stateName(aus.currentState);
  if (result.states.at(-1) != name) result.states.push(name);
}
result.finalState = stateName(aus.currentState);
const ready = await um.getReadyUpdate();
result.ready = ready ? { state: ready.state, errorCode: ready.errorCode } : null;
const [last] = await um.getHistory();
result.history = last ? { state: last.state, errorCode: last.errorCode,
                          displayVersion: last.displayVersion } : null;
return result;
"""


def wait_panel(browser, panels, timeout=900):
    """Wait until the About dialog shows one of `panels`; return
    (panel id, its button label)."""
    return browser.run(
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
    parser.add_argument("--install", required=True, type=Path)
    parser.add_argument("--packages", required=True, type=Path,
                        help="output of make-windows-update-tests.sh")
    parser.add_argument("--out", required=True, type=Path)
    parser.add_argument(
        "--next-mar", type=Path,
        help="install this signed package as the later build instead of "
             "next.mar (release builds: the build's own package, signed with "
             "the release key; it has no update-test marker)")
    args = parser.parse_args()

    install = args.install.resolve()
    exe = install / "hallow.exe"
    out = args.out.resolve()
    shots = out / "screenshots"
    shots.mkdir(parents=True, exist_ok=True)
    served = Path(tempfile.mkdtemp(prefix="hallow-updates-"))
    (served / "mars").mkdir()

    build = json.loads((args.packages / "build.json").read_text())
    installed_id = ini_value(install / "application.ini", "BuildID")
    app_version = ini_value(install / "application.ini", "Version")
    print(f"installed: Hallow {build['version']} (Gecko {app_version}, build {installed_id})")
    later_id = str(int(installed_id) + 1)
    older_id = str(int(installed_id) - 1000000)  # a day earlier

    mars = {}
    for name in ["next", "rogue", "unsigned", "tampered", "wrong-channel", "downgrade"]:
        src = args.packages / f"{name}.mar"
        if name == "next" and args.next_mar:
            src = args.next_mar
        if not src.exists():
            # Release builds: only the build's own (production) key could
            # sign these, and the test never has it.
            print(f"INFO no {name}.mar: tests needing it are skipped")
            continue
        dest = served / "mars" / f"{name}.mar"
        shutil.copy(src, dest)
        data = dest.read_bytes()
        mars[name] = {
            "mar": name,
            "url": f"{BASE}/mars/{name}.mar",
            "size": len(data),
            "sha512": hashlib.sha512(data).hexdigest(),
        }

    # Any package will do for updates that must not even be offered.
    plain = "next" if "next" in mars else "unsigned"

    def offer(mar, build_id=later_id, display=None, **extra):
        o = dict(mars[mar])
        o.update(appVersion=app_version, buildID=build_id,
                 display=display or f"{build['version']} update test ({mar})")
        o.update(extra)
        return o

    def set_manifest(offers):
        (served / "update.xml").write_text(update_xml(offers))

    # Point Hallow at this machine with the enterprise policy Firefox has
    # for that; nothing else about the installation changes.
    policies = install / "distribution" / "policies.json"
    policies.parent.mkdir(exist_ok=True)
    policies.write_text(json.dumps({"policies": {"AppUpdateURL": f"{BASE}/update.xml"}}))

    profile = Path(tempfile.mkdtemp(prefix="hallow-profile-"))
    # Marionette normally turns off application updates; this test needs them.
    (profile / "user.js").write_text(
        'user_pref("remote.prefs.recommended", false);\n'
        'user_pref("app.update.disabledForTesting", false);\n'
        'user_pref("app.update.log", true);\n'
        'user_pref("browser.shell.checkDefaultBrowser", false);\n'
    )

    server = http.server.ThreadingHTTPServer(
        ("127.0.0.1", PORT), lambda *a: Handler(*a, directory=str(served))
    )
    threading.Thread(target=server.serve_forever, daemon=True).start()

    failures = []

    def expect(name, condition, detail):
        print(f"{'PASS' if condition else 'FAIL'} {name}: {detail}", flush=True)
        if not condition:
            failures.append(name)

    marker = install / "update-test-marker.txt"
    browser = Browser(exe, profile)
    try:
        # --- Updates while Hallow is closed -------------------------------------
        # The background update task may update this per-user installation
        # (prefs/hallow.js); Firefox's own reasons not to would show here.
        reasons = browser.run(
            """
            const { BackgroundUpdate } = ChromeUtils.importESModule(
              "resource://gre/modules/BackgroundUpdate.sys.mjs");
            return await BackgroundUpdate._reasonsToNotUpdateInstallation();
            """
        )
        expect("the background update task may update Hallow while it is closed",
               not any("maintenance service" in r or "not writable" in r for r in reasons),
               reasons)

        # --- Updates that must not be offered ---------------------------------
        for name, offers in [
            ("an older build of this version", [offer(plain, build_id=older_id)]),
            ("the same build", [offer(plain, build_id=installed_id)]),
            ("an update without a build ID", [offer(plain, build_id=None)]),
            ("a package served over plain HTTP from another host",
             [offer(plain, url="http://hallow-update-test.invalid/next.mar")]),
        ]:
            set_manifest(offers)
            result = browser.run(CHECK_AND_INSTALL, [60000])
            expect(f"not offered: {name}",
                   result["checkSucceeded"] and not result["offered"], result)

        # --- Packages that must be refused by the updater -----------------------
        # The updater's error codes (toolkit/mozapps/update/common/updatererrors.h).
        for mar, why, code in [
            ("rogue", "signed with another key", 19),  # CERT_VERIFY_ERROR
            ("unsigned", "not signed", 19),
            ("tampered", "changed after signing", 19),
            ("wrong-channel", "for Firefox's update channel", 22),  # MAR_CHANNEL_MISMATCH_ERROR
            ("downgrade", "for an older Firefox version", 23),  # VERSION_DOWNGRADE_ERROR
        ]:
            if mar not in mars:
                continue
            set_manifest([offer(mar)])
            result = browser.run(CHECK_AND_INSTALL, [300000])
            result["updaterLog"] = last_update_log()[-1500:]
            errors = {result["history"]["errorCode"]} if result.get("history") else set()
            errors |= {int(c) for c in re.findall(r"failed: (\d+)", result["updaterLog"])}
            refused = (
                result["offered"]
                and result["finalState"] != "STATE_PENDING"
                and not (result["ready"] or {}).get("state", "").startswith("applied")
                and not marker.exists()
            )
            expect(f"refused: package {why}", refused and code in errors,
                   {k: v for k, v in result.items() if k != "updaterLog"})
            print(result["updaterLog"])

        # --- A later build installs from the About dialog ---------------------
        if "next" not in mars:
            return finish(failures)
        set_manifest([offer("next", display=f"{build['version']} (update test)")])
        browser.run(
            "Services.wm.getMostRecentWindow('navigator:browser').openAboutDialog();"
        )
        panel, label = wait_panel(
            browser,
            ["apply", "downloadFailed", "checkingFailed", "noUpdatesFound",
             "internalError", "manualUpdate", "unsupportedSystem"],
        )
        screenshot(shots, "about-update-ready")
        expect("About dialog downloads the update and asks to restart",
               panel == "apply", f"{panel}: {label}")

        # After the restart the update is installed; the manifest then offers
        # nothing newer.
        set_manifest([])
        if panel == "apply":
            try:
                browser.run(
                    """
                    const win = Services.wm.getMostRecentWindow("Browser:About");
                    setTimeout(() => win.document.getElementById("updateButton").click(), 500);
                    """
                )
            except (RuntimeError, OSError, ConnectionError):
                pass
            # The updater replaces the files and starts Hallow again with the
            # same arguments (including --marionette).
            time.sleep(10)
            browser.connect()
            info = browser.run(
                """
                const um = Cc["@mozilla.org/updates/update-manager;1"]
                  .getService(Ci.nsIUpdateManager);
                const installed = await um.updateInstalledAtStartup();
                const [last] = await um.getHistory();
                return {
                  installedAtStartup: installed && { state: installed.state,
                    displayVersion: installed.displayVersion },
                  last: last && { state: last.state, errorCode: last.errorCode },
                };
                """
            )
            expect(
                "the update is installed and Hallow restarted",
                (args.next_mar or (marker.exists() and marker.read_text().strip() == "next"))
                and info["installedAtStartup"] is not None
                and info["installedAtStartup"]["state"] == "succeeded",
                info,
            )
            browser.run(
                "Services.wm.getMostRecentWindow('navigator:browser').openAboutDialog();"
            )
            panel, label = wait_panel(
                browser, ["noUpdatesFound", "apply", "checkingFailed", "downloadFailed"],
                timeout=120,
            )
            screenshot(shots, "about-up-to-date")
            expect("About dialog then reports Hallow up to date",
                   panel == "noUpdatesFound", f"{panel}: {label}")
    finally:
        try:
            browser.close()
        except Exception as e:
            print("closing the browser:", e)
            kill_hallow()
        server.shutdown()
        policies.unlink(missing_ok=True)
        for d in update_dirs():
            for name in ["last-update.log", "backup-update.log", "0/update.log"]:
                if (d / name).exists():
                    shutil.copy(d / name, out / name.replace("/", "-"))

    finish(failures)


def finish(failures):
    if failures:
        print(f"{len(failures)} update test(s) failed: {', '.join(failures)}")
        sys.exit(1)
    print("all update tests passed")


if __name__ == "__main__":
    main()
