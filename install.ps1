# Sootie Windows Installer (PowerShell)
# Usage: iwr -useb https://raw.githubusercontent.com/joe223/sootie/main/install.ps1 | iex

$ErrorActionPreference = "Stop"

$Repo = "joe223/sootie"
$BinaryName = "sootie.exe"
$InstallDir = if ($env:INSTALL_DIR) { $env:INSTALL_DIR } else { "$env:LOCALAPPDATA\Microsoft\WindowsApps" }

function Write-Info($msg) {
    Write-Host "[INFO] $msg" -ForegroundColor Blue
}

function Write-Success($msg) {
    Write-Host "[SUCCESS] $msg" -ForegroundColor Green
}

function Write-Warn($msg) {
    Write-Host "[WARN] $msg" -ForegroundColor Yellow
}

function Write-Error($msg) {
    Write-Host "[ERROR] $msg" -ForegroundColor Red
}

function Get-Platform {
    if ([Environment]::Is64BitOperatingSystem) {
        return "windows-x64"
    } else {
        throw "Only 64-bit Windows is supported"
    }
}

function Get-DownloadUrl($platform) {
    $version = if ($env:SOOTIE_VERSION) { $env:SOOTIE_VERSION } else { "latest" }

    if ($version -eq "latest") {
        return "https://github.com/$Repo/releases/latest/download/sootie-${platform}.exe"
    } else {
        return "https://github.com/$Repo/releases/download/$version/sootie-${platform}.exe"
    }
}

function Download-Binary($platform) {
    $url = Get-DownloadUrl $platform
    $tempFile = [System.IO.Path]::GetTempFileName() + ".exe"

    Write-Info "Downloading Sootie for $platform..."
    Write-Info "URL: $url"

    try {
        $progressPreference = 'silentlyContinue'
        Invoke-WebRequest -Uri $url -OutFile $tempFile -UseBasicParsing
        $progressPreference = 'Continue'
    } catch {
        Write-Error "Failed to download from $url"
        Write-Error $_.Exception.Message
        exit 1
    }

    return $tempFile
}

function Install-Binary($binaryPath) {
    $installPath = Join-Path $InstallDir $BinaryName

    Write-Info "Installing to $installPath..."

    # Create install directory if it doesn't exist
    if (!(Test-Path $InstallDir)) {
        New-Item -ItemType Directory -Path $InstallDir -Force | Out-Null
    }

    # Remove existing binary if present
    if (Test-Path $installPath) {
        Write-Warn "Removing existing installation..."
        Remove-Item $installPath -Force
    }

    Move-Item $binaryPath $installPath -Force

    Write-Success "Sootie installed successfully!"

    # Check if install dir is in PATH
    $pathDirs = $env:PATH -split ";"
    $inPath = $pathDirs | Where-Object { $_ -ieq $InstallDir }

    if (-not $inPath) {
        Write-Warn "Install directory is not in PATH"
        Write-Info "Add the following to your PATH: $InstallDir"
        Write-Info "Or run Sootie using the full path: $installPath"
    }
}

function Verify-Installation {
    $installPath = Join-Path $InstallDir $BinaryName

    if (Test-Path $installPath) {
        Write-Success "Sootie is installed at: $installPath"

        try {
            $version = & $installPath --version 2>$null
            Write-Info "Version: $version"
        } catch {
            # Ignore version check errors
        }
    } else {
        Write-Error "Installation verification failed"
        exit 1
    }
}

function Show-PostInstallInstructions {
    Write-Host ""
    Write-Success "Installation complete!"
    Write-Host ""
    Write-Host "Next steps:"
    Write-Host "  1. Run: sootie setup"
    Write-Host "  2. Configure your MCP client (Claude Code, Cursor, etc.)"
    Write-Host ""
    Write-Host "Documentation: https://github.com/$Repo#readme"
}

# Main
Write-Info "Sootie Windows Installer"
Write-Info "======================="
Write-Host ""

$platform = Get-Platform
Write-Info "Detected platform: $platform"

$binaryPath = Download-Binary $platform
Install-Binary $binaryPath

Verify-Installation
Show-PostInstallInstructions
