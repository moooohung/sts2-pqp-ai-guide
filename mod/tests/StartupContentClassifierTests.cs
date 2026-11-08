using HumanPlayRecorder;

static class StartupContentClassifierTests
{
    public static void Run()
    {
        var vanilla = new HashSet<string>(StringComparer.Ordinal) { "CARD.BASH", "RELIC.VAJRA", "CHARACTER.IRONCLAD" };
        StartupContentEntry[] entries = [
            new("CARD.BASH", null, StartupContentScope.BaseRun),
            new("MODIFIER.HEXTECH_MAYHEM_MODIFIER", "HextechRunes", StartupContentScope.BaseRun),
            new("CARD.SHARED_POOL_CARD", "SharedCards", StartupContentScope.BaseRun),
            new("CHARACTER.WATCHER", "Watcher", StartupContentScope.ModdedCharacter),
            new("CARD.ERUPTION", "Watcher", StartupContentScope.ModdedCharacter),
            new("POWER.MANTRA", "Watcher", StartupContentScope.Unreferenced),
            new("ORB.HELPER", "LibraryMod", StartupContentScope.Unreferenced),
            new("RELIC.AUDITED_PROBE", "BaseCardPoolsOnly", StartupContentScope.BaseRun),
            new("RELIC.VAJRA", "SameIdMod", StartupContentScope.BaseRun),
            new("EVENT.UNATTRIBUTED", null, StartupContentScope.BaseRun),
        ];
        bool Audited(string id) => id == "BaseCardPoolsOnly";
        string[] contained = StartupContentClassifier.OffendingModIds(entries, vanilla, Audited, characterContentContained: true);
        if (!contained.SequenceEqual(["HextechRunes", "SharedCards"]))
            throw new Exception("start-up offenders with BaseCardPoolsOnly: " + string.Join(",", contained));
        string[] open = StartupContentClassifier.OffendingModIds(entries, vanilla, Audited, characterContentContained: false);
        if (!open.SequenceEqual(["HextechRunes", "SharedCards", "Watcher"]))
            throw new Exception("start-up offenders without BaseCardPoolsOnly: " + string.Join(",", open));
        Console.WriteLine("PASS: start-up classifier flags base-run modifiers/pool content, keeps audited, " +
            "unreferenced and BaseCardPoolsOnly-contained character content out");
    }
}
