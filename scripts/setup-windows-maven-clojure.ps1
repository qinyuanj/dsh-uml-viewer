#Requires -Version 5.1
<#
.SYNOPSIS
    Install Maven and the Clojure CLI on native Windows x64.

.DESCRIPTION
    The Go, Java and Clojure fixtures need their own toolchains before
    crates/uml-viewer/tests/crap_runner_smoke.rs and
    crates/mutator/tests/go_java_clojure.rs can execute instead of skipping.
    Go 1.25.3 and Temurin JDK 17 come from actions/setup-go and
    actions/setup-java; this script installs the other two, under the current
    user, with no administrator prompt:

        %USERPROFILE%/dsh-language-fixtures/apache-maven-3.9.16   Maven
        %USERPROFILE%/dsh-language-fixtures/ClojureTools         the module
        %USERPROFILE%/dsh-language-fixtures/bin                  the clj launcher

    It ends by writing both bin directories to PATH and to GITHUB_PATH, so a
    later step starts from the job's environment rather than this process's,
    and by proving that mvn -v and clj --version answer.

    Why Maven is installed rather than borrowed from the runner image: the
    image's copy is not pinned, and the Java fixture's JaCoCo report plus the
    Java mutation cases are the gate. The archive URL is immutable and the
    download is hash-checked, the way scripts/setup-windows-lua.ps1 pins its
    downloads.

    Why the Clojure CLI needs a launcher at all: the official Windows
    distribution is the ClojureTools PowerShell module (download.clojure.org
    ships a zip of it), not an executable, so clj exists only as a PowerShell
    alias. crapper and mutator start clj as a program argument list, and the
    tests probe it the same way; a .cmd launcher on PATH is what cmd.exe (and
    therefore both tools) can start. The generated clj.cmd -> clj.ps1 pair
    imports the module by absolute path and forwards the arguments.

    Why the launcher rebuilds -M:cov: ClojureTools.psm1 takes the alias as a
    separate argument after -M:, and that is the shape a run has to reach it
    in. Every caller in this repository (quality-core's Clojure runner,
    mutator's coverage script and test plan) spells the single token -M:cov,
    which is also what a person types, and PowerShell delivers those two
    differently: typing clj -M:cov hands the function the pair -M: / cov, but
    a -File script receives -M / cov, with the colon dropped. The launcher
    therefore re-inserts the colon as its own token and passes the alias after
    it, so the module sees exactly what typing -M:cov produced. -X: and -T:
    are the same shape; nothing here uses them yet.

    Re-running is safe: a component already at the pinned version is kept and
    the PATH entries are not duplicated.

.PARAMETER InstallRoot
    Where the toolchains are unpacked. Defaults to
    %USERPROFILE%/dsh-language-fixtures.

.EXAMPLE
    powershell -ExecutionPolicy Bypass -File scripts/setup-windows-maven-clojure.ps1
#>
[CmdletBinding()]
param(
    [string]$InstallRoot = (Join-Path $env:USERPROFILE 'dsh-language-fixtures')
)

$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest

$MavenVersion = '3.9.16'
$MavenSha512 = 'ed41650d42485cfc243fad22158caf9cbb5dc408ce7a09ddb94dd42a019de929ca43065bfa450612cf12bf78b5cafa3884b96c090de326ff590448c933454af3'
$ClojureVersion = '1.12.6.1673'
$ClojureSha256 = 'bb2f8a9f3fa94834813bd437b2a28b857c2f4b7267cb80168c0c6b910f883d4f'

$Downloads = Join-Path $InstallRoot 'downloads'
$MavenRoot = Join-Path $InstallRoot "apache-maven-$MavenVersion"
$ClojureRoot = Join-Path $InstallRoot 'ClojureTools'
$BinDir = Join-Path $InstallRoot 'bin'
$MavenBin = Join-Path $MavenRoot 'bin'

# Windows PowerShell 5.1 can still default to TLS 1.0 on older builds.
if ($PSVersionTable.PSVersion.Major -lt 7) {
    [Net.ServicePointManager]::SecurityProtocol = [Net.ServicePointManager]::SecurityProtocol -bor [Net.SecurityProtocolType]::Tls12
}

New-Item -ItemType Directory -Force -Path $Downloads, $BinDir | Out-Null

function Install-PinnedZip {
    param(
        [Parameter(Mandatory)] [string]$Uri,
        [Parameter(Mandatory)] [string]$Zip,
        [Parameter(Mandatory)] [string]$Destination,
        [Parameter(Mandatory)] [string]$Algorithm,
        [Parameter(Mandatory)] [string]$Hash,
        [Parameter(Mandatory)] [string]$Marker
    )
    if (Test-Path (Join-Path $Destination $Marker)) {
        Write-Host "kept $Destination"
        return
    }
    if (-not (Test-Path $Zip)) {
        Write-Host "downloading $Uri"
        Invoke-WebRequest -Uri $Uri -OutFile $Zip -UseBasicParsing
    }
    $actual = (Get-FileHash -Path $Zip -Algorithm $Algorithm).Hash.ToLowerInvariant()
    if ($actual -ne $Hash.ToLowerInvariant()) {
        Remove-Item -Path $Zip -Force
        throw "$Algorithm mismatch for ${Uri}: $actual"
    }
    Expand-Archive -Path $Zip -DestinationPath $Destination -Force
    Write-Host "unpacked $Zip to $Destination"
}

Install-PinnedZip -Uri "https://archive.apache.org/dist/maven/maven-3/$MavenVersion/binaries/apache-maven-$MavenVersion-bin.zip" `
    -Zip (Join-Path $Downloads "apache-maven-$MavenVersion-bin.zip") `
    -Destination $InstallRoot -Algorithm SHA512 -Hash $MavenSha512 -Marker "apache-maven-$MavenVersion/bin/mvn.cmd"

# The zip's top level is ClojureTools/: the module manifest, its
# implementation, deps.edn and the clojure-tools jar the module runs.
Install-PinnedZip -Uri "https://download.clojure.org/install/clojure-tools-$ClojureVersion.zip" `
    -Zip (Join-Path $Downloads "clojure-tools-$ClojureVersion.zip") `
    -Destination $InstallRoot -Algorithm SHA256 -Hash $ClojureSha256 -Marker 'ClojureTools/ClojureTools.psd1'

$Launcher = @'
# Generated by scripts/setup-windows-maven-clojure.ps1 -- do not edit.
# The clj the quality tools start on native Windows: the official Clojure CLI
# distribution here is the ClojureTools PowerShell module, so this launcher
# imports it by absolute path and forwards the arguments.
$ErrorActionPreference = 'Stop'
Import-Module (Join-Path (Split-Path $PSScriptRoot -Parent) 'ClojureTools/ClojureTools.psd1')

# ClojureTools.psm1 takes the alias as a separate argument after -M:, which is
# what typing clj -M:cov produces. A -File script does not: PowerShell splits
# -M:cov into -M and cov and drops the colon, so put the colon back as its own
# token and let the alias follow it. An intact -M:alias is handled too, for a
# caller that reaches this script without that split.
$forwarded = @($args)
if ($forwarded.Count -gt 0 -and $forwarded[0] -match '^-(M|X|T):(.+)$') {
    $forwarded = @("$($Matches[1]):", $Matches[2]) + @($forwarded | Select-Object -Skip 1)
} elseif ($forwarded.Count -gt 1 -and $forwarded[0] -match '^-(M|X|T)$' -and $forwarded[1] -notmatch '^-') {
    $forwarded = @("$($forwarded[0]):") + $forwarded[1..($forwarded.Count - 1)]
}
clj @forwarded
if ($null -eq $LASTEXITCODE) { exit 0 }
exit $LASTEXITCODE
'@

$Batch = @'
@echo off
rem Generated by scripts/setup-windows-maven-clojure.ps1 -- do not edit.
rem cmd.exe starts this launcher, which is how crapper, mutator and the tests
rem reach clj; the official module itself is only a PowerShell alias.
powershell -NoLogo -NoProfile -ExecutionPolicy Bypass -File "%~dp0clj.ps1" %*
exit /b %ERRORLEVEL%
'@

Set-Content -Path (Join-Path $BinDir 'clj.ps1') -Value $Launcher -Encoding utf8
# ASCII, so cmd.exe never meets a byte-order mark.
Set-Content -Path (Join-Path $BinDir 'clj.cmd') -Value $Batch -Encoding ascii

$prepended = @($MavenBin, $BinDir)
$env:PATH = (($prepended + ($env:PATH -split ';' | Where-Object { $_ })) | Select-Object -Unique) -join ';'
if ($env:GITHUB_PATH) {
    foreach ($directory in $prepended) {
        Add-Content -Path $env:GITHUB_PATH -Value $directory -Encoding ascii
    }
}

if (-not $env:JAVA_HOME) {
    throw 'JAVA_HOME is not set; run actions/setup-java before this script.'
}
# One native command per check: pwsh exits with the last command's code only.
& mvn -v
if ($LASTEXITCODE -ne 0) { throw "mvn -v exited $LASTEXITCODE" }
& clj --version
if ($LASTEXITCODE -ne 0) { throw "clj --version exited $LASTEXITCODE" }

Write-Host "Maven $MavenVersion and Clojure CLI $ClojureVersion are on PATH."
