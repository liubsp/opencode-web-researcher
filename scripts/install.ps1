param(
    [switch]$Global,
    [string]$Project,
    [string]$Ref = 'main',
    [string]$InstallDir,
    [string]$SourceDirectory
)
$ErrorActionPreference = 'Stop'
$homeDir = if ($env:WEB_RESEARCH_HOME) { $env:WEB_RESEARCH_HOME }
        elseif (Test-Path "$env:LOCALAPPDATA\web-research-opencode" -PathType Container) { "$env:LOCALAPPDATA\web-research-opencode" }
        else { "$env:LOCALAPPDATA\opencode-web-researcher" }
if (-not $InstallDir) { $InstallDir = Join-Path $homeDir 'app' }
if ($Global -and $Project) { throw 'Choose -Global or -Project, not both' }
function Run([string]$Program, [string[]]$Arguments) {
    & $Program @Arguments
    if ($LASTEXITCODE -ne 0) { throw "$Program failed ($LASTEXITCODE)" }
}
function FileHash([string]$Path) {
    # Some PowerShell installations omit the Get-FileHash script module.
    $stream = [IO.File]::OpenRead($Path)
    $sha = [Security.Cryptography.SHA256]::Create()
    try { return [BitConverter]::ToString($sha.ComputeHash($stream)).Replace('-','').ToLowerInvariant() }
    finally { $sha.Dispose(); $stream.Dispose() }
}
foreach ($tool in @('node','npm.cmd','cargo')) { Get-Command $tool -ErrorAction Stop | Out-Null }
if ($Project) { $Project = (Resolve-Path -LiteralPath $Project).Path }
$InstallDir = [IO.Path]::GetFullPath($InstallDir)
New-Item -ItemType Directory -Force $InstallDir | Out-Null
$lock = [IO.File]::Open((Join-Path $InstallDir 'install.lock'), 'OpenOrCreate', 'ReadWrite', 'None')
$stage = Join-Path ([IO.Path]::GetTempPath()) ('web-research-install-' + [guid]::NewGuid())
$server = Join-Path $InstallDir 'bin\opencode-web-researcher.exe'
$legacyServer = $server
$dataHome = [IO.Path]::GetFullPath($homeDir)
$previousHome = $env:WEB_RESEARCH_HOME
$env:WEB_RESEARCH_HOME = $dataHome
$runtime = Join-Path $InstallDir 'runtime'
try {
    New-Item -ItemType Directory $stage | Out-Null
    if ($SourceDirectory) { $source = (Resolve-Path -LiteralPath $SourceDirectory).Path }
    else {
    $archive = Join-Path $stage 'source.zip'
    Invoke-WebRequest "https://github.com/liubsp/opencode-web-researcher/archive/$([uri]::EscapeDataString($Ref)).zip" -OutFile $archive
    Expand-Archive $archive (Join-Path $stage 'source')
    $source = (Get-ChildItem (Join-Path $stage 'source') -Directory | Select-Object -First 1).FullName
    }
    Push-Location $source
    try {
        Run node @('scripts/build-release.mjs')
        Run npm.cmd @('ci')
        Run npm.cmd @('run','build')
        Run npm.cmd @('pack','--workspace','opencode-web-researcher','--pack-destination',$stage)
    } finally { Pop-Location }
    $package = (Get-ChildItem $stage -Filter '*.tgz' | Select-Object -First 1).FullName
    # Build succeeds before interrupting the installed daemon.
    $candidate = Join-Path $source 'target\release\opencode-web-researcher.exe'
    # Immutable paths cannot be locked by clients executing a previous build.
    $hash = FileHash $candidate
    $server = Join-Path $InstallDir ("bin\updates\$hash\opencode-web-researcher.exe")
    New-Item -ItemType Directory -Force (Split-Path $server) | Out-Null
    if (-not (Test-Path $server)) { Copy-Item $candidate $server }
    if ((FileHash $server) -ne $hash) { throw 'Installed server build differs from candidate' }
    $previousAgent = Join-Path $stage 'previous-agent.md'
    $bundledAgent = Join-Path $runtime 'node_modules\opencode-web-researcher\agents\web-researcher.md'
    if (Test-Path $bundledAgent) { Copy-Item $bundledAgent $previousAgent }
    $previousRevisions = Join-Path $stage 'previous-revisions'
    $legacyRevisions = Join-Path $runtime 'node_modules\opencode-web-researcher\dist\updates'
    if (Test-Path $legacyRevisions) { Copy-Item $legacyRevisions $previousRevisions -Recurse }
    $replaceBinary = -not (Test-Path $legacyServer) -or (FileHash $candidate) -ne (FileHash $legacyServer)
    New-Item -ItemType Directory -Force (Split-Path $server),$runtime | Out-Null
    try { Run npm.cmd @('install','--prefix',$runtime,'--omit=dev','--no-audit','--no-fund',$package) }
    finally {
        if (Test-Path $previousRevisions) {
            New-Item -ItemType Directory -Force $legacyRevisions | Out-Null
            Get-ChildItem $previousRevisions | Copy-Item -Destination $legacyRevisions -Recurse -Force
        }
    }
    Run node @((Join-Path $source 'scripts\revise-plugin.mjs'),(Join-Path $runtime 'node_modules\opencode-web-researcher'))
    Run $server @('configure')
    $installerArgs = @('--home',$dataHome)
    if (Test-Path $previousAgent) { $installerArgs += @('--previous-agent',$previousAgent) }
    if ($Global) {
        Run node (@((Join-Path $runtime 'node_modules\opencode-web-researcher\dist\install.js'),'--global','--binary',$server) + $installerArgs)
    } elseif ($Project) {
        Run node (@((Join-Path $runtime 'node_modules\opencode-web-researcher\dist\install.js'),'--project',$Project,'--binary',$server) + $installerArgs)
    }
    # Activation holds the shared startup lock through replacement registration.
    # It never closes Chrome, and all newer launchers consult its preferred-build pointer.
    Run $server @('activate')
    if ($replaceBinary) {
        $copied = $false
        for ($attempt = 0; $attempt -lt 30 -and -not $copied; $attempt++) {
            try { Copy-Item $candidate $legacyServer -Force; $copied = $true }
            catch { Start-Sleep -Seconds 1 }
        }
        if (-not $copied) { throw 'New daemon is active, but the legacy launcher is still locked; installation requires completing that replacement' }
    }
    Write-Output "Installed server: $server"
    Write-Output 'Reload OpenCode configuration to load updated tools. Already-running calls retain their previous definitions. Existing research data/login are preserved.'
} finally {
    $env:WEB_RESEARCH_HOME = $previousHome
    $lock.Dispose()
    if (Test-Path $stage) { Remove-Item $stage -Recurse -Force }
}
