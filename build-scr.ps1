# Build the Windows screen saver (.scr) variant of Tank.
#
# Usage (PowerShell):
#   .\build-scr.ps1            # debug build + copy to ./tank.scr
#   .\build-scr.ps1 -Release   # release build + copy to ./tank.scr
#
# The screen saver is just the `saver` binary renamed to `.scr`. After the build
# copy tank.scr into %SystemRoot%\System32 (e.g. C:\Windows\System32), then pick
# "Tank Matrix Saver" from the Screen Saver Settings dialog. Settings can be
# changed with `tank.scr /c`.

param(
    [switch]$Release
)

$profile = if ($Release) { "release" } else { "debug" }

Write-Host "Building tank screen saver ($profile)..."
cargo build --bin saver $(if ($Release) { "--release" } else { "" })

if ($LASTEXITCODE -ne 0) {
    Write-Error "cargo build failed"
    exit $LASTEXITCODE
}

$exe = "target/$profile/saver.exe"
if (-not (Test-Path $exe)) {
    Write-Error "built binary not found at $exe"
    exit 1
}

Copy-Item $exe -Destination "tank.scr" -Force
Write-Host "Created tank.scr in the project root."
Write-Host "Install: copy tank.scr to $env:SystemRoot\System32"
