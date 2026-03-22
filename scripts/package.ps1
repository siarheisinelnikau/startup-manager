param([switch]$Offline)
$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest
$projectRoot = Split-Path -Parent $PSScriptRoot
Push-Location -LiteralPath $projectRoot
try {
    $buildArguments = @('build', '--release', '--locked', '-j', '2')
    if ($Offline) { $buildArguments += '--offline' }
    & cargo @buildArguments
    if ($LASTEXITCODE -ne 0) { throw 'Release compilation failed' }
    $version = [regex]::Match((Get-Content -Raw -LiteralPath 'Cargo.toml'), '(?m)^version = "([^"]+)"').Groups[1].Value
    if ($version -notmatch '^\d+\.\d+\.\d+$') { throw 'Expected a stable package version' }
    $asset = "StartupManager-v$version.exe"
    New-Item -ItemType Directory -Force -Path 'dist' | Out-Null
    Copy-Item -LiteralPath 'target/release/startup-manager.exe' -Destination (Join-Path 'dist' $asset)
    Copy-Item -LiteralPath 'LICENSE', 'THIRD_PARTY_NOTICES.txt' -Destination 'dist'
    $digest = (Get-FileHash -LiteralPath (Join-Path 'dist' $asset) -Algorithm SHA256).Hash.ToLowerInvariant()
    [IO.File]::WriteAllText((Join-Path $projectRoot 'dist/SHA256SUMS.txt'), "$digest  $asset`n", [Text.UTF8Encoding]::new($false))
    Write-Output "Prepared dist/$asset and SHA256SUMS.txt"
} finally { Pop-Location }
