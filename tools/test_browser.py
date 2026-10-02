#!/usr/bin/env python3
"""Checks of an installed Hallow, run by CI under Xvfb.

- Startup: timings, and that no Hallow module runs at startup (the update
  check is loaded by Firefox's update timer, long after the first window).
- Graphics and video: the compositor in use and the hardware video decoding
  decisions Firefox's own driver checks made (no forcing by Hallow).
- Codec preference (patches/0006): what Media Source Extensions and
  MediaCapabilities report for AV1, VP9 and H.264 given those decisions.
- Playback of AV1, VP9 and H.264 test clips (software decoding when the
  machine has no usable GPU, like CI).
- Optionally YouTube: plays a video, records the codec YouTube picked and
  navigates to a second one. Network or bot checks can block this from CI,
  so it is reported but does not fail the run.

    tools/test_browser.py --out results/browser [--media DIR] [--youtube]
"""

import argparse
import json
import subprocess
import sys
import tempfile
import time
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
from marionette import Marionette  # noqa: E402

PORT = 8766
CLIPS = {
    # name: (file, MIME type for canPlayType/MediaCapabilities)
    "av1": ("av1.mp4", 'video/mp4; codecs="av01.0.05M.08"'),
    "vp9": ("vp9.webm", 'video/webm; codecs="vp09.00.10.08"'),
    "h264": ("h264.mp4", 'video/mp4; codecs="avc1.42E01E"'),
}
YOUTUBE = [
    "https://www.youtube.com/watch?v=aqz-KE-bpKQ",  # Big Buck Bunny, up to 4K
    "https://www.youtube.com/watch?v=jNQXAC9IVRw",
]


def screenshot(out, name):
    subprocess.run(["import", "-window", "root", str(out / f"{name}.png")], check=False)


def make_clips(media):
    """Short test clips in each codec, made with FFmpeg."""
    media.mkdir(parents=True, exist_ok=True)
    encoders = {
        "av1.mp4": ["-c:v", "libaom-av1", "-cpu-used", "8", "-row-mt", "1"],
        "vp9.webm": ["-c:v", "libvpx-vp9", "-deadline", "realtime"],
        "h264.mp4": ["-c:v", "libx264", "-preset", "ultrafast", "-pix_fmt", "yuv420p"],
    }
    for name, args in encoders.items():
        if not (media / name).exists():
            subprocess.run(
                ["ffmpeg", "-loglevel", "error", "-y", "-f", "lavfi", "-i",
                 "testsrc2=size=640x360:rate=30", "-t", "6", *args, str(media / name)],
                check=True,
            )
    (media / "index.html").write_text("<!doctype html><title>clips</title>\n")


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--out", required=True, type=Path)
    parser.add_argument("--media", type=Path, help="test clips (made if missing)")
    parser.add_argument("--youtube", action="store_true")
    args = parser.parse_args()
    out = args.out.resolve()
    out.mkdir(parents=True, exist_ok=True)
    media = (args.media or out / "media").resolve()
    make_clips(media)
    server = subprocess.Popen(
        [sys.executable, "-m", "http.server", str(PORT), "--bind", "127.0.0.1",
         "--directory", str(media)],
        stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL,
    )

    profile = tempfile.mkdtemp()
    browser = subprocess.Popen(
        ["/usr/lib/hallow/hallow", "--new-instance", "--profile", profile,
         "--marionette", "--remote-allow-system-access", "about:blank"],
        stdout=subprocess.DEVNULL, stderr=subprocess.STDOUT,
    )
    report = {}
    failures = []

    def expect(name, condition, detail):
        print(f"{'PASS' if condition else 'FAIL'} {name}: {detail}", flush=True)
        if not condition:
            failures.append(name)

    m = Marionette()
    try:
        m.chrome()
        # --- Startup ---------------------------------------------------------
        report["startup"] = m.run(
            """
            if (!Services.startup.getStartupInfo().sessionRestored) {
              await new Promise(r =>
                Services.obs.addObserver(r, "sessionstore-windows-restored"));
            }
            // Let idle-time startup tasks run too.
            await new Promise(r => setTimeout(r, 10000));
            const info = Services.startup.getStartupInfo();
            const since = t => t ? t - info.process : null;
            return {
              firstPaintMs: since(info.firstPaint2 || info.firstPaint),
              sessionRestoredMs: since(info.sessionRestored),
              hallowModulesLoaded: [
                "resource://gre/modules/LinuxPackageUpdater.sys.mjs",
              ].filter(url => Cu.isESModuleLoaded(url)),
              version: AppConstants.MOZ_APP_VERSION_DISPLAY,
              geckoVersion: Services.appinfo.version,
              packagedApp: Services.sysinfo.getProperty("isPackagedApp"),
            };
            """
        )
        s = report["startup"]
        expect(
            "no Hallow module runs at startup",
            s["hallowModulesLoaded"] == [],
            s,
        )

        # --- Graphics and video decoding ---------------------------------------
        report["graphics"] = m.run(
            """
            const { Troubleshoot } = ChromeUtils.importESModule(
              "resource://gre/modules/Troubleshoot.sys.mjs");
            const snapshot = await Troubleshoot.snapshot();
            const g = snapshot.graphics;
            const features = (g.featureLog?.features ?? []).map(f => ({
              name: f.name, status: f.status,
              log: (f.log ?? []).map(l => `${l.type}: ${l.status} ${l.message ?? ""}`.trim()),
            }));
            return {
              compositor: g.windowLayerManagerType,
              features: features.filter(f =>
                /WEBRENDER|COMPOSIT|DECOD|VIDEO|VAAPI/.test(f.name)),
              codecSupportInfo: snapshot.media?.codecSupportInfo,
            };
            """
        )
        print("compositor:", report["graphics"]["compositor"])
        for f in report["graphics"]["features"]:
            print(f"  {f['name']}: {f['status']}  {'; '.join(f['log'])}")
        print("codec support:", report["graphics"]["codecSupportInfo"])

        # --- Codec preference and playback ---------------------------------------
        m.navigate(f"http://127.0.0.1:{PORT}/index.html")
        report["codecs"] = m.run(
            """
            const query = (type, contentType) =>
              navigator.mediaCapabilities.decodingInfo({
                type,
                video: { contentType, width: 1920, height: 1080,
                         bitrate: 4000000, framerate: 30 },
              }).then(i => ({ supported: i.supported, smooth: i.smooth,
                              powerEfficient: i.powerEfficient }));
            const result = {};
            for (const [name, [file, type]] of Object.entries(args[0])) {
              result[name] = {
                mse: MediaSource.isTypeSupported(type),
                canPlayType: document.createElement("video").canPlayType(type),
                // powerEfficient at 1080p means hardware decoding.
                file: await query("file", type),
                mediaSource: await query("media-source", type),
              };
            }
            return result;
            """,
            [CLIPS],
        )
        for name, value in report["codecs"].items():
            print(f"{name}: {value}")
        # On a machine without hardware decoding (CI), AV1 must be withheld
        # from MSE but still play as a file; VP9 and H.264 stay offered.
        av1 = report["codecs"]["av1"]
        expect(
            "AV1 is offered to streaming sites only with hardware decoding",
            av1["mse"] == av1["file"]["powerEfficient"]
            and av1["mediaSource"]["supported"] == av1["mse"],
            av1,
        )
        expect("AV1 files still play", av1["file"]["supported"], av1)
        expect("VP9 is offered", report["codecs"]["vp9"]["mse"], report["codecs"]["vp9"])
        expect("H.264 is offered", report["codecs"]["h264"]["mse"], report["codecs"]["h264"])

        report["playback"] = {}
        for name, (file, _type) in CLIPS.items():
            m.navigate(f"http://127.0.0.1:{PORT}/{file}")
            result = m.run(
                """
                const video = document.querySelector("video");
                video.muted = true;
                await video.play().catch(() => {});
                const deadline = Date.now() + 15000;
                while (video.currentTime < 2 && !video.error && Date.now() < deadline) {
                  await new Promise(r => setTimeout(r, 200));
                }
                const q = video.getVideoPlaybackQuality();
                return {
                  currentTime: video.currentTime,
                  error: video.error && video.error.message,
                  frames: q.totalVideoFrames, dropped: q.droppedVideoFrames,
                };
                """
            )
            report["playback"][name] = result
            expect(f"{name} clip plays", result["currentTime"] >= 2 and not result["error"], result)
        screenshot(out, "playback")

        # --- YouTube ------------------------------------------------------------
        if args.youtube:
            report["youtube"] = []
            for url in YOUTUBE:
                try:
                    m.navigate(url)
                    result = m.run(
                        """
                        const deadline = Date.now() + 60000;
                        let video;
                        while (Date.now() < deadline) {
                          video = document.querySelector("video.html5-main-video, video");
                          if (video && video.currentTime > 3) break;
                          // Start playback if autoplay is blocked.
                          if (video && video.paused) {
                            video.muted = true;
                            video.play().catch(() => {});
                          }
                          await new Promise(r => setTimeout(r, 500));
                        }
                        const player = document.getElementById("movie_player");
                        let stats = null;
                        try { stats = player?.getStatsForNerds?.(); } catch (e) {}
                        const q = video?.getVideoPlaybackQuality();
                        const text = document.body?.innerText ?? "";
                        return {
                          title: document.title,
                          // YouTube asks datacenter IPs like CI's to sign in.
                          botCheck: /confirm you.re not a bot/i.test(text),
                          playing: !!video && video.currentTime > 3,
                          currentTime: video?.currentTime ?? null,
                          codecs: stats?.codecs ?? null,
                          resolution: stats?.resolution ?? null,
                          frames: q?.totalVideoFrames ?? null,
                          dropped: q?.droppedVideoFrames ?? null,
                        };
                        """
                    )
                except RuntimeError as e:
                    result = {"error": str(e)}
                result["url"] = url
                report["youtube"].append(result)
                status = (
                    "plays" if result.get("playing")
                    else "blocked by YouTube's bot check (CI IP)" if result.get("botCheck")
                    else "did not play"
                )
                print(f"INFO YouTube {url}: {status}: {result}", flush=True)
                screenshot(out, f"youtube-{len(report['youtube'])}")
    finally:
        (out / "report.json").write_text(json.dumps(report, indent=2))
        m.quit()
        try:
            browser.wait(timeout=30)
        except subprocess.TimeoutExpired:
            browser.kill()
        server.terminate()

    if failures:
        print(f"{len(failures)} browser check(s) failed: {', '.join(failures)}")
        sys.exit(1)
    print("all browser checks passed")


if __name__ == "__main__":
    main()
