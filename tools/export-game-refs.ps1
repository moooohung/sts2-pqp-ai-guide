#Requires -Version 7.4
<#
.SYNOPSIS
Copies the game assemblies the offline build compiles against from an installed Slay the Spire 2
into a folder laid out for the private moooohung/sts2-game-refs repository, and writes
game-refs.json (game version and SHA-256 of every file).

.DESCRIPTION
The layout matches the game folder, so the workflow builds with -p:Sts2GameDir=<checkout>:

  data_sts2_windows_x86_64/sts2.dll
  data_sts2_windows_x86_64/0Harmony.dll
  data_sts2_windows_x86_64/GodotSharp.dll
  data_sts2_windows_x86_64/System.IO.Hashing.dll
  game-refs.json

These four are exactly the <Reference> items of mod/HumanPlayRecorder.csproj. They are the game's
files and are not redistributed: they live only in the private repository.

.EXAMPLE
pwsh oss/tools/export-game-refs.ps1 -Output D:\Download\worktrees\sts2-game-refs
#>
param(
    [string]$GameDir = 'G:\SteamLibrary\steamapps\common\Slay the Spire 2',
    [Parameter(Mandatory)][string]$Output
)
$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest

$Files = @('sts2.dll', '0Harmony.dll', 'GodotSharp.dll', 'System.IO.Hashing.dll')
$data = 'data_sts2_windows_x86_64'
$source = Join-Path $GameDir $data
$target = Join-Path $Output $data
New-Item -ItemType Directory -Force $target | Out-Null

$entries = foreach ($name in $Files) {
    $from = Join-Path $source $name
    $to = Join-Path $target $name
    Copy-Item -LiteralPath $from -Destination $to -Force
    $item = Get-Item -LiteralPath $to
    [ordered]@{
        path = "$data/$name"
        bytes = $item.Length
        sha256 = (Get-FileHash -LiteralPath $to -Algorithm SHA256).Hash.ToLowerInvariant()
        file_version = $item.VersionInfo.FileVersion
    }
}
$stray = Get-ChildItem -LiteralPath $target -File | Where-Object Name -notin $Files
if ($stray) { throw "unexpected files in ${target}: $($stray.Name -join ', ')" }

$game = $entries | Where-Object { $_.path -eq "$data/sts2.dll" }
$manifest = [ordered]@{
    schema = 'sts2-game-refs-v1'
    game_version = (Get-Item -LiteralPath (Join-Path $source 'sts2.dll')).VersionInfo.ProductVersion
    # The mod's ReleaseBootstrap.SupportedGameHash and the policy's game_version ("public-beta+<first 8>") come from this.
    game_dll_sha256 = $game.sha256
    exported_from = 'Slay the Spire 2 (Steam, Windows x64)'
    files = @($entries)
}
$json = ($manifest | ConvertTo-Json -Depth 4) -replace "`r`n", "`n"
[IO.File]::WriteAllText((Join-Path $Output 'game-refs.json'), $json + "`n")
Write-Host "game refs: $($Files.Count) files, sts2.dll $($game.sha256) -> $Output"
