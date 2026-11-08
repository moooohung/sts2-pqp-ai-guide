using HumanPlayRecorder;

static class RunStartDiagnosticsTests
{
    static Exception Known()
    {
        try { throw new InvalidOperationException("active mod set is not replay-supported"); }
        catch (Exception e) { return e; }
    }
    static Exception Private()
    {
        try { throw new IOException(@"C:\Users\PrivatePerson\76561198000000000\save.json"); }
        catch (Exception e) { return e; }
    }
    public static void Run()
    {
        string known = RunStartDiagnostics.Failure(Known());
        if (!known.Contains("message=active mod set is not replay-supported") ||
            !known.Contains("method=RunStartDiagnosticsTests.Known")) throw new Exception("missing safe diagnostic");
        string safe = RunStartDiagnostics.Failure(Private());
        if (safe.Contains("PrivatePerson") || safe.Contains("76561198000000000") || safe.Contains(@"C:\") || safe.Contains('\n'))
            throw new Exception("exception diagnostic leaked private data");
        if (!AuditedCompatibleMods.Matches("BaseCardPoolsOnly",AuditedCompatibleMods.BaseCardPoolsOnlySha256) ||
            AuditedCompatibleMods.Matches("BaseCardPoolsOnly",new string('0',64)) ||
            AuditedCompatibleMods.Matches("OtherMod", AuditedCompatibleMods.BaseCardPoolsOnlySha256))
            throw new Exception("audited compatibility must require both exact ID and SHA-256");
        Console.WriteLine("PASS: exact audited build identity, safe exception message + throwing method, private exception redaction");
        foreach (string ns in new[] { "Combat", "Commands", "Models", "Hooks", "Rooms", "Rewards", "Map", "Random", "Runs", "Saves", "Multiplayer.Game" })
            if (!GameplayPatchClassifier.IsGameplay(new("MegaCrit.Sts2.Core." + ns + ".Example", "Execute")))
                throw new Exception("gameplay namespace misclassified: " + ns);
        foreach (var method in new GameplayPatchEvidence[] {
            new("MegaCrit.Sts2.Core.Nodes.Combat.NCreature", "Animate"),
            new("MegaCrit.Sts2.Core.Audio.AudioManager", "Play"),
            new("MegaCrit.Sts2.Core.Commands.SfxCmd", "Play"),
            new("MegaCrit.Sts2.Core.Models.Cards.Bash", "get_PortraitPath", true),
            new("MegaCrit.Sts2.Core.Models.Cards.Bash", "get_Portrait", true, true) })
            if (GameplayPatchClassifier.IsGameplay(method)) throw new Exception("cosmetic target misclassified");
        if (!GameplayPatchClassifier.IsGameplay(new("OtherMod.Cards.CustomCard", "OnPlay", true)))
            throw new Exception("mod-owned model target must count as gameplay");
        Console.WriteLine("PASS: synthetic gameplay versus visual/audio/UI Harmony target classification");
        string root = Path.Combine(Path.GetTempPath(), "hpr-compatible-" + Guid.NewGuid().ToString("N"));
        Directory.CreateDirectory(root);
        File.WriteAllText(Path.Combine(root, "BaseCardPoolsOnly.config.json"),
            """{"allowed_mod_assemblies":["Watcher"]}""");
        if (AuditedCompatibleMods.ReadCandidateHash("BaseCardPoolsOnly", root, 1) is not null)
            throw new Exception("non-default mod-character whitelist cannot receive the audited base-only exception");
        Console.WriteLine("PASS: nonempty safety-guard character whitelist is not audited-compatible");
        var audit=new[]{new PatchAuditEntry("Visual","Game.Draw","postfix"),
            new PatchAuditEntry("Observer","Game.Damage","prefix")};
        var summary=RecordingFormat.AuditSummary(audit);
        if(summary.entries!=2||summary.prefix!=1||summary.postfix!=1||summary.transpiler!=0||summary.finalizer!=0||
            summary.sha256.Length!=64)throw new Exception("patch audit summary");
        if(RecordingFormat.AuditSummary([audit[0] with{method="Game.Other"},audit[1]]).sha256==summary.sha256)
            throw new Exception("patch summary not bound to exact methods");
        Console.WriteLine("PASS: audit patch kinds, counts and full-array hash binding");
    }
}
