param(
    [switch]$Global,
    [string]$Project,
    [string]$Ref = 'main',
    [string]$InstallDir,
    [string]$SourceDirectory
)
$ErrorActionPreference = 'Stop'
if (-not $InstallDir) {
    $homeDir = if ($env:WEB_RESEARCH_HOME) { $env:WEB_RESEARCH_HOME }
        elseif (Test-Path "$env:LOCALAPPDATA\web-research-opencode" -PathType Container) { "$env:LOCALAPPDATA\web-research-opencode" }
        else { "$env:LOCALAPPDATA\opencode-web-researcher" }
    $InstallDir = Join-Path $homeDir 'app'
}
if ($Global -and $Project) { throw 'Choose -Global or -Project, not both' }
function Run([string]$Program, [string[]]$Arguments) {
    & $Program @Arguments
    if ($LASTEXITCODE -ne 0) { throw "$Program failed ($LASTEXITCODE)" }
}
foreach ($tool in @('node','npm.cmd','cargo')) { Get-Command $tool -ErrorAction Stop | Out-Null }
if ($Project) { $Project = (Resolve-Path -LiteralPath $Project).Path }
$InstallDir = [IO.Path]::GetFullPath($InstallDir)
New-Item -ItemType Directory -Force $InstallDir | Out-Null
$lock = [IO.File]::Open((Join-Path $InstallDir 'install.lock'), 'OpenOrCreate', 'ReadWrite', 'None')
$stage = Join-Path ([IO.Path]::GetTempPath()) ('web-research-install-' + [guid]::NewGuid())
$server = Join-Path $InstallDir 'bin\opencode-web-researcher.exe'
$runtime = Join-Path $InstallDir 'runtime'
$restart = $false
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
    if (Test-Path $candidate) {
        & $candidate status *> $null
        $restart = $LASTEXITCODE -eq 0
        if ($restart) { Run $candidate @('shutdown') }
    }
    New-Item -ItemType Directory -Force (Split-Path $server),$runtime | Out-Null
    $copied = $false
    for ($attempt = 0; $attempt -lt 30; $attempt++) {
        try { Copy-Item $candidate $server -Force; $copied = $true; break }
        catch { Start-Sleep -Seconds 1 }
    }
    if (-not $copied) { throw 'Server executable remains busy; retry after active requests finish' }
    Run npm.cmd @('install','--prefix',$runtime,'--omit=dev','--no-audit','--no-fund',$package)
    Run $server @('configure')
    if ($Global) {
        Run node @((Join-Path $runtime 'node_modules\opencode-web-researcher\dist\install.js'),'--global','--binary',$server)
    } elseif ($Project) {
        Run node @((Join-Path $runtime 'node_modules\opencode-web-researcher\dist\install.js'),'--project',$Project,'--binary',$server)
    }
    Write-Output "Installed server: $server"
    Write-Output 'Reload opted-in OpenCode locations to load the updated plugin. Existing data/login are preserved.'
} finally {
    try {
        if ($restart -and (Test-Path $server)) {
            # Keep a detached daemon from inheriting the installer's captured output pipe.
            $info = New-Object System.Diagnostics.ProcessStartInfo
            $info.FileName = $server
            $info.Arguments = 'connect'
            $info.UseShellExecute = $false
            $info.CreateNoWindow = $true
            $info.RedirectStandardOutput = $true
            $info.RedirectStandardError = $true
            $process = [System.Diagnostics.Process]::Start($info)
            $stdout = $process.StandardOutput.ReadToEndAsync()
            $stderr = $process.StandardError.ReadToEndAsync()
            $process.WaitForExit()
            if ($process.ExitCode -ne 0) { throw 'Installed server could not restart; inspect service.log' }
            $process.Dispose()
        }
    } finally {
        $lock.Dispose()
        if (Test-Path $stage) { Remove-Item $stage -Recurse -Force }
    }
}
