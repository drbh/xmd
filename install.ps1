# Installs the wtf binary (the command line and the language server) from the
# GitHub release for Windows. No Rust toolchain needed.
#
#   irm https://github.com/drbh/jot/releases/latest/download/install.ps1 | iex
#
#   $env:WTF_VERSION = "v0.1.0"     pin a release instead of the latest one
#   $env:WTF_INSTALL_DIR = "DIR"    install somewhere other than %LOCALAPPDATA%\wtf\bin
#   $env:WTF_ASSET_URL = "URL"      fetch the archive and checksums.txt from this
#                                   directory URL instead of GitHub (for testing)
$ErrorActionPreference = "Stop"

$repo = "drbh/jot"
$installDir = if ($env:WTF_INSTALL_DIR) { $env:WTF_INSTALL_DIR } else { Join-Path $env:LOCALAPPDATA "wtf\bin" }

$arch = if ($env:PROCESSOR_ARCHITEW6432) { $env:PROCESSOR_ARCHITEW6432 } else { $env:PROCESSOR_ARCHITECTURE }
if ($arch -ne "AMD64") {
    throw "wtf: no prebuilt binary for Windows $arch; build one with: cargo install --git https://github.com/$repo wtf"
}
$target = "x86_64-pc-windows-msvc"
$asset = "wtf-$target.zip"

$base = if ($env:WTF_ASSET_URL) {
    $env:WTF_ASSET_URL.TrimEnd("/")
} elseif ($env:WTF_VERSION) {
    "https://github.com/$repo/releases/download/$($env:WTF_VERSION)"
} else {
    "https://github.com/$repo/releases/latest/download"
}

$tmp = Join-Path ([System.IO.Path]::GetTempPath()) ("wtf-install-" + [System.IO.Path]::GetRandomFileName())
New-Item -ItemType Directory -Path $tmp | Out-Null
try {
    Write-Host "downloading $base/$asset"
    Invoke-WebRequest -UseBasicParsing -Uri "$base/$asset" -OutFile (Join-Path $tmp $asset)
    Invoke-WebRequest -UseBasicParsing -Uri "$base/checksums.txt" -OutFile (Join-Path $tmp "checksums.txt")

    $line = Get-Content (Join-Path $tmp "checksums.txt") | Where-Object { $_ -match "^([0-9a-fA-F]{64})\s+\*?$([regex]::Escape($asset))$" } | Select-Object -First 1
    if (-not $line) { throw "wtf: $asset is not listed in checksums.txt" }
    $expected = ($line -split "\s+")[0].ToLowerInvariant()
    $actual = (Get-FileHash -Algorithm SHA256 (Join-Path $tmp $asset)).Hash.ToLowerInvariant()
    if ($actual -ne $expected) { throw "wtf: checksum mismatch for ${asset}: expected $expected, got $actual" }

    Expand-Archive -Path (Join-Path $tmp $asset) -DestinationPath (Join-Path $tmp "unpacked") -Force
    New-Item -ItemType Directory -Path $installDir -Force | Out-Null
    Move-Item -Path (Join-Path $tmp "unpacked\wtf.exe") -Destination (Join-Path $installDir "wtf.exe") -Force
} finally {
    Remove-Item -Recurse -Force $tmp -ErrorAction SilentlyContinue
}

$exe = Join-Path $installDir "wtf.exe"
Write-Host "installed $(& $exe --version) to $exe"
$onPath = ($env:Path -split ";") | Where-Object { $_.TrimEnd("\") -ieq $installDir.TrimEnd("\") }
if (-not $onPath) {
    Write-Host ""
    Write-Host "$installDir is not on your PATH. Add it for your user (then open a new terminal):"
    Write-Host "  [Environment]::SetEnvironmentVariable('Path', [Environment]::GetEnvironmentVariable('Path', 'User') + ';$installDir', 'User')"
}
