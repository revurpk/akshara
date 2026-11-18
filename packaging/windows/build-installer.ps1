<#
.SYNOPSIS
    Checks the build dependencies, fetches missing ones, and builds the
    Akshara Office Windows installer and portable zip.

.DESCRIPTION
    Run from any directory, in Windows PowerShell 5.1 or PowerShell 7:

        powershell -ExecutionPolicy Bypass -File packaging\windows\build-installer.ps1

    Dependencies and how missing ones are handled:

      Tool                          Needed for            If missing
      ----------------------------  --------------------  ---------------------------------
      Visual Studio 2022 C++ tools  Rust + Flutter build  error (or -InstallVsBuildTools)
      Rust (MSVC, >= 1.85)          the core library      rustup, per user (~\.cargo)
      Git                           the Flutter tool      MinGit, into -ToolsDir
      Flutter (Dart >= 3.14)        the app               Flutter stable, into -ToolsDir
      Inno Setup 6                  the setup .exe        per-user install, into -ToolsDir

    Everything except the Visual Studio Build Tools installs without
    administrator rights. Microsoft's Build Tools installer always asks for
    elevation, so it runs only when -InstallVsBuildTools is given.

    Every download uses HTTPS and is verified before use: rustup and Flutter
    against their published SHA-256 sums, MinGit and Inno Setup (both from
    their official GitHub releases) against GitHub's asset digests, and the
    Inno Setup and Build Tools installers also by their Authenticode
    signatures. Set GITHUB_TOKEN to avoid GitHub's anonymous API rate limit.
    Tools found in -ToolsDir are reused on later runs. The only
    persistent change outside -ToolsDir is rustup's own (~\.cargo and the user
    PATH).

    Outputs (in dist\):
      akshara-<version>-windows-x64-setup.exe     per-user installer, no UAC
      akshara-<version>-windows-x64-portable.zip  unzip anywhere, run akshara.exe

.PARAMETER Offline
    Never download anything. Missing tools are reported and the script stops.

.PARAMETER CheckOnly
    Only report which dependencies are present; build nothing.

.PARAMETER InstallVsBuildTools
    Install the Visual Studio 2022 Build Tools (C++ workload and CMake) if
    missing. This shows a UAC prompt.

.PARAMETER ToolsDir
    Where fetched tools go. Default: %LOCALAPPDATA%\Akshara\build-tools.

.PARAMETER Test
    Run the Rust and Flutter test suites before packaging.

.PARAMETER NoPortable
    Skip the portable zip.

.EXAMPLE
    .\packaging\windows\build-installer.ps1 -CheckOnly

.EXAMPLE
    .\packaging\windows\build-installer.ps1 -Test
#>
[CmdletBinding()]
param(
    [switch]$Offline,
    [switch]$CheckOnly,
    [switch]$InstallVsBuildTools,
    [string]$ToolsDir = (Join-Path $env:LOCALAPPDATA 'Akshara\build-tools'),
    [switch]$Test,
    [switch]$NoPortable
)

Set-StrictMode -Version 3.0
$ErrorActionPreference = 'Stop'
$ProgressPreference = 'SilentlyContinue'   # Invoke-WebRequest is much faster without it
[Net.ServicePointManager]::SecurityProtocol = [Net.SecurityProtocolType]::Tls12
Add-Type -AssemblyName System.IO.Compression.FileSystem

$MinRust = [version]'1.85.0'
$MinDart = [version]'3.14.0'
$RepoRoot = (Resolve-Path (Join-Path $PSScriptRoot '..\..')).Path
$DownloadDir = Join-Path $ToolsDir 'downloads'
$env:FLUTTER_SUPPRESS_ANALYTICS = 'true'
$env:DART_SUPPRESS_ANALYTICS = 'true'

# ---------------------------------------------------------------- helpers ---

function Write-Step([string]$Message) { Write-Host "==> $Message" -ForegroundColor Cyan }
function Write-Ok([string]$Message) { Write-Host "    ok  $Message" -ForegroundColor Green }
function Write-Missing([string]$Message) { Write-Host "    --  $Message" -ForegroundColor Yellow }

# Runs a native command and stops if it fails.
function Invoke-Native {
    param([Parameter(Mandatory)][string]$Exe, [string[]]$Arguments = @())
    & $Exe @Arguments
    if ($LASTEXITCODE -ne 0) {
        throw "'$Exe $($Arguments -join ' ')' failed with exit code $LASTEXITCODE."
    }
}

function Add-ToPath([string]$Dir) {
    $parts = $env:PATH -split ';'
    if ($parts -notcontains $Dir) { $env:PATH = "$Dir;$env:PATH" }
}

function Find-Command([string]$Name) {
    $cmd = Get-Command $Name -CommandType Application -ErrorAction SilentlyContinue |
        Select-Object -First 1
    if ($cmd) { return $cmd.Source }
    return $null
}

function Assert-CanDownload([string]$What, [string]$Manual) {
    if ($Offline -or $CheckOnly) {
        throw "$What is missing and downloads are disabled. $Manual"
    }
}

# Downloads over HTTPS with a few retries; returns the local path.
function Get-Download([string]$Url, [string]$FileName) {
    if (-not $Url.StartsWith('https://')) { throw "Refusing non-HTTPS download: $Url" }
    New-Item -ItemType Directory -Force -Path $DownloadDir | Out-Null
    $out = Join-Path $DownloadDir $FileName
    for ($attempt = 1; $attempt -le 3; $attempt++) {
        try {
            Write-Host "    downloading $Url"
            Invoke-WebRequest -Uri $Url -OutFile $out -UseBasicParsing
            return $out
        } catch {
            if ($attempt -eq 3) { throw "Download failed: $Url ($($_.Exception.Message))" }
            Start-Sleep -Seconds (3 * $attempt)
        }
    }
}

# Calls the GitHub REST API; set GITHUB_TOKEN to avoid the anonymous rate limit.
function Invoke-GitHubApi([string]$Path) {
    $headers = @{ 'User-Agent' = 'akshara-build'; 'Accept' = 'application/vnd.github+json' }
    if ($env:GITHUB_TOKEN) { $headers['Authorization'] = "Bearer $env:GITHUB_TOKEN" }
    return Invoke-RestMethod -UseBasicParsing -Uri "https://api.github.com/$Path" -Headers $headers
}

# Verifies a download against the "sha256:..." digest GitHub publishes for a release asset.
function Assert-GitHubDigest([string]$Path, $Asset) {
    if (-not ($Asset.PSObject.Properties['digest'] -and $Asset.digest -like 'sha256:*')) {
        Remove-Item -Force $Path
        throw "GitHub published no SHA-256 digest for $($Asset.name); refusing to use it."
    }
    Assert-Sha256 $Path ($Asset.digest -replace '^sha256:', '')
}

function Assert-Sha256([string]$Path, [string]$Expected) {
    $actual = (Get-FileHash -Algorithm SHA256 -Path $Path).Hash
    if ($actual -ne $Expected.Trim().ToUpperInvariant()) {
        Remove-Item -Force $Path
        throw "Checksum mismatch for $(Split-Path -Leaf $Path): expected $Expected, got $actual."
    }
    Write-Host "    sha256 verified"
}

# Requires a valid Authenticode signature, optionally from one of the named publishers.
function Assert-Signed([string]$Path, [string[]]$ExpectedSigners = @()) {
    $sig = Get-AuthenticodeSignature -FilePath $Path
    $subject = ''
    if ($sig.SignerCertificate) { $subject = $sig.SignerCertificate.Subject }
    $known = (-not $ExpectedSigners) -or
        (@($ExpectedSigners | Where-Object { $subject -like "*$_*" }).Count -gt 0)
    if ($sig.Status -ne 'Valid' -or -not $known) {
        Remove-Item -Force $Path
        $who = 'a trusted publisher'
        if ($ExpectedSigners) { $who = $ExpectedSigners -join ' or ' }
        throw "$(Split-Path -Leaf $Path) is not validly signed by $who " +
            "(status: $($sig.Status), signer: '$subject')."
    }
    Write-Host "    signature verified: $($sig.SignerCertificate.Subject)"
}

function Expand-Zip([string]$Zip, [string]$Destination) {
    if (Test-Path $Destination) { Remove-Item -Recurse -Force $Destination }
    New-Item -ItemType Directory -Force -Path $Destination | Out-Null
    [IO.Compression.ZipFile]::ExtractToDirectory($Zip, $Destination)
}

# --------------------------------------------------------- dependencies ---

function Get-VsWhere {
    $path = Join-Path ${env:ProgramFiles(x86)} 'Microsoft Visual Studio\Installer\vswhere.exe'
    if (Test-Path $path) { return $path }
    return $null
}

function Test-VsCpp {
    $vswhere = Get-VsWhere
    if (-not $vswhere) { return $null }
    $found = & $vswhere -latest -products * -version '[17.0,)' `
        -requires Microsoft.VisualStudio.Component.VC.Tools.x86.x64 Microsoft.VisualStudio.Component.VC.CMake.Project `
        -property displayName
    if ($LASTEXITCODE -eq 0 -and $found) { return ($found | Select-Object -First 1) }
    return $null
}

function Ensure-VsBuildTools {
    $vs = Test-VsCpp
    if ($vs) { Write-Ok "Visual Studio C++ tools: $vs"; return }
    Write-Missing 'Visual Studio 2022 with the C++ desktop workload and CMake tools'
    $manual = 'Install "Desktop development with C++" from https://visualstudio.microsoft.com/downloads/ ' +
        '(Build Tools for Visual Studio 2022 is enough), or re-run with -InstallVsBuildTools.'
    if (-not $InstallVsBuildTools) { throw "Visual Studio C++ tools are missing. $manual" }
    Assert-CanDownload 'Visual Studio Build Tools' $manual
    $installer = Get-Download 'https://aka.ms/vs/17/release/vs_buildtools.exe' 'vs_buildtools.exe'
    Assert-Signed $installer @('Microsoft Corporation')
    Write-Host '    installing (a UAC prompt will appear; this takes a while)'
    $vsArgs = @('--quiet', '--wait', '--norestart', '--nocache',
        '--add', 'Microsoft.VisualStudio.Workload.VCTools', '--includeRecommended',
        '--add', 'Microsoft.VisualStudio.Component.VC.CMake.Project')
    $proc = Start-Process -FilePath $installer -ArgumentList $vsArgs -Verb RunAs -Wait -PassThru
    if ($proc.ExitCode -eq 3010) {
        Write-Warning 'Build Tools installed; Windows asks for a restart. Restart, then run this script again.'
        exit 3010
    }
    if ($proc.ExitCode -ne 0) { throw "Build Tools installer failed with exit code $($proc.ExitCode)." }
    $vs = Test-VsCpp
    if (-not $vs) { throw 'Build Tools installed, but the C++ workload was not detected.' }
    Write-Ok "Visual Studio C++ tools: $vs"
}

function Get-RustVersion {
    $rustc = Find-Command 'rustc'
    if (-not $rustc) { return $null }
    $info = & $rustc -vV
    $release = ($info | Where-Object { $_ -like 'release:*' }) -replace 'release:\s*', ''
    $hostLine = ($info | Where-Object { $_ -like 'host:*' }) -replace 'host:\s*', ''
    return [pscustomobject]@{ Version = [version]($release -replace '-.*', ''); Host = $hostLine }
}

function Ensure-Rust {
    Add-ToPath (Join-Path $env:USERPROFILE '.cargo\bin')
    $rust = Get-RustVersion
    $manual = 'Install Rust from https://rustup.rs (per user, no admin rights needed).'
    if (-not $rust) {
        Write-Missing 'Rust'
        Assert-CanDownload 'Rust' $manual
        $base = 'https://static.rust-lang.org/rustup/dist/x86_64-pc-windows-msvc/rustup-init.exe'
        $init = Get-Download $base 'rustup-init.exe'
        $sum = Get-Download "$base.sha256" 'rustup-init.exe.sha256'
        Assert-Sha256 $init ((Get-Content $sum -Raw) -split '\s+')[0]
        Invoke-Native $init @('-y', '--profile', 'minimal', '--default-toolchain', 'stable',
            '--default-host', 'x86_64-pc-windows-msvc')
        $rust = Get-RustVersion
        if (-not $rust) { throw 'rustup finished but rustc is still not on PATH.' }
    }
    if ($rust.Host -notlike '*-pc-windows-msvc') {
        Write-Missing "Rust uses the $($rust.Host) toolchain; Flutter needs MSVC"
        Assert-CanDownload 'The Rust MSVC toolchain' 'Run: rustup default stable-x86_64-pc-windows-msvc'
        Invoke-Native 'rustup' @('default', 'stable-x86_64-pc-windows-msvc')
        $rust = Get-RustVersion
    }
    if ($rust.Version -lt $MinRust) {
        Write-Missing "Rust $($rust.Version) is older than $MinRust"
        Assert-CanDownload "Rust $MinRust" 'Run: rustup update stable'
        Invoke-Native 'rustup' @('update', 'stable')
        $rust = Get-RustVersion
    }
    Write-Ok "Rust $($rust.Version) ($($rust.Host))"
}

function Ensure-Git {
    $mingit = Join-Path $ToolsDir 'mingit\cmd'
    if (Test-Path (Join-Path $mingit 'git.exe')) { Add-ToPath $mingit }
    $git = Find-Command 'git'
    if ($git) { Write-Ok "Git: $git"; return }
    Write-Missing 'Git (the Flutter tool needs it)'
    Assert-CanDownload 'Git' 'Install Git for Windows from https://git-scm.com/download/win (a per-user install is offered).'
    $release = Invoke-GitHubApi 'repos/git-for-windows/git/releases/latest'
    $asset = $release.assets | Where-Object { $_.name -match '^MinGit-[\d.]+-64-bit\.zip$' } | Select-Object -First 1
    if (-not $asset) { throw 'Could not find a MinGit download in the latest Git for Windows release.' }
    $zip = Get-Download $asset.browser_download_url $asset.name
    Assert-GitHubDigest $zip $asset
    Expand-Zip $zip (Join-Path $ToolsDir 'mingit')
    Add-ToPath $mingit
    Write-Ok "Git: $(Find-Command 'git')"
}

function Get-DartVersion {
    $flutter = Find-Command 'flutter.bat'
    if (-not $flutter) { return $null }
    # Windows PowerShell 5.1 turns redirected stderr into errors; tolerate them here.
    $ErrorActionPreference = 'Continue'
    $text = (& $flutter --version --machine 2>$null) -join "`n"
    if ($LASTEXITCODE -ne 0) { return $null }
    # The first run may print set-up messages before the JSON object.
    $start = $text.IndexOf('{'); $end = $text.LastIndexOf('}')
    if ($start -lt 0 -or $end -le $start) { return $null }
    $info = $text.Substring($start, $end - $start + 1) | ConvertFrom-Json
    return [pscustomobject]@{
        Flutter = $info.frameworkVersion
        Dart    = [version](($info.dartSdkVersion -split '[ -]')[0])
    }
}

function Ensure-Flutter {
    $local = Join-Path $ToolsDir 'flutter\bin'
    if (Test-Path (Join-Path $local 'flutter.bat')) { Add-ToPath $local }
    $ver = Get-DartVersion
    if ($ver -and $ver.Dart -ge $MinDart) { Write-Ok "Flutter $($ver.Flutter) (Dart $($ver.Dart))"; return }
    if ($ver) {
        Write-Missing "Flutter $($ver.Flutter) has Dart $($ver.Dart); Dart $MinDart or newer is needed"
    } else {
        Write-Missing 'Flutter'
    }
    Assert-CanDownload 'A recent Flutter SDK' 'Download Flutter stable from https://docs.flutter.dev/get-started/install and add flutter\bin to PATH.'
    $index = Invoke-RestMethod -UseBasicParsing -Uri 'https://storage.googleapis.com/flutter_infra_release/releases/releases_windows.json'
    $stable = $index.releases | Where-Object { $_.hash -eq $index.current_release.stable } | Select-Object -First 1
    if (-not $stable) { throw 'Could not find the current stable Flutter release.' }
    if ([version](($stable.dart_sdk_version -split '[ -]')[0]) -lt $MinDart) {
        throw "The current Flutter stable ($($stable.version)) has Dart $($stable.dart_sdk_version); Dart $MinDart is needed."
    }
    $zip = Get-Download "$($index.base_url)/$($stable.archive)" (Split-Path -Leaf $stable.archive)
    Assert-Sha256 $zip $stable.sha256
    Write-Host '    extracting (this takes a few minutes)'
    $dest = Join-Path $ToolsDir 'flutter'
    if (Test-Path $dest) { Remove-Item -Recurse -Force $dest }
    [IO.Compression.ZipFile]::ExtractToDirectory($zip, $ToolsDir)   # the archive holds flutter\
    Add-ToPath $local
    Invoke-Native 'flutter.bat' @('--disable-analytics')
    $ver = Get-DartVersion
    if (-not $ver) { throw 'Flutter was extracted but does not run.' }
    Write-Ok "Flutter $($ver.Flutter) (Dart $($ver.Dart))"
}

function Find-Iscc {
    $candidates = @(
        (Find-Command 'iscc'),
        (Join-Path $ToolsDir 'innosetup\ISCC.exe')
    )
    # An existing Inno Setup 6 or 7 install works too; akshara.iss uses only 6.x features.
    foreach ($v in @('6', '7')) {
        $candidates += (Join-Path $env:LOCALAPPDATA "Programs\Inno Setup $v\ISCC.exe")
        $candidates += (Join-Path ${env:ProgramFiles(x86)} "Inno Setup $v\ISCC.exe")
        $candidates += (Join-Path $env:ProgramFiles "Inno Setup $v\ISCC.exe")
    }
    foreach ($c in $candidates) { if ($c -and (Test-Path $c)) { return $c } }
    return $null
}

function Ensure-InnoSetup {
    $iscc = Find-Iscc
    if ($iscc) { Write-Ok "Inno Setup: $iscc"; return $iscc }
    Write-Missing 'Inno Setup 6'
    Assert-CanDownload 'Inno Setup 6' 'Install it from https://jrsoftware.org/isdl.php and choose "Install for me only".'
    # Official installers are published as immutable GitHub releases with SHA-256
    # digests. Take the newest stable 6.x release (the line akshara.iss targets).
    $releases = Invoke-GitHubApi 'repos/jrsoftware/issrc/releases?per_page=50'
    $release = $releases |
        Where-Object { -not $_.prerelease -and -not $_.draft -and $_.tag_name -match '^is-6_\d+_\d+$' } |
        Sort-Object { [version](($_.tag_name -replace '^is-', '') -replace '_', '.') } -Descending |
        Select-Object -First 1
    if (-not $release) { throw 'Could not find an Inno Setup 6 release on GitHub (jrsoftware/issrc).' }
    $asset = $release.assets | Where-Object { $_.name -match '^innosetup-6\.\d+\.\d+\.exe$' } | Select-Object -First 1
    if (-not $asset) { throw "Release $($release.tag_name) has no Inno Setup installer." }
    $installer = Get-Download $asset.browser_download_url $asset.name
    Assert-GitHubDigest $installer $asset
    Assert-Signed $installer
    $dir = Join-Path $ToolsDir 'innosetup'
    $isArgs = @('/VERYSILENT', '/SUPPRESSMSGBOXES', '/NORESTART', '/SP-', '/CURRENTUSER',
        '/NOICONS', "/DIR=`"$dir`"")
    $proc = Start-Process -FilePath $installer -ArgumentList $isArgs -Wait -PassThru
    if ($proc.ExitCode -ne 0) { throw "Inno Setup installer failed with exit code $($proc.ExitCode)." }
    $iscc = Find-Iscc
    if (-not $iscc) { throw 'Inno Setup was installed but ISCC.exe was not found.' }
    Write-Ok "Inno Setup: $iscc"
    return $iscc
}

# ----------------------------------------------------------------- build ---

function Get-WorkspaceVersion {
    $line = Get-Content (Join-Path $RepoRoot 'Cargo.toml') |
        Where-Object { $_ -match '^version\s*=\s*"([^"]+)"' } | Select-Object -First 1
    if (-not $line) { throw 'No version found in Cargo.toml.' }
    return ([regex]::Match($line, '"([^"]+)"')).Groups[1].Value
}

if ($env:OS -ne 'Windows_NT') { throw 'Run this script on Windows. On other systems use the build scripts in packaging/.' }
if ([Environment]::Is64BitOperatingSystem -eq $false) { throw 'A 64-bit Windows is required.' }

$failed = @()
Write-Step "Checking build dependencies (fetched tools go to $ToolsDir)"
foreach ($ensure in @('Ensure-VsBuildTools', 'Ensure-Rust', 'Ensure-Git', 'Ensure-Flutter')) {
    try { & $ensure } catch {
        if (-not $CheckOnly) { throw }
        $failed += $_.Exception.Message
    }
}
$iscc = $null
try { $iscc = Ensure-InnoSetup } catch {
    if (-not $CheckOnly) { throw }
    $failed += $_.Exception.Message
}
if ($CheckOnly) {
    if ($failed.Count -gt 0) {
        $failed | ForEach-Object { Write-Host "    $_" -ForegroundColor Yellow }
        Write-Host 'Run without -CheckOnly to fetch what can be fetched.'
        exit 1
    }
    Write-Host 'All build dependencies are present.' -ForegroundColor Green
    exit 0
}

$version = Get-WorkspaceVersion
$appDir = Join-Path $RepoRoot 'app'
$releaseDir = Join-Path $appDir 'build\windows\x64\runner\Release'
$distDir = Join-Path $RepoRoot 'dist'

Push-Location $appDir
try {
    Write-Step 'Fetching Dart packages'
    & flutter.bat pub get --offline
    if ($LASTEXITCODE -ne 0) {
        if ($Offline) { throw "'flutter pub get --offline' failed and downloads are disabled." }
        Write-Host '    not in the local pub cache yet; fetching once'
        Invoke-Native 'flutter.bat' @('pub', 'get')
    }

    if ($Test) {
        Write-Step 'Running tests'
        Push-Location $RepoRoot
        try {
            Invoke-Native 'cargo' @('test', '--workspace')
            Invoke-Native 'cargo' @('build', '-p', 'akshara-ffi')
        } finally { Pop-Location }
        Invoke-Native 'flutter.bat' @('test')
    }

    Write-Step "Building Akshara $version for Windows (release)"
    Invoke-Native 'flutter.bat' @('build', 'windows', '--release')
} finally { Pop-Location }

foreach ($f in @('akshara.exe', 'akshara.dll', 'flutter_windows.dll')) {
    if (-not (Test-Path (Join-Path $releaseDir $f))) { throw "Build output is missing $f in $releaseDir." }
}

New-Item -ItemType Directory -Force -Path $distDir | Out-Null
Write-Step 'Building the installer'
Invoke-Native $iscc @('/Q', "/DAppVersion=$version", (Join-Path $PSScriptRoot 'akshara.iss'))
$outputs = @(Join-Path $distDir "akshara-$version-windows-x64-setup.exe")

if (-not $NoPortable) {
    Write-Step 'Building the portable zip'
    $zip = Join-Path $distDir "akshara-$version-windows-x64-portable.zip"
    if (Test-Path $zip) { Remove-Item -Force $zip }
    [IO.Compression.ZipFile]::CreateFromDirectory($releaseDir, $zip,
        [IO.Compression.CompressionLevel]::Optimal, $false)
    $outputs += $zip
}

Write-Step 'Done'
foreach ($o in $outputs) {
    if (-not (Test-Path $o)) { throw "Expected output is missing: $o" }
    $hash = (Get-FileHash -Algorithm SHA256 -Path $o).Hash.ToLowerInvariant()
    $size = '{0:N1} MB' -f ((Get-Item $o).Length / 1MB)
    Write-Host "    $(Split-Path -Leaf $o)  ($size)  sha256 $hash"
}
