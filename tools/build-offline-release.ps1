#Requires -Version 7.4
<#
.SYNOPSIS
Builds, gates and packages the offline edition from the public repository layout (mod/, engine/).
The release workflow runs exactly this script; run it locally to reproduce a release.

.DESCRIPTION
0. Requires the checkout to be exactly -SourceCommit with no changes; both binaries are stamped
   with that one public-repository commit, and release.json records it.
1. Checks the game reference assemblies against game-refs.json.
2. Builds the native bridge from engine/ (latest stable Rust, --locked) and rejects network crates.
3. Builds the offline mod (-p:Edition=Offline, Release). The build's own PackagingCheck step
   includes the no-network gate.
4. Runs the no-network gate on the mod, its regression fixtures, and the network-import gate on
   the bridge.
5. Downloads the current published policy (manifest, then weights by hash) and verifies SHA-256
   and metadata; any failure stops the release.
6. Assembles the mod folder with the AGPL LICENSE and policy/MODEL-LICENSE.md, checks the
   package, and zips the mods-folder layout (PqpAiGuideOffline/...).

.EXAMPLE
pwsh tools/build-offline-release.ps1 -Repo . -GameRefs ..\sts2-game-refs -SourceCommit (git rev-parse HEAD) -Version v0.1.0 -Output ..\out
#>
param(
    [Parameter(Mandatory)][string]$Repo,
    [Parameter(Mandatory)][string]$GameRefs,
    [Parameter(Mandatory)][ValidatePattern('^[0-9a-f]{40}$')][string]$SourceCommit,
    [Parameter(Mandatory)][string]$Version,
    [Parameter(Mandatory)][string]$Output,
    # Local runs may pass an already verified policy folder instead of downloading it.
    [string]$PolicyDir,
    # Release runs also refuse Writer placeholders (TODO_COPY) in shipped text.
    [switch]$RequireFinalText
)
$ErrorActionPreference = 'Stop'
$PSNativeCommandUseErrorActionPreference = $true
Set-StrictMode -Version Latest

$Repo = (Resolve-Path $Repo).Path
$GameRefs = (Resolve-Path $GameRefs).Path
# The release records one source: the public repository commit checked out here, unmodified.
# Both binaries are stamped with it (mod assembly metadata, bridge sts2_advisor_source_commit).
$head = (git -C $Repo rev-parse HEAD).Trim()
if ($head -ne $SourceCommit) { throw "checkout is at $head, not the release commit $SourceCommit" }
$dirty = git -C $Repo status --porcelain --untracked-files=all
if ($dirty) { throw "checkout has changes outside commit ${SourceCommit}:`n$($dirty -join "`n")" }
$sourceRepository = (git -C $Repo remote get-url origin).Trim()
New-Item -ItemType Directory -Force $Output | Out-Null
$Output = (Resolve-Path $Output).Path
$mod = Join-Path $Repo 'mod'
$engine = Join-Path $Repo 'engine'
$work = Join-Path $Output 'work'
if (Test-Path $work) { throw "fresh output directory required: $work exists" }
New-Item -ItemType Directory $work | Out-Null
$check = @('run', '--project', (Join-Path $mod 'tests/PackagingCheck/PackagingCheck.csproj'), '-c', 'Release', '--')

Write-Host '== game references'
$refs = Get-Content -Raw (Join-Path $GameRefs 'game-refs.json') | ConvertFrom-Json
if ($refs.schema -ne 'sts2-game-refs-v1') { throw 'unsupported game-refs.json' }
foreach ($file in $refs.files) {
    $hash = (Get-FileHash -LiteralPath (Join-Path $GameRefs $file.path) -Algorithm SHA256).Hash.ToLowerInvariant()
    if ($hash -ne $file.sha256) { throw "game reference $($file.path) does not match game-refs.json" }
}
$gameHash = $refs.game_dll_sha256
Write-Host "game sts2.dll $gameHash ($($refs.game_version))"

Write-Host '== native bridge (engine/)'
rustup toolchain install stable --profile minimal --no-self-update
Push-Location $engine
try {
    # Any HTTP, socket or TLS crate in the bridge's dependency graph fails the release.
    $crates = cargo +stable tree --locked -p sts2_advisor_bridge -e normal,build --prefix none --format '{p}' |
        ForEach-Object { ($_ -split ' ')[0] } | Sort-Object -Unique
    $network = 'reqwest', 'hyper', 'h2', 'h3', 'ureq', 'curl', 'curl-sys', 'isahc', 'surf', 'attohttpc', 'tokio', 'mio',
        'socket2', 'native-tls', 'rustls', 'openssl', 'tungstenite', 'tokio-tungstenite', 'quinn', 'websocket', 'http'
    $found = @($crates | Where-Object { $_ -in $network })
    if ($found.Count) { throw "native bridge depends on network crates: $($found -join ', ')" }
    Write-Host "PASS: native bridge dependency graph ($($crates.Count) crates) has no network crate"
    $env:STS2_ADVISOR_SOURCE_COMMIT = $SourceCommit
    # Reproducible bridge: the release must match a local build of the same commit byte for byte.
    # Build paths are remapped, the linker is the rust-lld shipped with the toolchain (not the
    # runner's MSVC link.exe), the PE timestamp is a content hash and the PDB path a bare file name.
    $cargoHome = if ($env:CARGO_HOME) { $env:CARGO_HOME } else { Join-Path $HOME '.cargo' }
    $env:CARGO_ENCODED_RUSTFLAGS = @(
        "--remap-path-prefix=$engine=/engine", "--remap-path-prefix=$cargoHome=/cargo",
        '-Clink-arg=/Brepro', '-Clink-arg=/PDBALTPATH:%_PDB%'
    ) -join [char]0x1f
    $env:CARGO_TARGET_X86_64_PC_WINDOWS_MSVC_LINKER = 'rust-lld.exe'
    cargo +stable build --release --locked -p sts2_advisor_bridge
    $rustc = (rustc +stable -V).Trim()
} finally { Pop-Location }
$bridge = Join-Path $engine 'target/release/sts2_advisor_bridge.dll'

Write-Host '== offline mod (mod/, -p:Edition=Offline)'
# ContinuousIntegrationBuild maps source and PDB paths to /_/, so the DLL does not depend on
# where the repository is checked out.
dotnet build (Join-Path $mod 'HumanPlayRecorder.csproj') -c Release -p:Edition=Offline `
    "-p:RecorderSourceCommit=$SourceCommit" "-p:Sts2GameDir=$GameRefs" -p:ContinuousIntegrationBuild=true
$dotnetSdk = (dotnet --version).Trim()
$dll = Join-Path $mod 'bin/Offline/Release/net9.0/PqpAiGuideOffline.dll'
$env:STS2_GAME_DATA = Join-Path $GameRefs 'data_sts2_windows_x86_64'
dotnet @check --network-gate $dll
dotnet @check --native-network-gate $bridge
# The gate must still reject the regression fixtures (reflection bypass, unlisted game send).
& (Join-Path $mod 'tests/GateFixtures/run-gate-fixtures.ps1') -GameData $env:STS2_GAME_DATA -SourceCommit $SourceCommit -Control $dll

Write-Host '== policy'
if ($PolicyDir) {
    $policy = (Resolve-Path $PolicyDir).Path
} else {
    $policy = Join-Path $work 'policy'
    New-Item -ItemType Directory $policy | Out-Null
    $origin = 'https://sts.pqp.me'
    Invoke-WebRequest -UseBasicParsing -MaximumRedirection 0 -OutFile (Join-Path $policy 'current.json') "$origin/api/aiplay/policy/latest"
    $hash = (Get-Content -Raw (Join-Path $policy 'current.json') | ConvertFrom-Json).weights_sha256
    if ($hash -notmatch '^[0-9a-f]{64}$') { throw 'published policy manifest has no valid weights_sha256' }
    Invoke-WebRequest -UseBasicParsing -MaximumRedirection 0 -OutFile (Join-Path $policy "$hash.safetensors") "$origin/api/aiplay/policy/weights/$hash.safetensors"
}
# SHA-256 against current.json plus the same metadata checks the online mod applies, judged for the
# built mod: its game pin and sts2.dll reference stamp must equal the refs, and a checkpoint labelled
# for another game build is accepted only when the mod ships with the guide switched off (inert).
dotnet @check --policy-gate $dll $gameHash $policy

Write-Host '== package'
$package = Join-Path $work 'package'
New-Item -ItemType Directory $package | Out-Null
$editionFiles = Join-Path $mod 'Editions/Offline'
$license = Join-Path $Repo 'LICENSE'
if ((Get-FileHash $license).Hash -ne (Get-FileHash (Join-Path $mod 'LICENSE')).Hash) { throw 'mod/LICENSE differs from the repository LICENSE' }
dotnet @check --assemble-offline $package $dll $bridge $SourceCommit $SourceCommit $policy `
    (Join-Path $editionFiles 'MODEL-LICENSE.md') (Join-Path $editionFiles 'PqpAiGuideOffline.json') $gameHash $license
dotnet @check --package-offline $package $SourceCommit $SourceCommit
if ($RequireFinalText) { dotnet @check --final-text $package }

$name = "pqp-ai-guide-offline-$Version"
$zip = Join-Path $Output "$name.zip"
# Unzip into the game's mods folder: mods/PqpAiGuideOffline/... Stored entries in ordinal order,
# stamped with the commit time, so the archive bytes do not depend on the machine or its zlib.
$commitTime = (git -C $Repo show -s --format=%cI $SourceCommit).Trim()
dotnet @check --zip-folder (Join-Path $package 'mods/PqpAiGuideOffline') $zip $commitTime
$zipHash = (Get-FileHash -LiteralPath $zip -Algorithm SHA256).Hash.ToLowerInvariant()
$dllHash = (Get-FileHash -LiteralPath $dll -Algorithm SHA256).Hash.ToLowerInvariant()
$bridgeHash = (Get-FileHash -LiteralPath $bridge -Algorithm SHA256).Hash.ToLowerInvariant()
$model = Get-Content -Raw (Join-Path $policy 'current.json') | ConvertFrom-Json
[IO.File]::WriteAllText("$zip.sha256", "$zipHash  $name.zip`n")
$summary = [ordered]@{
    version = $Version; source_repository = $sourceRepository; source_commit = $SourceCommit
    zip = "$name.zip"; zip_sha256 = $zipHash
    mod_dll_sha256 = $dllHash; bridge_sha256 = $bridgeHash; game_dll_sha256 = $gameHash
    checkpoint_id = $model.checkpoint_id; step = $model.step; weights_sha256 = $model.weights_sha256
    rustc = $rustc; dotnet_sdk = $dotnetSdk
    files = [ordered]@{}
}
foreach ($file in Get-ChildItem -LiteralPath (Join-Path $package 'mods/PqpAiGuideOffline') -Recurse -File | Sort-Object FullName) {
    $relative = [IO.Path]::GetRelativePath((Join-Path $package 'mods'), $file.FullName).Replace('\', '/')
    $summary.files[$relative] = (Get-FileHash -LiteralPath $file.FullName -Algorithm SHA256).Hash.ToLowerInvariant()
}
($summary | ConvertTo-Json) | Set-Content (Join-Path $Output 'release.json')
$summary.GetEnumerator() | Where-Object Key -ne 'files' | ForEach-Object { Write-Host ("{0}: {1}" -f $_.Key, $_.Value) }
$summary.files.GetEnumerator() | ForEach-Object { Write-Host ("  {0}  {1}" -f $_.Value, $_.Key) }
