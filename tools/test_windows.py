#!/usr/bin/env python3
"""Install and test a Hallow Windows build, as CI does on a Windows runner.

    python tools/test_windows.py --dist dist --out results

`dist` holds what the Windows build job uploads: hallow-*-win64-setup.exe,
the .zip, .complete.mar, .json, windows-icons/ and update-tests/. Stages:

  install       silent install, per user, without administrator rights
  contents      the installed files (updater, update channel, fonts, tiles)
  registration  Settings > Apps entry, browser registration, shortcuts (HKCU)
  icons         the icons in hallow.exe, the installer and the shortcuts are
                Hallow's, at every size Windows uses; how Windows draws them
  browser       tools/test_browser.py against the installed Hallow
  ui            screenshots of Hallow's windows and of the desktop/taskbar
  updates       tools/test_windows_updates.py (signed updates end to end)
  uninstall     the uninstaller removes Hallow and its registration

Every stage runs; results/summary.md lists each stage and check.
"""

import argparse
import json
import os
import struct
import subprocess
import sys
import time
import traceback
import winreg
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
sys.path.insert(0, str(Path(__file__).resolve().parent))

LOCALAPPDATA = Path(os.environ["LOCALAPPDATA"])
INSTALL = LOCALAPPDATA / "Hallow"
EXE = INSTALL / "hallow.exe"
UPDATE_URL = (
    "https://github.com/YAD-ctrlz/Hallow-browser/releases/latest/download/update-win64.xml"
)
# Every size Windows uses at each display scale (tools/hallow-build/src/icons.rs).
APP_ICON_SIZES = [16, 20, 24, 30, 32, 36, 40, 48, 60, 64, 72, 80, 96, 128, 256]
SMALL_ICON_SIZES = [16, 20, 24, 32, 40, 48, 64]
RT_ICON, RT_GROUP_ICON = 3, 14
IDI_APPICON = 1


class Results:
    def __init__(self, out):
        self.out = out
        self.stages = []
        self.current = None

    def stage(self, name):
        self.current = {"name": name, "checks": [], "error": None}
        self.stages.append(self.current)
        print(f"\n::group::stage {name}", flush=True)

    def expect(self, name, condition, detail=""):
        detail = detail if isinstance(detail, str) else json.dumps(detail, default=str)
        print(f"{'PASS' if condition else 'FAIL'} {name}: {detail}", flush=True)
        self.current["checks"].append((name, bool(condition), detail))

    def end(self):
        print("::endgroup::", flush=True)

    def failed(self, stage):
        return stage["error"] or any(not ok for _, ok, _ in stage["checks"])

    def write_summary(self):
        lines = ["# Hallow Windows tests", ""]
        for s in self.stages:
            lines.append(f"## {'FAIL' if self.failed(s) else 'PASS'} {s['name']}")
            for name, ok, detail in s["checks"]:
                lines.append(f"- {'PASS' if ok else 'FAIL'} {name}" + (
                    "" if ok else f": `{detail[:300]}`"))
            if s["error"]:
                lines.append(f"- ERROR `{s['error'][:500]}`")
            lines.append("")
        bad = [s["name"] for s in self.stages if self.failed(s)]
        lines.append("All stages passed." if not bad else f"Failed stages: {', '.join(bad)}")
        (self.out / "summary.md").write_text("\n".join(lines) + "\n")
        return not bad


def run(cmd, timeout=900, **kwargs):
    print("+", " ".join(str(c) for c in cmd), flush=True)
    return subprocess.run([str(c) for c in cmd], timeout=timeout, text=True, **kwargs)


def ini(path):
    sections, current = {}, None
    for line in Path(path).read_text(errors="replace").splitlines():
        line = line.strip()
        if line.startswith("[") and line.endswith("]"):
            current = sections.setdefault(line[1:-1], {})
        elif "=" in line and current is not None and not line.startswith(";"):
            k, v = line.split("=", 1)
            current[k.strip()] = v.strip()
    return sections


def reg_values(root, path):
    try:
        with winreg.OpenKey(root, path) as key:
            values, i = {}, 0
            while True:
                try:
                    name, value, _ = winreg.EnumValue(key, i)
                except OSError:
                    return values
                values[name] = value
                i += 1
    except OSError:
        return None


def reg_subkeys(root, path, view=0):
    try:
        with winreg.OpenKey(root, path, 0, winreg.KEY_READ | view) as key:
            names, i = [], 0
            while True:
                try:
                    names.append(winreg.EnumKey(key, i))
                except OSError:
                    return names
                i += 1
    except OSError:
        return []


def shortcut_target(lnk):
    script = (
        f"$s = (New-Object -ComObject WScript.Shell).CreateShortcut('{lnk}');"
        "Write-Output $s.TargetPath; Write-Output $s.IconLocation"
    )
    out = run(["powershell", "-NoProfile", "-Command", script], capture_output=True).stdout
    lines = out.strip().splitlines() + ["", ""]
    return lines[0].strip(), lines[1].strip()


def screenshot(out, name):
    try:
        from PIL import ImageGrab

        path = out / "screenshots" / f"{name}.png"
        path.parent.mkdir(parents=True, exist_ok=True)
        ImageGrab.grab().save(path)
        return path
    except Exception as e:
        print(f"screenshot {name} failed: {e}")


# --- Icons ----------------------------------------------------------------------


def icon_groups(path):
    """{group id: [(size, icon id)]} of a PE file's RT_GROUP_ICON resources,
    and {icon id: image bytes} of its RT_ICON resources."""
    import pefile

    pe = pefile.PE(str(path), fast_load=True)
    pe.parse_data_directories(
        directories=[pefile.DIRECTORY_ENTRY["IMAGE_DIRECTORY_ENTRY_RESOURCE"]]
    )
    groups, icons = {}, {}
    for kind in getattr(pe, "DIRECTORY_ENTRY_RESOURCE", None).entries:
        for res in kind.directory.entries:
            for lang in res.directory.entries:
                data = pe.get_data(lang.data.struct.OffsetToData, lang.data.struct.Size)
                if kind.id == RT_GROUP_ICON:
                    count = struct.unpack_from("<H", data, 4)[0]
                    entries = []
                    for i in range(count):
                        w, h, _, _, _, _, _, icon_id = struct.unpack_from(
                            "<BBBBHHIH", data, 6 + 14 * i)
                        entries.append((w or 256, icon_id))
                    groups[res.id if res.id is not None else str(res.name)] = entries
                elif kind.id == RT_ICON:
                    icons[res.id] = data
    pe.close()
    return groups, icons


def decode_icon_image(data):
    """A PIL image of one RT_ICON resource (PNG or 32-bit DIB)."""
    import io

    from PIL import Image

    if data[:4] == b"\x89PNG":
        return Image.open(io.BytesIO(data)).convert("RGBA")
    width, height2, _, bits = struct.unpack_from("<iiHH", data, 4)
    if bits != 32:
        raise ValueError(f"{bits}-bit icon image")
    size = width
    pixels = data[40:40 + size * size * 4]
    return Image.frombuffer("RGBA", (size, size), pixels, "raw", "BGRA", 0, -1)


def ico_images(path):
    """{size: PIL image} of an .ico file."""
    import io

    from PIL import Image

    data = Path(path).read_bytes()
    count = struct.unpack_from("<H", data, 4)[0]
    images = {}
    for i in range(count):
        w, _, _, _, _, _, size, offset = struct.unpack_from("<BBBBHHII", data, 6 + 16 * i)
        body = data[offset:offset + size]
        images[w or 256] = (
            Image.open(io.BytesIO(body)).convert("RGBA")
            if body[:4] == b"\x89PNG"
            else decode_icon_image(body)
        )
    return images


def shell_icon(path, size):
    """The icon Windows itself draws for `path` at `size` pixels (the
    executable's or shortcut's icon, picked and scaled by the shell)."""
    import ctypes
    from ctypes import wintypes

    from PIL import Image

    user32, gdi32 = ctypes.windll.user32, ctypes.windll.gdi32
    hicon = wintypes.HICON()
    icon_id = wintypes.UINT()
    user32.PrivateExtractIconsW.argtypes = [
        wintypes.LPCWSTR, ctypes.c_int, ctypes.c_int, ctypes.c_int,
        ctypes.POINTER(wintypes.HICON), ctypes.POINTER(wintypes.UINT),
        wintypes.UINT, wintypes.UINT]
    if user32.PrivateExtractIconsW(str(path), 0, size, size, ctypes.byref(hicon),
                                   ctypes.byref(icon_id), 1, 0) != 1:
        return None

    class ICONINFO(ctypes.Structure):
        _fields_ = [("fIcon", wintypes.BOOL), ("xHotspot", wintypes.DWORD),
                    ("yHotspot", wintypes.DWORD), ("hbmMask", wintypes.HBITMAP),
                    ("hbmColor", wintypes.HBITMAP)]

    class BITMAPINFOHEADER(ctypes.Structure):
        _fields_ = [("biSize", wintypes.DWORD), ("biWidth", wintypes.LONG),
                    ("biHeight", wintypes.LONG), ("biPlanes", wintypes.WORD),
                    ("biBitCount", wintypes.WORD), ("biCompression", wintypes.DWORD),
                    ("biSizeImage", wintypes.DWORD), ("biXPelsPerMeter", wintypes.LONG),
                    ("biYPelsPerMeter", wintypes.LONG), ("biClrUsed", wintypes.DWORD),
                    ("biClrImportant", wintypes.DWORD)]

    info = ICONINFO()
    user32.GetIconInfo(hicon, ctypes.byref(info))
    header = BITMAPINFOHEADER(40, size, -size, 1, 32, 0, 0, 0, 0, 0, 0)
    buf = ctypes.create_string_buffer(size * size * 4)
    dc = user32.GetDC(None)
    gdi32.GetDIBits(dc, info.hbmColor, 0, size, buf, ctypes.byref(header), 0)
    user32.ReleaseDC(None, dc)
    gdi32.DeleteObject(info.hbmColor)
    gdi32.DeleteObject(info.hbmMask)
    user32.DestroyIcon(hicon)
    return Image.frombuffer("RGBA", (size, size), buf.raw, "raw", "BGRA", 0, 1)


def icon_sheet(rows, path):
    """Rows of (label, [PIL images]) drawn on light and dark backgrounds."""
    from PIL import Image, ImageDraw

    pad, label_w = 10, 170
    blocks = []
    for bg in ["#f3f3f3", "#202020"]:
        for label, images in rows:
            images = [i for i in images if i is not None]
            scale = [2 if i.width <= 48 else 1 for i in images]
            w = label_w + sum(i.width * s + pad for i, s in zip(images, scale)) + pad
            h = max([i.height * s for i, s in zip(images, scale)] + [20]) + 2 * pad
            block = Image.new("RGBA", (w, h), bg)
            ImageDraw.Draw(block).text(
                (pad, pad), label, fill="#000000" if bg == "#f3f3f3" else "#ffffff")
            x = label_w
            for img, s in zip(images, scale):
                img = img.resize((img.width * s, img.height * s), Image.NEAREST)
                block.alpha_composite(img, (x, pad))
                x += img.width + pad
            blocks.append(block)
    sheet = Image.new("RGBA", (max(b.width for b in blocks), sum(b.height for b in blocks)),
                      "#808080")
    y = 0
    for b in blocks:
        sheet.alpha_composite(b, (0, y))
        y += b.height
    path.parent.mkdir(parents=True, exist_ok=True)
    sheet.convert("RGB").save(path)


# --- Stages ---------------------------------------------------------------------


def stage_install(r, dist):
    setup = next(dist.glob("hallow-*-win64-setup.exe"))
    started = time.time()
    proc = run([setup, "/S"], timeout=600)
    # The installer can finish its last steps in a child process.
    deadline = time.time() + 120
    while not EXE.exists() and time.time() < deadline:
        time.sleep(1)
    r.expect("silent installer exits successfully", proc.returncode == 0, proc.returncode)
    r.expect("installed per user in %LOCALAPPDATA%\\Hallow", EXE.exists(), str(EXE))
    r.expect("nothing installed under Program Files",
             not Path(os.environ.get("ProgramFiles", "C:/Program Files"), "Hallow").exists())
    r.expect("installation takes under two minutes", time.time() - started < 120,
             f"{time.time() - started:.0f} s")


def stage_contents(r, dist):
    build = json.loads(next(dist.glob("hallow-*-win64.json")).read_text())
    for name in [
        "hallow.exe", "private_browsing.exe", "updater.exe", "uninstall/helper.exe",
        "application.ini", "update-settings.ini", "omni.ja", "browser/omni.ja",
        "fonts/IBMPlexSans-Variable.ttf", "fonts/IBMPlexSans-Italic-Variable.ttf",
        "fonts/SpaceGrotesk-Variable.ttf", "hallow.VisualElementsManifest.xml",
        "browser/VisualElements/VisualElements_150.png",
        "browser/VisualElements/VisualElements_70.png",
    ]:
        r.expect(f"installs {name}", (INSTALL / name).exists())
    for name in [
        "maintenanceservice.exe", "maintenanceservice_installer.exe",
        "default-browser-agent.exe", "crashreporter.exe", "firefox.exe",
    ]:
        r.expect(f"does not install {name}", not (INSTALL / name).exists())
    app = ini(INSTALL / "application.ini")
    r.expect("Gecko version matches the build", app["App"].get("Version") == build["appVersion"],
             app["App"].get("Version"))
    r.expect("Hallow's update channel, never Mozilla's",
             app.get("AppUpdate", {}).get("URL") == UPDATE_URL, app.get("AppUpdate"))
    settings = ini(INSTALL / "update-settings.ini")
    r.expect("updater accepts only Hallow's MAR channel",
             settings.get("Settings", {}).get("ACCEPTED_MAR_CHANNEL_IDS") == "hallow-release",
             settings)
    manifest = (INSTALL / "hallow.VisualElementsManifest.xml").read_text()
    r.expect("Start tile uses Hallow's colors", "#120c24" in manifest, manifest[-200:])


def stage_registration(r, dist):
    uninstall = r"Software\Microsoft\Windows\CurrentVersion\Uninstall"
    keys = [k for k in reg_subkeys(winreg.HKEY_CURRENT_USER, uninstall) if "Hallow" in k]
    r.expect("Settings > Apps entry for the current user", len(keys) == 1, keys)
    if keys:
        v = reg_values(winreg.HKEY_CURRENT_USER, f"{uninstall}\\{keys[0]}") or {}
        r.expect("listed as Hallow", str(v.get("DisplayName", "")).startswith("Hallow"),
                 v.get("DisplayName"))
        r.expect("published by Hallow", v.get("Publisher") == "Hallow", v.get("Publisher"))
        r.expect("Hallow's icon in Settings > Apps",
                 str(v.get("DisplayIcon", "")).lower().startswith(str(EXE).lower()),
                 v.get("DisplayIcon"))
        r.expect("uninstalls with Hallow's uninstaller",
                 "uninstall\\helper.exe" in str(v.get("UninstallString", "")).lower(),
                 v.get("UninstallString"))
        links = {k: v.get(k) for k in ["URLInfoAbout", "HelpLink", "URLUpdateInfo"]}
        r.expect("links to Hallow, not Mozilla",
                 all("YAD-ctrlz/Hallow-browser" in str(u) for u in links.values()), links)
    for view in (winreg.KEY_WOW64_64KEY, winreg.KEY_WOW64_32KEY):
        machine = [k for k in reg_subkeys(winreg.HKEY_LOCAL_MACHINE, uninstall, view)
                   if "Hallow" in k]
        r.expect("nothing registered machine-wide (HKLM)", not machine, machine)
    clients = r"Software\Clients\StartMenuInternet"
    browsers = [k for k in reg_subkeys(winreg.HKEY_CURRENT_USER, clients)
                if (reg_values(winreg.HKEY_CURRENT_USER, f"{clients}\\{k}") or {}).get("")
                == "Hallow"]
    r.expect("registered as a browser for Default apps", len(browsers) == 1, browsers)
    if browsers:
        caps = reg_values(winreg.HKEY_CURRENT_USER,
                          f"{clients}\\{browsers[0]}\\Capabilities") or {}
        r.expect("Default apps shows Hallow", caps.get("ApplicationName") == "Hallow", caps)

    desktop = Path(os.environ["USERPROFILE"]) / "Desktop" / "Hallow.lnk"
    start = (Path(os.environ["APPDATA"]) / "Microsoft/Windows/Start Menu/Programs/Hallow.lnk")
    for name, lnk in [("desktop", desktop), ("Start menu", start)]:
        target, icon = shortcut_target(lnk) if lnk.exists() else ("", "")
        r.expect(f"{name} shortcut opens Hallow",
                 lnk.exists() and target.lower() == str(EXE).lower(), f"{lnk}: {target} {icon}")


def expected_icons(dist, out):
    """The .ico files the build rendered (uploaded with it), or, for builds
    that did not upload them, rendered again by this checkout's hallow-build."""
    if (dist / "windows-icons" / "firefox.ico").exists():
        return dist / "windows-icons"
    rendered = out / "rendered-icons"
    run(["cargo", "run", "--release", "-q", "-p", "hallow-build", "--", "icons", rendered],
        timeout=1200, cwd=ROOT)
    return rendered / "windows"


def stage_icons(r, dist, out):
    shots = out / "icons"
    reference = expected_icons(dist, out)
    groups, icons = icon_groups(EXE)
    app = groups.get(IDI_APPICON, [])
    r.expect("hallow.exe has an icon for every Windows size",
             sorted(s for s, _ in app) == APP_ICON_SIZES, sorted(s for s, _ in app))
    expected = ico_images(reference / "firefox.ico")
    same = all(
        decode_icon_image(icons[i]).tobytes() == expected[s].tobytes()
        for s, i in app if s in expected
    )
    r.expect("hallow.exe's icon is Hallow's (pixel for pixel)", bool(app) and same)
    pb_groups, pb_icons = icon_groups(INSTALL / "private_browsing.exe")
    pb = pb_groups.get(IDI_APPICON, [])
    expected_pb = ico_images(reference / "pbmode.ico")
    r.expect("private browsing has Hallow's private icon",
             bool(pb) and all(decode_icon_image(pb_icons[i]).tobytes()
                              == expected_pb[s].tobytes() for s, i in pb),
             sorted(s for s, _ in pb))
    setup = next(dist.glob("hallow-*-win64-setup.exe"))
    s_groups, s_icons = icon_groups(setup)
    sizes = sorted(s for g in s_groups.values() for s, _ in g)
    r.expect("the installer has Hallow's icon", sizes == SMALL_ICON_SIZES, sizes)

    # How Windows draws them: the shell picks and scales the icon for each
    # size, as for the taskbar, title bars, Start and Explorer.
    shell_sizes = [16, 20, 24, 30, 32, 36, 40, 48, 64, 96, 128, 256]
    drawn = {s: shell_icon(EXE, s) for s in shell_sizes}
    r.expect("Windows draws hallow.exe's icon at every size",
             all(img is not None and img.getbbox() for img in drawn.values()),
             [s for s, i in drawn.items() if i is None])
    # The artwork fills the icon like other apps' (apparent size).
    big = drawn.get(256)
    if big is not None:
        bbox = big.getchannel("A").point(lambda a: 255 if a > 128 else 0).getbbox()
        r.expect("the icon fills its canvas", bbox and min(bbox[2] - bbox[0], bbox[3] - bbox[1])
                 >= 0.82 * 256, bbox)
    desktop = Path(os.environ["USERPROFILE"]) / "Desktop" / "Hallow.lnk"
    icon_sheet(
        [
            ("hallow.exe (shell)", [drawn[s] for s in shell_sizes]),
            ("private browsing", [shell_icon(INSTALL / "private_browsing.exe", s)
                                  for s in [16, 24, 32, 48, 256]]),
            ("installer", [shell_icon(setup, s) for s in [16, 24, 32, 48]]),
            ("desktop shortcut", [shell_icon(desktop, s) for s in [16, 32, 48]]
             if desktop.exists() else []),
        ],
        shots / "icons.png",
    )
    print(f"icon sheet: {shots / 'icons.png'}")


def stage_browser(r, dist, out):
    proc = run([sys.executable, ROOT / "tools/test_browser.py", "--binary", EXE,
                "--out", out / "browser"], timeout=1200, capture_output=True)
    print(proc.stdout[-20000:])
    print(proc.stderr[-5000:])
    for line in proc.stdout.splitlines():
        if line.startswith(("PASS ", "FAIL ")):
            status, _, rest = line.partition(" ")
            name, _, detail = rest.partition(": ")
            r.expect(name, status == "PASS", detail)
    r.expect("browser checks ran to the end", proc.returncode == 0
             or "browser check(s) failed" in proc.stdout, proc.returncode)


def stage_ui(r, dist, out):
    from marionette import Marionette

    profile = Path(os.environ["RUNNER_TEMP"] if "RUNNER_TEMP" in os.environ else
                   os.environ["TEMP"]) / "hallow-ui-profile"
    profile.mkdir(exist_ok=True)
    proc = subprocess.Popen([EXE, "-no-remote", "-profile", profile, "--marionette",
                             "--remote-allow-system-access"])
    m = Marionette(timeout=180)
    try:
        m.chrome()
        info = m.run(
            """
            const win = Services.wm.getMostRecentWindow("navigator:browser");
            win.focus();
            await new Promise(r => setTimeout(r, 3000));
            const root = win.document.documentElement;
            return {
              title: win.document.title,
              uiFont: win.getComputedStyle(root).fontFamily,
              // A bundled font is available when text set in it is not
              // set in the fallback.
              fonts: Object.fromEntries(["IBM Plex Sans", "Space Grotesk"].map(f => {
                const ctx = win.document.createElementNS(
                  "http://www.w3.org/1999/xhtml", "canvas").getContext("2d");
                const width = font => {
                  ctx.font = font;
                  return ctx.measureText("Hallow browser 0123456789").width;
                };
                return [f, width(`20px '${f}', monospace`) != width("20px monospace")];
              })),
              version: AppConstants.MOZ_APP_VERSION_DISPLAY,
            };
            """
        )
        r.expect("the bundled fonts are available to Hallow",
                 all(info["fonts"].values()), info["fonts"])
        r.expect("the interface font is IBM Plex Sans", "IBM Plex Sans" in info["uiFont"], info)
        screenshot(out, "home")
        m.run("Services.wm.getMostRecentWindow('navigator:browser').openAboutDialog();"
              "await new Promise(r => setTimeout(r, 4000));")
        about = m.run(
            """
            const win = Services.wm.getMostRecentWindow("Browser:About");
            return { version: win.document.getElementById("version").textContent };
            """
        )
        r.expect("About Hallow shows the Hallow version",
                 json.loads(next(dist.glob("hallow-*-win64.json")).read_text())["version"]
                 in about["version"], about)
        screenshot(out, "about")
        m.run(
            """
            Services.wm.getMostRecentWindow("Browser:About").close();
            const win = Services.wm.getMostRecentWindow("navigator:browser");
            win.openPreferences("paneGeneral");
            await new Promise(r => setTimeout(r, 4000));
            """
        )
        screenshot(out, "settings")
        # The desktop with Hallow's shortcut, and the taskbar with Hallow
        # running in it.
        run(["powershell", "-NoProfile", "-Command",
             "(New-Object -ComObject Shell.Application).MinimizeAll()"])
        time.sleep(3)
        screenshot(out, "desktop-and-taskbar")
    finally:
        m.quit()
        try:
            proc.wait(timeout=30)
        except subprocess.TimeoutExpired:
            proc.kill()
        subprocess.run(["taskkill", "/F", "/T", "/IM", "hallow.exe"], capture_output=True)


def stage_updates(r, dist, out):
    cmd = [sys.executable, ROOT / "tools/test_windows_updates.py", "--install", INSTALL,
           "--packages", dist / "update-tests", "--out", out / "updates"]
    signed = list((dist / "signed").glob("hallow-*-win64.complete.mar"))
    if signed:
        # Release: the build's own package, signed with the release key.
        cmd += ["--next-mar", signed[0]]
    proc = run(cmd, timeout=2400, capture_output=True)
    print(proc.stdout[-30000:])
    print(proc.stderr[-5000:])
    for line in proc.stdout.splitlines():
        if line.startswith(("PASS ", "FAIL ")):
            status, _, rest = line.partition(" ")
            name, _, detail = rest.partition(": ")
            r.expect(name, status == "PASS", detail[:2000])
    r.expect("update tests ran to the end", proc.returncode == 0
             or "update test(s) failed" in proc.stdout, proc.returncode)


def stage_uninstall(r, dist, out):
    subprocess.run(["taskkill", "/F", "/T", "/IM", "hallow.exe"], capture_output=True)
    helper = INSTALL / "uninstall" / "helper.exe"
    proc = run([helper, "/S"], timeout=300)
    # The uninstaller continues from a copy of itself in %TEMP%.
    deadline = time.time() + 180
    while EXE.exists() and time.time() < deadline:
        time.sleep(2)
    r.expect("silent uninstall succeeds", proc.returncode == 0 and not EXE.exists(),
             proc.returncode)
    uninstall = r"Software\Microsoft\Windows\CurrentVersion\Uninstall"
    keys = [k for k in reg_subkeys(winreg.HKEY_CURRENT_USER, uninstall) if "Hallow" in k]
    r.expect("Settings > Apps entry removed", not keys, keys)
    desktop = Path(os.environ["USERPROFILE"]) / "Desktop" / "Hallow.lnk"
    start = Path(os.environ["APPDATA"]) / "Microsoft/Windows/Start Menu/Programs/Hallow.lnk"
    r.expect("shortcuts removed", not desktop.exists() and not start.exists())
    left = [str(p.relative_to(INSTALL)) for p in INSTALL.rglob("*")] if INSTALL.exists() else []
    r.expect("program files removed", not any(p.endswith(".exe") or p.endswith(".dll")
                                              for p in left), left[:20])


STAGES = {
    "install": stage_install,
    "contents": stage_contents,
    "registration": stage_registration,
    "icons": stage_icons,
    "browser": stage_browser,
    "ui": stage_ui,
    "updates": stage_updates,
    "uninstall": stage_uninstall,
}


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--dist", required=True, type=Path)
    parser.add_argument("--out", required=True, type=Path)
    parser.add_argument("--stages", default=",".join(STAGES))
    args = parser.parse_args()
    dist, out = args.dist.resolve(), args.out.resolve()
    out.mkdir(parents=True, exist_ok=True)
    r = Results(out)
    for name in args.stages.split(","):
        r.stage(name)
        try:
            fn = STAGES[name]
            if fn.__code__.co_argcount == 2:
                fn(r, dist)
            else:
                fn(r, dist, out)
        except Exception:
            r.current["error"] = traceback.format_exc()
            print(r.current["error"], flush=True)
        r.end()
    ok = r.write_summary()
    sys.exit(0 if ok else 1)


if __name__ == "__main__":
    main()
