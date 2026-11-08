using System.Reflection;
using MegaCrit.Sts2.Core.Modding;
using MegaCrit.Sts2.Core.Models;

namespace HumanPlayRecorder;

public sealed record StartupOffendingMod(string Id, string Name);

// Start-up view of the same vanilla catalog the per-run gate enforces: which
// loaded mods register non-vanilla content that a base-character A10 run can
// draw. Content reachable only through another mod's Harmony patch is not
// visible here; remembered per-run exclusions supplement the registry scan.
public static class StartupContentScan
{
    // Vanilla pools that the beta game code never draws from at random (no
    // CardPool<T>/RelicPool<T>/PotionPool<T> generator reads them; their members
    // enter only when other content names the type, e.g. a card creating its
    // token). Membership there alone does not reach a base run; every other
    // vanilla pool (character pools, colorless, curse, shared relic/potion) does.
    static readonly HashSet<string> TypeOnlyPools = new(StringComparer.Ordinal) {
        "CARD_POOL.DEPRECATED_CARD_POOL", "CARD_POOL.EVENT_CARD_POOL", "CARD_POOL.QUEST_CARD_POOL",
        "CARD_POOL.STATUS_CARD_POOL", "CARD_POOL.TOKEN_CARD_POOL",
        "RELIC_POOL.DEPRECATED_RELIC_POOL", "RELIC_POOL.EVENT_RELIC_POOL", "RELIC_POOL.FALLBACK_RELIC_POOL",
        "POTION_POOL.DEPRECATED_POTION_POOL", "POTION_POOL.EVENT_POTION_POOL", "POTION_POOL.TOKEN_POTION_POOL"
    };

    public static IReadOnlyList<StartupOffendingMod> OffendingModsAtStartup()
    {
        var vanilla = VanillaContent.Load().models.Keys.ToHashSet(StringComparer.Ordinal);
        var mods = ModManager.GetLoadedMods().Where(m => m.manifest?.id is not null).ToArray();
        var owners = new Dictionary<Assembly, string>();
        foreach (var mod in mods)
            foreach (var assembly in mod.assemblies) owners.TryAdd(assembly, mod.manifest!.id!);
        var audited = mods.Where(m => AuditedCompatibleMods.Matches(m.manifest!.id!,
                AuditedCompatibleMods.ReadCandidateHash(m.manifest.id!, m.path, m.assemblies.Count)))
            .Select(m => m.manifest!.id!).ToHashSet(StringComparer.Ordinal);

        var scope = new Dictionary<AbstractModel, StartupContentScope>(ReferenceEqualityComparer.Instance);
        void Mark(IEnumerable<AbstractModel?> models, StartupContentScope s)
        {
            foreach (var m in models)
                if (m is not null && (!scope.TryGetValue(m, out var current) || current < s)) scope[m] = s;
        }
        void Guard(AbstractModel model, Action mark)
        {
            try { mark(); }
            catch (Exception e)
            {
                Godot.GD.Print($"[HumanPlayRecorder] start-up content scan skipped {model.Id}: {e.GetType().Name}");
            }
        }
        bool IsVanilla(AbstractModel m) => vanilla.Contains(m.Id.ToString());
        void MarkEncounters(IEnumerable<EncounterModel> encounters)
        {
            foreach (var encounter in encounters)
            {
                Mark([encounter], StartupContentScope.BaseRun);
                Mark(encounter.AllPossibleMonsters, StartupContentScope.BaseRun);
            }
        }

        var all = ModelDb.All.Where(VanillaContent.Relevant).ToArray();
        foreach (var model in all)
            Guard(model, () =>
            {
                switch (model)
                {
                    case CharacterModel c:
                        var s = IsVanilla(c) ? StartupContentScope.BaseRun : StartupContentScope.ModdedCharacter;
                        Mark([c, c.CardPool, c.RelicPool, c.PotionPool], s);
                        Mark(c.StartingDeck, s);
                        Mark(c.StartingRelics, s);
                        Mark(c.StartingPotions, s);
                        break;
                    case ActModel act when IsVanilla(act):
                        Mark(act.AllEvents, StartupContentScope.BaseRun);
                        Mark(act.AllAncients, StartupContentScope.BaseRun);
                        MarkEncounters(act.AllEncounters);
                        break;
                    case ActModel or ModifierModel:
                        Mark([model], StartupContentScope.BaseRun);
                        break;
                }
            });
        Mark(ModelDb.AllSharedEvents, StartupContentScope.BaseRun);
        Mark(ModelDb.AllSharedAncients, StartupContentScope.BaseRun);
        MarkEncounters(ModelDb.EventEncounters);
        // Pool members inherit their pool's scope; every vanilla pool is a base-run list.
        foreach (var pool in all.Where(m => m is CardPoolModel or RelicPoolModel or PotionPoolModel))
            Guard(pool, () =>
            {
                var s = scope.GetValueOrDefault(pool, StartupContentScope.Unreferenced);
                if (IsVanilla(pool) && !TypeOnlyPools.Contains(pool.Id.ToString())) s = StartupContentScope.BaseRun;
                if (s == StartupContentScope.Unreferenced) return;
                Mark(pool switch
                {
                    CardPoolModel cards => cards.AllCards,
                    RelicPoolModel relics => relics.AllRelics,
                    _ => ((PotionPoolModel)pool).AllPotions
                }, s);
            });

        var entries = scope.Select(p => new StartupContentEntry(p.Key.Id.ToString(),
            owners.GetValueOrDefault(p.Key.GetType().Assembly), p.Value)).ToArray();
        bool contained = audited.Contains("BaseCardPoolsOnly");
        string[] ids = StartupContentClassifier.OffendingModIds(entries, vanilla, audited.Contains, contained);
        foreach (string id in ids)
            Godot.GD.Print($"[HumanPlayRecorder] start-up content evidence: mod={id}; models=" + string.Join(",",
                entries.Where(e => e.OwnerModId == id && StartupContentClassifier.OffendingModIds([e], vanilla,
                    audited.Contains, contained).Length != 0).Select(e => e.ModelId).Order(StringComparer.Ordinal).Take(8)));
        ids = StartupConsent.IncludeRememberedMods(ids, mods.Select(m => m.manifest!.id!));
        return ids.Select(id => mods.First(m => m.manifest!.id == id))
            .Select(m => new StartupOffendingMod(m.manifest!.id!,
                string.IsNullOrWhiteSpace(m.manifest.name) ? m.manifest.id! : m.manifest.name!))
            .ToArray();
    }
}
