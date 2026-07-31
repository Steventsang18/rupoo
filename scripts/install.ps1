# Rupoo - Windows installer (PowerShell 5.1+)
#
# Downloads the official release binary from GitHub Releases,
# verifies its SHA-256 checksum, and installs it to $HOME\bin.
#
# Usage:
#   powershell -ExecutionPolicy Bypass -c "irm https://raw.githubusercontent.com/Steventsang18/rupoo/master/scripts/install.ps1 | iex"
#   .\scripts\install.ps1               # latest version
#   .\scripts\install.ps1 -Version 0.6.3   # pinned version
#   .\scripts\install.ps1 -Dir C:\Tools   # custom install dir

[CmdletBinding()]
param(
    [string]$Version,
    [string]$Dir = "$HOME\bin"
)

$ErrorActionPreference = "Stop"
$Repo = "Steventsang18/rupoo"

# --- Detect architecture (only x86_64 is published for Windows) ---
switch ($env:PROCESSOR_ARCHITECTURE) {
    "AMD64" { $Target = "x86_64-pc-windows-msvc" }
    default { Write-Error "Unsupported architecture: $env:PROCESSOR_ARCHITECTURE (only x86_64 builds are published)"; exit 1 }
}

# --- Resolve version ---
if (-not $Version) {
    Write-Host "> Resolving latest release version..."
    $Release = Invoke-RestMethod -Uri "https://api.github.com/repos/$Repo/releases/latest" -Headers @{ "User-Agent" = "rupoo-installer" }
    $Version = $Release.tag_name.TrimStart("v")
}
Write-Host "> Installing rupoo v$Version ($Target)"

$Archive = "rupoo-v$Version-$Target.zip"
$BaseUrl = "https://github.com/$Repo/releases/download/v$Version"
$TmpDir = Join-Path $env:TEMP "rupoo-install"
if (Test-Path $TmpDir) { Remove-Item -Recurse -Force $TmpDir }
New-Item -ItemType Directory -Path $TmpDir | Out-Null

# --- Download binary + checksum ---
Write-Host "> Downloading $Archive ..."
$ZipPath = Join-Path $TmpDir "rupoo.zip"
Invoke-WebRequest -Uri "$BaseUrl/$Archive" -OutFile $ZipPath -UseBasicParsing
$ShaPath = Join-Path $TmpDir "rupoo.sha256"
try {
    Invoke-WebRequest -Uri "$BaseUrl/$Archive.sha256" -OutFile $ShaPath -UseBasicParsing
} catch {
    Write-Warning "Checksum file not found; skipping verification."
}

# --- Verify SHA-256 ---
if (Test-Path $ShaPath) {
    Write-Host "> Verifying SHA-256 checksum..."
    $Expected = (Get-Content $ShaPath).Split(" ")[0].Trim()
    $Actual = (Get-FileHash -Path $ZipPath -Algorithm SHA256).Hash.ToLower()
    if ($Actual -ne $Expected) {
        Write-Error "Checksum mismatch!`n  expected: $Expected`n  actual:   $Actual"
        exit 1
    }
    Write-Host "> Checksum OK."
}

# --- Extract + install ---
Write-Host "> Extracting..."
New-Item -ItemType Directory -Path $Dir -Force | Out-Null
Expand-Archive -Path $ZipPath -DestinationPath $TmpDir -Force
Copy-Item (Join-Path $TmpDir "rupoo.exe") (Join-Path $Dir "rupoo.exe") -Force

# --- Verify + PATH hint ---
$Installed = Join-Path $Dir "rupoo.exe"
if (Test-Path $Installed) {
    Write-Host ""
    Write-Host "OK rupoo v$Version installed to $Installed"
    & $Installed --version
} else {
    Write-Error "Installation failed."
    exit 1
}

$PathUser = [Environment]::GetEnvironmentVariable("Path", "User")
if ($PathUser -notlike "*$Dir*") {
    Write-Host ""
    Write-Host "Add $Dir to your user PATH:"
    Write-Host "  [Environment]::SetEnvironmentVariable('Path', \"$Dir;`$env:Path\", 'User')"
    Write-Host "  (then open a new terminal)"
}

Write-Host ""
Write-Host "Run 'rupoo' to start the interactive REPL."
