using System.Reflection;
using MegaCrit.Sts2.Core.Entities.Creatures;
using MegaCrit.Sts2.Core.Entities.Players;
using MegaCrit.Sts2.Core.Random;
using MegaCrit.Sts2.Core.Rewards;
using MegaCrit.Sts2.Core.Rooms;
using MegaCrit.Sts2.Core.Runs;
using MegaCrit.Sts2.Core.Entities.Merchant;

namespace HumanPlayRecorder;

// Explicit allowlisted projection. Never serialize game/save/network objects.
public static class ReplayCheckpoints
{
    static SortedDictionary<string,RngCheckpoint> Rngs(object set)
    {
        Ai.FrameCaptureBudget.Check();
        var result=new SortedDictionary<string,RngCheckpoint>(StringComparer.Ordinal);
        foreach(var p in set.GetType().GetProperties().Where(p=>p.PropertyType==typeof(Rng)))
        {
            Ai.FrameCaptureBudget.Check();
            var s=((Rng)p.GetValue(set)!).ToSerializable();
            result.Add(p.Name,new(checked((uint)s.counter),[s.state0,s.state1,s.state2,s.state3]));
        }
        return result;
    }
    static CreatureCheckpoint Creature(Creature c,string id)
    {
        Ai.FrameCaptureBudget.Check();
        return new(id,c.CurrentHp,c.MaxHp,c.Block,c.Powers.Select(p=>{
            Ai.FrameCaptureBudget.Check();return new PowerCheckpoint(p.Id.Entry,p.Amount);
        }).ToArray(),c.Monster is {} monster?Rng(monster.Rng):null);
    }
    static RngCheckpoint Rng(Rng rng){Ai.FrameCaptureBudget.Check();var s=rng.ToSerializable();return new(checked((uint)s.counter),[s.state0,s.state1,s.state2,s.state3]);}
    static SortedDictionary<string,decimal> Vars(MegaCrit.Sts2.Core.Localization.DynamicVars.DynamicVarSet vars)=>
        new(vars.ToDictionary(p=>{Ai.FrameCaptureBudget.Check();return p.Key;},p=>p.Value.BaseValue),StringComparer.Ordinal);
    static SortedDictionary<string,string[]> Bag(RelicGrabBag bag)=>new(
        bag.ToSerializable().RelicIdLists.ToDictionary(p=>p.Key.ToString(),p=>p.Value.Select(m=>m.Entry).ToArray()),StringComparer.Ordinal);
    public static ReplayCheckpoint Capture(RunState run,IEnumerable<Reward> rewards)
    {
        Ai.FrameCaptureBudget.Check();
        var combat=(run.CurrentRoom as CombatRoom)?.CombatState;
        return new("sts2-replay-checkpoint-v1",Rngs(run.Rng),
            run.Players.Select((p,i)=>{
                var c=p.PlayerCombatState;
                CardRef[] Cards(IEnumerable<MegaCrit.Sts2.Core.Models.CardModel>? cards)=>
                    cards?.Select(c=>{Ai.FrameCaptureBudget.Check();return new CardRef(c.Id.Entry,c.CurrentUpgradeLevel);}).ToArray()??[];
                return new CombatSeatCheckpoint(i,c?.Energy,c?.Stars,Cards(c?.Hand.Cards),Cards(c?.DrawPile.Cards),
                    Cards(c?.DiscardPile.Cards),Cards(c?.ExhaustPile.Cards),Creature(p.Creature,p.Character.Id.Entry),Rngs(p.PlayerRng),
                    p.Relics.Select(r=>new RelicCheckpoint(r.Id.Entry,r.Status.ToString(),r.StackCount,r.DisplayAmount,Vars(r.DynamicVars))).ToArray(),
                    p.PotionSlots.Select(p=>p is null?null:new PotionCheckpoint(p.Id.Entry,p.IsQueued,Vars(p.DynamicVars))).ToArray(),
                    p.PlayerOdds.CardRarity.CurrentValue,p.PlayerOdds.PotionReward.CurrentValue,Bag(p.RelicGrabBag));
            }).ToArray(),
            combat?.Enemies.Select(c=>Creature(c,c.Monster!.Id.Entry)).ToArray()??[],combat?.RoundNumber,
            run.CurrentRoom?.RoomType.ToString(),run.CurrentRoom?.ModelId?.Entry,
            run.CurrentMapPoint is {} map?[map.coord.col,map.coord.row]:null,
            run.CurrentMapPoint?.Children.OrderBy(p=>p.coord.col).Select(p=>new[]{p.coord.col,p.coord.row}).ToArray()??[],
            rewards.Select(r=>new OfferCheckpoint(r.GetType().Name,r.SuccessfullySelected,
                r is CardReward card?card.Cards.Select(c=>new CardRef(c.Id.Entry,c.CurrentUpgradeLevel)).ToArray():[],
                r is RelicReward relic?relic.Relic?.Id.Entry:r is PotionReward potion?potion.Potion?.Id.Entry:null,
                r is GoldReward gold?gold.Amount:null)).ToArray()) {
                    shared_relic_bag=Bag(run.SharedRelicGrabBag),
                    event_rng=run.CurrentRoom is EventRoom?Rng(RunManager.Instance.EventSynchronizer.GetLocalEvent().Rng):null,
                    event_options=run.CurrentRoom is EventRoom?RunManager.Instance.EventSynchronizer.GetLocalEvent().CurrentOptions
                        .Select(o=>new EventOptionCheckpoint(o.TextKey,!o.IsLocked,o.Relic?.Id.Entry)).ToArray():null,
                    shop=run.CurrentRoom is MerchantRoom?RunManager.Instance.MerchantSynchronizer.GetLocalInventory().AllEntries.Select(e=>new ShopCheckpoint(
                        e.GetType().Name,e switch {
                            MerchantCardEntry card=>card.CreationResult?.Card?.Id.Entry,
                            MerchantRelicEntry relic=>relic.Model?.Id.Entry,
                            MerchantPotionEntry potion=>potion.Model?.Id.Entry,_=>null
                        },e.Cost,e.IsStocked)).ToArray():null
                };
    }
}
