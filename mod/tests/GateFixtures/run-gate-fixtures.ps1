<#
No-network gate regression fixtures for the offline edition.
Builds the offline edition once per fixture with that fixture's probe compiled in
(-p:GateFixture=<name>, own output folders) and requires the gate to reject each build for
exactly that probe. The passing control is the plain offline Release build, whose own build
step runs the same gate (--expect-release); -Control <dll> also checks a built control here.
Needs the game reference assemblies: -GameData <folder with sts2.dll> (default: the build's).
#>
param(
    [string]$GameData,
    [string]$Control,
    [string]$SourceCommit = ('0' * 40)
)
$ErrorActionPreference = 'Stop'
$PSNativeCommandUseErrorActionPreference = $true
$mod = Resolve-Path (Join-Path $PSScriptRoot '..\..')
$project = Join-Path $mod 'HumanPlayRecorder.csproj'
$check = @('run', '--project', (Join-Path $mod 'tests/PackagingCheck/PackagingCheck.csproj'), '-c', 'Release', '--')
$build = @('-c', 'Release', '-nologo', '-v:q', '-p:Edition=Offline', "-p:RecorderSourceCommit=$SourceCommit")
if ($GameData) {
    $build += "-p:GameData=$GameData"
    $env:STS2_GAME_DATA = $GameData
}

$fixtures = [ordered]@{
    # The review's probe: load HumanPlayRecorder and invoke ReplayLinks.Open through reflection.
    'ReplayLinksProbe' = @('HumanPlayRecorder.GateFixtures.ReplayLinksProbe::Run',
        'System.Reflection.Assembly::Load', 'System.Reflection.Assembly::GetType', 'System.Reflection.MethodBase::Invoke')
    # A game multiplayer send from a method that is not a reviewed call site.
    'UnlistedSend' = @('HumanPlayRecorder.GateFixtures.UnlistedSend::Run',
        'MegaCrit.Sts2.Core.Multiplayer.Game.INetGameService::SendMessage')
}
foreach ($name in $fixtures.Keys) {
    dotnet build $project @build "-p:GateFixture=$name"
    $dll = Join-Path $mod "bin/GateFixture/$name/net9.0/PqpAiGuideOffline.dll"
    dotnet @check --gate-fixture $dll @($fixtures[$name])
}
if ($Control) {
    dotnet @check --network-gate $Control
}
Write-Host 'PASS: gate fixtures'
