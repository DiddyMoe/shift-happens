# Produces an internal release candidate. AGENTS.md forbids all tests and
# fixture/harness generation. Direct Windows qualification remains required;
# build/audit success must never be described as installed qualification.
[CmdletBinding()]
param(
    # Optional external Tauri JSON overlay containing genuine signing settings.
    # Secrets belong in the signing provider's environment/certificate store.
    [string] $SigningConfig,
    # An explicit reviewed source snapshot is supported when Git metadata is
    # unavailable. This proves exact input bytes, not commit/repository history.
    [string] $SourceManifest,
    # Capture only; review the resulting external inventory before passing it
    # back as SourceManifest. No build or qualification is performed here.
    [string] $CaptureSourceManifest
)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
$script:FailureExitCode = 1

function Invoke-Native([string] $Program, [string[]] $Arguments) {
    & $Program @Arguments
    $code = $LASTEXITCODE
    if ($code -ne 0) {
        $script:FailureExitCode = $code
        throw "$Program failed with exit code $code. Release stopped."
    }
}

function Get-SourceInventory([string] $Directory) {
    $entries = New-Object 'System.Collections.Generic.SortedDictionary[string,string]' ([StringComparer]::Ordinal)
    # Only these generated directories are excluded. Source files, lockfiles,
    # resources, documentation and any added source file remain in the inventory.
    $generated = @('.git', 'node_modules', 'dist', 'src-tauri/target', 'src-tauri/gen', 'exports', 'backups')
    function Read-SourceDirectory([string] $Current, [string] $Relative) {
        foreach ($entry in Get-ChildItem -LiteralPath $Current -Force) {
            $path = if ($Relative) { "$Relative/$($entry.Name)" } else { $entry.Name }
            if ($path -eq '.git' -or ($entry.PSIsContainer -and $path -in $generated)) { continue }
            if (($entry.Attributes -band [IO.FileAttributes]::ReparsePoint) -ne 0) {
                throw "Source inventory requires ordinary files/directories, not links: $path"
            }
            if ($entry.PSIsContainer) { Read-SourceDirectory $entry.FullName $path }
            else { $entries.Add($path, (Get-FileHash -LiteralPath $entry.FullName -Algorithm SHA256).Hash.ToLowerInvariant()) }
        }
    }
    Read-SourceDirectory $Directory ''
    return ,$entries
}

function Resolve-ExternalPath([string] $Path, [string] $Directory) {
    $full = [IO.Path]::GetFullPath($Path)
    if ($full.Equals($Directory, [StringComparison]::OrdinalIgnoreCase) -or
        $full.StartsWith($Directory + [IO.Path]::DirectorySeparatorChar, [StringComparison]::OrdinalIgnoreCase)) {
        throw 'Keep source inventory files outside the source directory.'
    }
    return $full
}

function Read-SourceManifest([byte[]] $Bytes) {
    $snapshot = [Text.Encoding]::UTF8.GetString($Bytes) | ConvertFrom-Json
    if ($snapshot.format -ne 1 -or $snapshot.application -ne 'Shift Happens' -or $snapshot.files.Count -eq 0) {
        throw 'The source inventory has an unsupported format or no files.'
    }
    $entries = New-Object 'System.Collections.Generic.SortedDictionary[string,string]' ([StringComparer]::Ordinal)
    foreach ($file in $snapshot.files) {
        if ($file.path -isnot [string] -or $file.sha256 -isnot [string] -or
            $file.path -match '(^/|\\|(^|/)\.\.?(/|$)|[\x00-\x1f\x7f])' -or
            $file.sha256 -cnotmatch '^[0-9a-f]{64}$') {
            throw 'The source inventory contains a malformed path or SHA-256.'
        }
        $entries.Add($file.path, $file.sha256)
    }
    return ,$entries
}

function Confirm-SourceInventory($Expected, [string] $Directory) {
    $actual = Get-SourceInventory $Directory
    if ($actual.Count -ne $Expected.Count) { throw 'The source file set differs from the reviewed inventory.' }
    foreach ($path in $Expected.Keys) {
        if (-not $actual.ContainsKey($path) -or $actual[$path] -cne $Expected[$path]) {
            throw "A source file is missing or changed from the reviewed inventory: $path"
        }
    }
}

function Confirm-SourceState($Expected, [string] $Directory, $Identity, [string] $InventoryPath) {
    Confirm-SourceInventory $Expected $Directory
    if ($InventoryPath) {
        if ((Get-FileHash -LiteralPath $InventoryPath -Algorithm SHA256).Hash.ToLowerInvariant() -cne $Identity.manifest_sha256) {
            throw 'The reviewed source inventory changed while building the candidate.'
        }
    } else {
        $after = @(Invoke-Native 'git.exe' @('status', '--porcelain=v1', '--untracked-files=all'))
        if ($after.Count -ne 0) { throw 'Release gates changed the clean Git source state.' }
        $afterCommit = @(Invoke-Native 'git.exe' @('rev-parse', '--verify', 'HEAD'))
        if ($afterCommit.Count -ne 1 -or $afterCommit[0] -cne $Identity.commit) { throw 'The Git commit changed while building the candidate.' }
    }
}

function Resolve-Archiver {
    $command = Get-Command '7z.exe' -ErrorAction SilentlyContinue
    if ($command) { return $command.Source }
    foreach ($directory in @($env:ProgramFiles, ${env:ProgramFiles(x86)})) {
        if ($directory) {
            $installed = Join-Path $directory '7-Zip/7z.exe'
            if (Test-Path -LiteralPath $installed -PathType Leaf) { return $installed }
        }
    }
    throw 'Install the full 7-Zip CLI (7z.exe with its 7z.dll), or add its directory to PATH. Reduced 7za/7zr builds cannot inspect NSIS payloads.'
}

function Read-Engine([string] $Executable) {
    $process = New-Object System.Diagnostics.Process
    $process.StartInfo.FileName = $Executable
    $process.StartInfo.Arguments = '--sqlite-engine-info'
    $process.StartInfo.UseShellExecute = $false
    $process.StartInfo.RedirectStandardOutput = $true
    $process.StartInfo.RedirectStandardError = $true
    try {
        [void] $process.Start()
        $output = $process.StandardOutput.ReadToEnd()
        $errorOutput = $process.StandardError.ReadToEnd()
        $process.WaitForExit()
        if ($process.ExitCode -ne 0) {
            $script:FailureExitCode = $process.ExitCode
            throw "The release executable rejected its SQLite runtime: $errorOutput"
        }
        if ([string]::IsNullOrWhiteSpace($output)) {
            throw 'No engine inventory was returned. Close every running Shift Happens instance and rerun.'
        }
        return $output | ConvertFrom-Json
    } finally {
        $process.Dispose()
    }
}

$root = Split-Path -Parent $PSScriptRoot
Push-Location $root
try {
    if ($env:OS -ne 'Windows_NT') { throw 'Run this release path on Windows with the MSVC build tools.' }
    if ($SourceManifest -and $CaptureSourceManifest) { throw 'Capture a source inventory separately from building its candidate.' }
    if ($CaptureSourceManifest) {
        $capturePath = Resolve-ExternalPath $CaptureSourceManifest $root
        if (Test-Path -LiteralPath $capturePath) { throw 'Choose an absent external source-inventory path; existing inventories are never overwritten.' }
        $captured = Get-SourceInventory $root
        $files = @($captured.GetEnumerator() | ForEach-Object { [ordered]@{ path = $_.Key; sha256 = $_.Value } })
        $json = [ordered]@{ format = 1; application = 'Shift Happens'; files = $files } | ConvertTo-Json -Depth 4
        $stream = New-Object IO.FileStream($capturePath, [IO.FileMode]::CreateNew, [IO.FileAccess]::Write, [IO.FileShare]::None)
        try {
            $bytes = (New-Object Text.UTF8Encoding($false)).GetBytes($json + "`n")
            $stream.Write($bytes, 0, $bytes.Length)
            $stream.Flush($true)
        } finally { $stream.Dispose() }
        Write-Host "Captured source inventory: $capturePath"
        Write-Host 'Review these exact source inputs before building with -SourceManifest. This inventory provides no Git history and performs no build or Windows qualification.'
        return
    }
    $sourceManifestPath = $null
    if ($SourceManifest) {
        $sourceManifestPath = Resolve-ExternalPath $SourceManifest $root
        $sourceManifestBytes = [IO.File]::ReadAllBytes($sourceManifestPath)
        $expectedSource = Read-SourceManifest $sourceManifestBytes
        Confirm-SourceInventory $expectedSource $root
        $sha256 = [Security.Cryptography.SHA256]::Create()
        try { $manifestHash = [BitConverter]::ToString($sha256.ComputeHash($sourceManifestBytes)).Replace('-', '').ToLowerInvariant() }
        finally { $sha256.Dispose() }
        $sourceIdentity = [ordered]@{
            kind = 'sha256_source_inventory'
            manifest_sha256 = $manifestHash
            file_count = $expectedSource.Count
        }
        Write-Host "Reviewed source inventory: $($sourceIdentity.manifest_sha256) ($($sourceIdentity.file_count) files; no Git history claimed)"
    } else {
        if (-not (Get-Command 'git.exe' -ErrorAction SilentlyContinue)) { throw 'Install Git for Windows or supply an explicit reviewed external -SourceManifest.' }
        if (-not (Test-Path -LiteralPath (Join-Path $root '.git'))) { throw 'Git metadata is absent. Use a genuine clean checkout, or capture/review an external source inventory and supply -SourceManifest. No repository history will be invented.' }
        $commit = @(Invoke-Native 'git.exe' @('rev-parse', '--verify', 'HEAD'))
        if ($commit.Count -ne 1) { throw 'A clean Git release requires an existing commit.' }
        $source = @(Invoke-Native 'git.exe' @('status', '--porcelain=v1', '--untracked-files=all'))
        if ($source.Count -ne 0) { throw 'Commit the intended source and lockfile changes before building a release candidate.' }
        Invoke-Native 'git.exe' @('diff', '--exit-code')
        Invoke-Native 'git.exe' @('diff', '--cached', '--exit-code')
        $expectedSource = Get-SourceInventory $root
        $sourceIdentity = [ordered]@{ kind = 'git_commit'; commit = $commit[0]; file_count = $expectedSource.Count }
    }
    if ($root.StartsWith('\\', [StringComparison]::Ordinal)) {
        throw 'Native npm/Cargo release builds require a local Windows drive path. Copy the reviewed source to a local directory and reuse the same source inventory; UNC/WSL paths cannot be npm.cmd working directories.'
    }
    $manifest = Get-Content -Raw 'package.json' | ConvertFrom-Json
    $config = Get-Content -Raw 'src-tauri/tauri.conf.json' | ConvertFrom-Json
    # The full 7-Zip CLI supports NSIS. Reduced 7za/7zr builds do not.
    $archiver = Resolve-Archiver
    $missing = @(@('node.exe', 'npm.cmd', 'cargo.exe', 'rustc.exe', 'rustup.exe') | Where-Object { -not (Get-Command $_ -ErrorAction SilentlyContinue) })
    if ($missing.Count -ne 0) { throw "Missing Windows release tools: $($missing -join ', '). Install supported Node/npm and the x86_64-pc-windows-msvc Rust toolchain, or add their directories to this process PATH." }
    $nodeVersion = @(Invoke-Native 'node.exe' @('--version'))
    if ([version] $nodeVersion[0].TrimStart('v') -lt [version] $manifest.engines.node.TrimStart('>=') ) { throw "Node does not satisfy package.json engines.node ($($manifest.engines.node))." }
    $npmVersion = @(Invoke-Native 'npm.cmd' @('--version'))
    if ("npm@$($npmVersion[0])" -cne $manifest.packageManager) { throw "Use the exact package manager declared in package.json: $($manifest.packageManager)." }
    $rustcVersion = @(Invoke-Native 'rustc.exe' @('--version'))
    $rustcDetails = @(Invoke-Native 'rustc.exe' @('-vV'))
    if ('host: x86_64-pc-windows-msvc' -cnotin $rustcDetails) {
        throw 'Use the native x86_64-pc-windows-msvc Rust host toolchain for this Windows release workflow.'
    }
    $cargoVersion = @(Invoke-Native 'cargo.exe' @('--version'))
    Invoke-Native 'cargo.exe' @('fmt', '--version')
    Invoke-Native 'cargo.exe' @('clippy', '--version')
    $advisoryTool = @(Invoke-Native 'cargo.exe' @('deny', '--version'))
    if ($advisoryTool[0] -cne 'cargo-deny 0.20.2') {
        throw 'Install the reviewed advisory tool with cargo install --locked --version 0.20.2 --bin cargo-deny cargo-deny.'
    }
    $sourceIdentity['tools'] = [ordered]@{
        node = $nodeVersion[0]
        package_manager = $manifest.packageManager
        rustc = $rustcVersion[0]
        cargo = $cargoVersion[0]
        cargo_deny = $advisoryTool[0]
        rust_host = 'x86_64-pc-windows-msvc'
    }
    $version = $manifest.version
    $signingPath = $null
    if ($SigningConfig) {
        $signingPath = (Resolve-Path -LiteralPath $SigningConfig).Path
        if ($signingPath.StartsWith($root + [IO.Path]::DirectorySeparatorChar, [StringComparison]::OrdinalIgnoreCase)) {
            throw 'Keep the signing overlay outside the repository.'
        }
        $overlay = Get-Content -Raw -LiteralPath $signingPath | ConvertFrom-Json
        $topKeys = @($overlay.PSObject.Properties.Name)
        if ($topKeys.Count -ne 1 -or $topKeys[0] -ne 'bundle') {
            throw 'The signing overlay must contain only bundle.publisher and bundle.windows signing fields.'
        }
        foreach ($key in $overlay.bundle.PSObject.Properties.Name) {
            if ($key -notin @('publisher', 'windows')) {
                throw 'The signing overlay must not override installer, application, build or security configuration.'
            }
        }
        foreach ($key in $overlay.bundle.windows.PSObject.Properties.Name) {
            if ($key -notin @('signCommand', 'certificateThumbprint', 'digestAlgorithm', 'timestampUrl', 'tsp')) {
                throw 'The Windows signing overlay contains a field outside the supported signing contract.'
            }
        }
    }
    $target = 'x86_64-pc-windows-msvc'
    # One supported installed feature set: Cargo defaults (no app features),
    # tauri/custom-protocol and the export-library features in Cargo.toml.
    # Debug desktop development is the other supported build combination.
    $cargo = @('--manifest-path', 'src-tauri/Cargo.toml', '--locked', '--target', $target)
    $release = @('--release', '--features', 'tauri/custom-protocol', '--bins', '--lib')
    if ($env:CARGO_TARGET_DIR) { throw 'Unset CARGO_TARGET_DIR so artifact paths are unambiguous.' }
    # The manager confirmed the calendar/checkmark assets are project-owned;
    # their source/ownership and bundled font/template licenses are in README.

    # Resource reads fail for missing/invalid configured assets. The native
    # Tauri compiler/bundler validates the effective config, ACL and resources.
    Add-Type -AssemblyName System.Drawing
    foreach ($path in $config.bundle.icon) {
        $resource = Join-Path 'src-tauri' $path
        $file = Get-Item -LiteralPath $resource
        if ($file.Length -eq 0) { throw "Empty configured icon: $path" }
        if ($file.Extension -eq '.ico') {
            $icon = New-Object System.Drawing.Icon($file.FullName)
            $icon.Dispose()
        } else {
            $image = [System.Drawing.Image]::FromFile($file.FullName)
            try {
                if ($image.Width -ne $image.Height -or $image.Width -lt 256) {
                    throw "Configured raster icon must be square and at least 256 pixels: $path"
                }
            } finally { $image.Dispose() }
        }
    }
    foreach ($path in $config.bundle.resources) { [void] (Get-Item -LiteralPath (Join-Path 'src-tauri' $path)) }
    [void] (Get-Item 'src-tauri/assets/fonts/NotoSans-Regular.ttf')
    [void] (Get-Item 'src-tauri/installer/LICENSE-MIT')
    [void] (Get-Item 'src-tauri/installer/LICENSE-APACHE')

    Invoke-Native 'npm.cmd' @('ci', '--ignore-scripts')
    Invoke-Native 'cargo' (@('fetch') + $cargo)
    Invoke-Native 'npm.cmd' @('run', 'format:check')
    Invoke-Native 'npm.cmd' @('run', 'typecheck')
    Invoke-Native 'npm.cmd' @('run', 'lint')
    Invoke-Native 'npm.cmd' @('run', 'rust:fmt')
    Invoke-Native 'cargo' (@('clippy') + $cargo + @('--bins', '--lib', '--', '-D', 'warnings'))
    # A clean source directory has no generated dist. The custom-protocol
    # context used by release Clippy embeds those production frontend assets.
    Invoke-Native 'npm.cmd' @('run', 'build')
    Invoke-Native 'cargo' (@('clippy') + $cargo + $release + @('--', '-D', 'warnings'))
    Invoke-Native 'cargo' (@('build') + $cargo + @('--bins', '--lib'))
    Invoke-Native 'cargo' (@('tree') + $cargo + @('--features', 'tauri/custom-protocol', '-i', 'libsqlite3-sys'))
    foreach ($library in @('rust_xlsxwriter', 'calamine', 'pdf-writer', 'lopdf', 'skrifa')) {
        Invoke-Native 'cargo' (@('tree') + $cargo + @('--features', 'tauri/custom-protocol', '-i', $library))
    }
    # Audit the exact Windows release graph, including its build dependencies.
    # All advisory categories and warnings are denied, with no ignored
    # advisories/packages. Missing tools, unavailable fresh advisory data, or
    # findings stop the release. The graph/strict policy lives in deny.toml.
    Invoke-Native 'cargo' @('deny', '--manifest-path', 'src-tauri/Cargo.toml', '--locked', '--config', 'src-tauri/deny.toml', '--target', $target, '--features', 'tauri/custom-protocol', '--log-level', 'info', 'check', '--deny', 'warnings', 'advisories')
    Invoke-Native 'npm.cmd' @('audit', '--audit-level=high')
    $installerArgs = @('run', 'tauri:installer', '--', '--target', $target)
    if ($SigningConfig) {
        $installerArgs += @('--config', $signingPath)
    } else { $installerArgs += '--no-sign' }
    # Tauri reuses beforeBuildCommand = npm run build and adds custom-protocol.
    # Runner args enforce the already reviewed Cargo lockfile.
    Invoke-Native 'npm.cmd' ($installerArgs + @('--', '--locked'))
    Confirm-SourceState $expectedSource $root $sourceIdentity $sourceManifestPath

    $outputDirectory = Join-Path $root "src-tauri/target/$target/release"
    $buildExecutable = Join-Path $outputDirectory 'shift-happens.exe'
    $installer = Join-Path $outputDirectory "bundle/nsis/Shift Happens_${version}_x64-setup.exe"
    [void] (Get-Item -LiteralPath $installer)
    # Tauri stamps/signs the NSIS payload, then restores the original unsigned
    # build executable. Inspect the actual shipped executable without running
    # an installer or changing app data, shortcuts or installation registration.
    $payloadDirectory = Join-Path $outputDirectory 'payload'
    [void] [IO.Directory]::CreateDirectory($payloadDirectory)
    $executable = Join-Path $payloadDirectory 'shift-happens.exe'
    # Remove only the previous generated executable so an absent archive entry
    # cannot accidentally qualify a stale candidate.
    if (Test-Path -LiteralPath $executable) { Remove-Item -LiteralPath $executable }
    Invoke-Native $archiver @('e', $installer, 'shift-happens.exe', "-o$payloadDirectory", '-aoa')
    [void] (Get-Item -LiteralPath $executable)
    foreach ($artifact in @($buildExecutable, $executable, $installer)) {
        $info = (Get-Item -LiteralPath $artifact).VersionInfo
        if ($info.ProductVersion -ne $version -or $info.ProductName -ne 'Shift Happens') {
            throw "Artifact identity/version mismatch: $artifact"
        }
    }
    $inventory = Read-Engine $executable
    if ($inventory.application_version -ne $version -or -not $inventory.sqlx_and_backup_match) {
        throw 'The compiled release identity or SQLite linkage is inconsistent.'
    }
    Write-Host "SQLx and backup SQLite: $($inventory.engine.version) / $($inventory.engine.source_id)"
    # The binary checks the WAL-reset safety gate through both real bindings.
    # Compare its compiled inventory to the exact source bytes SQLx embeds.
    $migrations = @(Get-ChildItem 'src-tauri/migrations/*.sql' | Sort-Object Name)
    if ($migrations.Count -ne $inventory.migrations.Count) { throw 'The compiled migration inventory differs from source.' }
    for ($i = 0; $i -lt $migrations.Count; $i++) {
        $migrationVersion = [long] ($migrations[$i].BaseName.Split('_')[0])
        $checksum = (Get-FileHash $migrations[$i].FullName -Algorithm SHA384).Hash.ToLowerInvariant()
        if ($migrationVersion -ne $inventory.migrations[$i].version -or $checksum -ne $inventory.migrations[$i].checksum) {
            throw "An embedded migration is stale or different: $($migrations[$i].Name)"
        }
    }

    # Inspect the effective, rendered template from this build; the native UI
    # must also be inspected during direct install/uninstall qualification.
    $rendered = Get-Content -Raw (Join-Path $outputDirectory 'nsis/x64/installer.nsi')
    if ($rendered -match 'DeleteAppDataCheckbox|RmDir\s+/r\s+"\$(APPDATA|LOCALAPPDATA)') {
        throw 'The effective uninstaller can delete app data.'
    }
    foreach ($artifact in @($executable, $installer)) {
        $signature = Get-AuthenticodeSignature -LiteralPath $artifact
        if ($SigningConfig) {
            if ($signature.Status -ne 'Valid' -or -not $signature.TimeStamperCertificate) {
                throw "A signed release requires a valid signature and timestamp: $artifact"
            }
        } elseif ($signature.Status -ne 'NotSigned') { throw "Unsigned internal build has unexpected signature status: $artifact" }
    }
    Confirm-SourceState $expectedSource $root $sourceIdentity $sourceManifestPath
    $hashes = foreach ($artifact in @($buildExecutable, $executable, $installer)) {
        $hash = Get-FileHash -LiteralPath $artifact -Algorithm SHA256
        $relative = $artifact.Substring($outputDirectory.Length + 1).Replace('\', '/')
        "$($hash.Hash.ToLowerInvariant())  $relative"
    }
    $hashes | Set-Content -Encoding ASCII (Join-Path $outputDirectory 'SHA256SUMS.txt')
    $sourceIdentity | ConvertTo-Json | Set-Content -Encoding UTF8 (Join-Path $outputDirectory 'SOURCE-IDENTITY.json')
    if ($sourceManifestPath) { [IO.File]::WriteAllBytes((Join-Path $outputDirectory 'SOURCE-INVENTORY.json'), $sourceManifestBytes) }
    $hashes | Write-Host
    Write-Host "Internal candidate: $installer"
    Write-Host 'Build gates completed. Direct Windows qualification is still required: fresh/empty/current/pending/blocked startup, preserved backup ordering and failures, committed WAL/hot journal behavior, initializer rollback, ACL isolation, manager finalization/export/backup, both install locations, second-instance focus, install-over, all uninstall paths, reinstall, shortcuts/icons and WebView2 installation.'
    Write-Host 'Tests and generated upgrade/classification fixtures were not run or created, as required by AGENTS.md. This script does not certify those outcomes or publish the candidate.'
    if (-not $SigningConfig) { Write-Host 'Unsigned internal candidate. Public distribution requires separately verified signing; signing does not guarantee immediate SmartScreen reputation.' }
} catch {
    [Console]::Error.WriteLine($_.Exception.Message)
    exit $script:FailureExitCode
} finally { Pop-Location }
