#!/usr/bin/env python3
"""PGO training run of an instrumented Windows build, on Windows.

Firefox's profile server (build/pgo/profileserver.py) normally runs under
mach in a configured source tree. Hallow cross-compiles Windows builds on
Linux, so the training run happens on a Windows runner with only what it
needs: the kit made by tools/pgo-kit.sh and tools/pgo/ standing in for the
parts of mach the server imports. The raw profiles it writes are merged on
Linux (`cargo hb pgo-merge`) for the optimized build.

    python tools/pgo_windows.py --kit kit.tar.gz --build hallow.win64.zip --out DIR
"""

import argparse
import os
import subprocess
import sys
import tarfile
import zipfile
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--kit", required=True, type=Path)
    parser.add_argument("--build", required=True, type=Path, help="instrumented build (.zip)")
    parser.add_argument("--out", required=True, type=Path, help="where the .profraw files go")
    parser.add_argument("--work", type=Path, default=Path(os.environ.get("RUNNER_TEMP", ".")) / "pgo-work")
    args = parser.parse_args()

    kit = args.work / "kit"
    build = args.work / "build"
    kit.mkdir(parents=True, exist_ok=True)
    with tarfile.open(args.kit) as tar:
        tar.extractall(kit, filter="data")
    with zipfile.ZipFile(args.build) as z:
        z.extractall(build)
    binary = build / "hallow" / "hallow.exe"
    if not binary.exists():
        sys.exit(f"{binary} not found in {args.build}")
    if os.name != "nt":
        binary.chmod(0o755)  # trying the runner out elsewhere

    paths = [ROOT / "tools" / "pgo"]
    paths += sorted(p for p in (kit / "testing" / "mozbase").iterdir() if p.is_dir())
    paths += [kit / "python" / "mozterm", kit / "third_party" / "python" / "redo",
              kit / "third_party" / "python" / "distro"]
    env = dict(os.environ)
    env["PYTHONPATH"] = os.pathsep.join(str(p) for p in paths)
    env["HALLOW_PGO_TOPSRCDIR"] = str(kit)
    env.pop("JARLOG_FILE", None)

    out = args.out.resolve()
    out.mkdir(parents=True, exist_ok=True)
    # The server writes the raw profiles into its working directory.
    cmd = [sys.executable, str(kit / "build" / "pgo" / "profileserver.py"),
           "--binary", str(binary)]
    print("+", " ".join(cmd), flush=True)
    result = subprocess.run(cmd, cwd=out, env=env)
    profiles = sorted(out.glob("*.profraw"))
    total = sum(p.stat().st_size for p in profiles)
    print(f"{len(profiles)} raw profiles, {total / 1e6:.1f} MB")
    if result.returncode != 0:
        sys.exit(f"the training run failed ({result.returncode})")
    if not profiles or total == 0:
        sys.exit("the training run wrote no profiles")


if __name__ == "__main__":
    main()
