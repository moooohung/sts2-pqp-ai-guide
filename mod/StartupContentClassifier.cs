namespace HumanPlayRecorder;

// Where a registered model can surface during a base-character A10 run.
public enum StartupContentScope
{
    // Registered but referenced by no run-generation list (helper powers,
    // orbs, unpooled tokens): it can only enter through other content.
    Unreferenced,
    // Only a modded character's own character, pools or starting inventory.
    // Vanilla generators that read every character's pool (Prismatic Gem,
    // Kaleidoscope, Splash, Orobas) are kept to the base roster only by the
    // audited BaseCardPoolsOnly build.
    ModdedCharacter,
    // Vanilla card/relic/potion pools, base starting inventory, vanilla act
    // event/encounter lists, shared events, and every run modifier or act.
    BaseRun
}

public sealed record StartupContentEntry(string ModelId, string? OwnerModId, StartupContentScope Scope);

public static class StartupContentClassifier
{
    // Pure decision used at start-up. vanillaModelIds is the shipped vanilla
    // catalog's key set (vanilla-content.json), not a second list.
    public static string[] OffendingModIds(IEnumerable<StartupContentEntry> entries,
        IReadOnlySet<string> vanillaModelIds, Func<string, bool> auditedCompatible,
        bool characterContentContained) =>
        entries.Where(e => e.OwnerModId is { } owner && !auditedCompatible(owner) &&
                !vanillaModelIds.Contains(e.ModelId) &&
                (e.Scope == StartupContentScope.BaseRun ||
                 (e.Scope == StartupContentScope.ModdedCharacter && !characterContentContained)))
            .Select(e => e.OwnerModId!)
            .Distinct(StringComparer.Ordinal)
            .Order(StringComparer.Ordinal)
            .ToArray();
}
