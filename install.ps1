param(
    [string]$Version = $(if ($env:AW_VERSION) { $env:AW_VERSION } else { "latest" }),
    [string]$InstallDir = $env:AW_INSTALL_DIR
)

$ErrorActionPreference = "Stop"

if (-not $InstallDir) {
    if (-not $HOME) {
        throw "HOME is not set; pass -InstallDir or set AW_INSTALL_DIR."
    }
    $InstallDir = Join-Path $HOME ".local\bin"
}

$Repo = if ($env:AW_REPO) { $env:AW_REPO } else { "abird-ai/aw" }
$BaseUrl = $env:AW_RELEASE_BASE_URL

$OsArchitecture = [System.Runtime.InteropServices.RuntimeInformation]::OSArchitecture
switch ($OsArchitecture) {
    ([System.Runtime.InteropServices.Architecture]::X64) {
        $Asset = "aw-windows-x86_64.exe"
    }
    ([System.Runtime.InteropServices.Architecture]::Arm64) {
        $Asset = "aw-windows-aarch64.exe"
    }
    default {
        throw "Unsupported Windows architecture: $OsArchitecture. Published: x64, arm64."
    }
}

if (-not $BaseUrl) {
    if ($Version -eq "latest") {
        $BaseUrl = "https://github.com/$Repo/releases/latest/download"
    } else {
        $Tag = if ($Version.StartsWith("v")) { $Version } else { "v$Version" }
        $BaseUrl = "https://github.com/$Repo/releases/download/$Tag"
    }
}

$WorkDir = Join-Path ([System.IO.Path]::GetTempPath()) ("aw-install-" + [guid]::NewGuid().ToString("N"))
New-Item -ItemType Directory -Path $WorkDir | Out-Null

try {
    $BinaryPath = Join-Path $WorkDir $Asset
    $ChecksumPath = "$BinaryPath.sha256"
    $VersionPath = Join-Path $WorkDir "VERSION"

    Write-Host "Downloading aw..."
    Invoke-WebRequest -UseBasicParsing -Uri "$($BaseUrl.TrimEnd('/'))/$Asset" -OutFile $BinaryPath
    Invoke-WebRequest -UseBasicParsing -Uri "$($BaseUrl.TrimEnd('/'))/$Asset.sha256" -OutFile $ChecksumPath
    Invoke-WebRequest -UseBasicParsing -Uri "$($BaseUrl.TrimEnd('/'))/VERSION" -OutFile $VersionPath

    $ChecksumLines = @(Get-Content $ChecksumPath)
    if ($ChecksumLines.Count -ne 1) { throw "Invalid SHA-256 sidecar." }
    $Parts = $ChecksumLines[0].Trim() -split "\s+"
    if ($Parts.Count -ne 2) { throw "Invalid SHA-256 sidecar." }
    $Expected = $Parts[0].ToLowerInvariant()
    $NamedAsset = $Parts[1].TrimStart('*')
    if ($Expected -notmatch '^[0-9a-f]{64}$') {
        throw "Invalid SHA-256 sidecar."
    }
    if ($NamedAsset -ne $Asset) { throw "Checksum sidecar names an unexpected asset." }

    $Actual = (Get-FileHash -Algorithm SHA256 $BinaryPath).Hash.ToLowerInvariant()
    if ($Expected -ne $Actual) {
        throw "SHA-256 verification failed."
    }

    $DownloadedVersion = (Get-Content $VersionPath -Raw).Trim()
    if ($DownloadedVersion -notmatch '^[0-9A-Za-z][0-9A-Za-z.+-]{0,63}$') {
        throw "Invalid release VERSION metadata."
    }
    if ($Version -ne "latest") {
        $RequestedVersion = if ($Version.StartsWith("v")) { $Version.Substring(1) } else { $Version }
        if ($DownloadedVersion -ne $RequestedVersion) {
            throw "Downloaded aw version $DownloadedVersion does not match requested version $RequestedVersion."
        }
    }

    New-Item -ItemType Directory -Force -Path $InstallDir | Out-Null
    $Destination = Join-Path $InstallDir "aw.exe"
    $HadExisting = Test-Path -LiteralPath $Destination
    $InstalledHash = if ($HadExisting) {
        (Get-FileHash -Algorithm SHA256 $Destination).Hash.ToLowerInvariant()
    } else { $null }

    if ($InstalledHash -eq $Expected) {
        Write-Host "aw $DownloadedVersion is already up to date at $Destination"
    } else {
        $DestinationTemp = Join-Path $InstallDir (".aw.exe.tmp." + [guid]::NewGuid().ToString("N"))
        $DestinationBackup = Join-Path $InstallDir (".aw.exe.bak." + [guid]::NewGuid().ToString("N"))
        try {
            Copy-Item -LiteralPath $BinaryPath -Destination $DestinationTemp
            if ($HadExisting) {
                [System.IO.File]::Replace($DestinationTemp, $Destination, $DestinationBackup)
            } else {
                [System.IO.File]::Move($DestinationTemp, $Destination)
            }
        } finally {
            Remove-Item -Force -ErrorAction SilentlyContinue $DestinationTemp
            Remove-Item -Force -ErrorAction SilentlyContinue $DestinationBackup
        }

        if ($HadExisting) {
            Write-Host "Updated aw to $DownloadedVersion at $Destination"
        } else {
            Write-Host "Installed aw $DownloadedVersion to $Destination"
        }
    }

    if (-not (($env:PATH -split ';') -contains $InstallDir)) {
        Write-Host "Add $InstallDir to PATH to run 'aw' directly."
    }
}
finally {
    Remove-Item -Recurse -Force -ErrorAction SilentlyContinue $WorkDir
}
