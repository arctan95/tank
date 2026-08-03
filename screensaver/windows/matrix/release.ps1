# Build and package Matrix.scr.

$ErrorActionPreference = "Stop"

$ProjectRoot = Resolve-Path "$PSScriptRoot\..\..\.."
$Dist = Join-Path $PSScriptRoot "dist"
$Executable = Join-Path $ProjectRoot "target\release\matrix-saver.exe"
$ScreenSaver = Join-Path $Dist "Matrix.scr"
$Archive = Join-Path $Dist "Matrix.scr.zip"

Push-Location $ProjectRoot
try {
    cargo build --release --bin matrix-saver
} finally {
    Pop-Location
}

New-Item -ItemType Directory -Force $Dist | Out-Null
Copy-Item -Force $Executable $ScreenSaver
Remove-Item -Force -ErrorAction SilentlyContinue $Archive
Compress-Archive -Path $ScreenSaver -DestinationPath $Archive

Write-Host "Done: $Archive"
