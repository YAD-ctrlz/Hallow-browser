# Install and test a Hallow Windows build on a Windows runner (CI): the
# Windows counterpart of tools/test-package.sh.
#
#   tools/test-windows.ps1 -Dist dist -Results results
#
# Dist holds the Windows build job's artifacts (installer, zip, MAR, build
# info, update-tests/). The stages and checks are in tools/test_windows.py;
# every stage runs and results/summary.md lists them. Screenshots are printed
# as base64 JPEGs in collapsed log groups so they can be looked at from the
# job log alone.
param(
  [string]$Dist = "dist",
  [string]$Results = "results",
  # A list: PowerShell passes `-Stages install,updates` as two values (as a
  # plain string they would arrive as "install updates").
  [string[]]$Stages = @()
)
$ErrorActionPreference = "Continue"
New-Item -ItemType Directory -Force $Results | Out-Null

python -m pip install --quiet --disable-pip-version-check pillow pefile
if (-not (Get-Command ffmpeg -ErrorAction SilentlyContinue)) {
  # Test clips in AV1, VP9 and H.264 (tools/test_browser.py).
  choco install ffmpeg-full -y --no-progress | Out-Null
  $env:Path += ";C:\ProgramData\chocolatey\bin"
}

$arguments = @("tools/test_windows.py", "--dist", $Dist, "--out", $Results)
if ($Stages) { $arguments += @("--stages", ($Stages -join ",")) }
python @arguments
$status = $LASTEXITCODE

$printScreenshots = @'
import base64, io, sys
from pathlib import Path
from PIL import Image
for png in sorted(Path(sys.argv[1]).rglob("*.png")):
    image = Image.open(png).convert("RGB")
    if image.width > 1100:
        image = image.resize((1100, round(image.height * 1100 / image.width)))
    buf = io.BytesIO()
    image.save(buf, "JPEG", quality=60)
    print(f"::group::{png.relative_to(sys.argv[1])} (base64 jpeg)")
    print(base64.b64encode(buf.getvalue()).decode())
    print("::endgroup::")
'@
$printScreenshots | python - $Results

Get-Content (Join-Path $Results "summary.md")
exit $status
