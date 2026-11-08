//! Frozen structured_env.py:838-1047 semantic-progress key.
//! Excludes phase/modal identity, RNG, instance allocation and presentation.
use sts2_sim::{card::Card, run::{RunManager, RunPhase}};

fn card(c: &Card, costs: Option<(i32,i32)>) -> String {
    let mut vars: Vec<_> = c.vars.iter().map(|(k,v)| (k.name(),*v)).collect();
    vars.sort_unstable();
    let mut ench: Vec<_> = c.enchantments.iter().copied().collect();
    ench.sort_unstable();
    // Frozen CardInstance has no upgrade_level attribute: getattr(...,0),
    // not an invented 1 for upgraded cards. Wither's fake level is an effect var.
    let upgrade_level=0;
    let (cost,stars)=costs.unwrap_or_else(|| (c.cost,
        c.cv.turn_star_cost_override.or(c.cv.combat_star_cost_override).unwrap_or(c.star_cost)));
    format!("{:?}", (c.id.name(),c.upgraded,upgrade_level,c.cost,cost,c.damage,c.block,c.star_cost,stars,vars,ench))
}

fn pending(choice: Option<(Vec<usize>,Vec<(&'static str,Card)>)>) -> Option<(usize,Vec<usize>,Vec<(&'static str,String)>)> {
    choice.map(|(selected,cards)| (selected.len(),selected,
        cards.iter().map(|(pile,c)| (*pile,card(c,None))).collect()))
}

pub(crate) fn signature(run: &RunManager) -> Result<String,&'static str> {
    let r = run.run_ctx();
    let deck: Vec<_> = r.deck.iter().map(|c| card(c,None)).collect();
    // Frozen relic classes have no `counter` attribute. Private/display counters
    // must not masquerade as getattr(relic,"counter",None) semantic progress.
    let relics: Vec<_> = r.relics.iter().map(|v| (v.id.name(),Option::<i32>::None)).collect();
    let live=run.combat().filter(|_| run.phase()==RunPhase::Combat);
    let (hp,max_hp)=live.map_or((r.hp,r.max_hp),|c| (c.player().hp,c.player().max_hp));
    let potions=live.and_then(|c| c.potion_slots_for_player(0)).unwrap_or(&r.potions);
    let run_choice=pending(run.observation_progress_run_choice()?);
    let event_choice=pending(run.observation_progress_event_choice());
    let mut key = format!("{:?}", (r.act_index,r.total_floor,hp,max_hp,r.gold,
        deck,relics,potions,run.observation_progress_menu(),run_choice,event_choice));
    if run.phase() == RunPhase::Combat {
        let mut c = run.combat().expect("combat phase").clone();
        let piles: Vec<Vec<_>> = [c.players[0].hand.as_slice(),c.players[0].draw.as_slice(),
            c.players[0].discard.as_slice(),c.players[0].exhaust.as_slice()].iter().map(|p| p.to_vec()).collect();
        let piles: Vec<Vec<_>> = piles.iter().map(|p| p.iter().map(|&k| {
            let cost=c.modified_card_cost(0,k);
            let stars=c.modified_star_cost(k);
            card(&c.cards[k as usize],Some((cost,stars)))
        }).collect()).collect();
        // r7 CombatState.all_creatures: primary, allies, enemies (not arena
        // allocation order, nor detached/deleted creatures).
        let creatures: Vec<_> = std::iter::once(0).chain(c.allies.iter().copied())
            .chain(c.enemies.iter().copied()).map(|id| {
            let v=c.cr(id);
            let mut powers: Vec<_> = v.powers.iter().chain(v.extra.iter()).map(|&p| {
                let p=&c.powers[p as usize]; (p.id.name(),p.amount)
            }).collect(); powers.sort_unstable();
            (v.is_player,v.monster_id,v.combat_id,v.hp,v.block,powers)
        }).collect();
        let enemies: Vec<_> = c.enemies.iter().map(|&id| {
            let v=c.cr(id); (v.monster_id,v.hp,v.block,v.is_alive())
        }).collect();
        let choice=if let Some(choice)=c.pending_choice.clone() {
            let mut selected=c.choice_selected.clone(); selected.sort_unstable();
            let source=match choice.source {
                sts2_sim::combat::PileKind::Hand=>"hand", sts2_sim::combat::PileKind::Draw=>"draw",
                sts2_sim::combat::PileKind::Discard=>"discard", sts2_sim::combat::PileKind::Exhaust=>"exhaust",
                sts2_sim::combat::PileKind::Play=>"play", sts2_sim::combat::PileKind::None=>"none",
            };
            let options:Vec<_>=c.choice_options.clone().into_iter().map(|k| {
                let cost=c.modified_card_cost(0,k); let stars=c.modified_star_cost(k);
                (source,card(&c.cards[k as usize],Some((cost,stars))))
            }).collect();
            Some((selected.len(),selected,options))
        } else { None };
        key.push_str(&format!("{:?}", (c.turn_count,c.player().hp,c.player().block,
            c.players[0].energy,enemies,piles,creatures,choice)));
    }
    Ok(key)
}

pub(crate) fn update(before: &str, after: &str, stalled: &mut usize, decisions: usize) -> bool {
    *stalled = if before == after { *stalled + 1 } else { 0 };
    *stalled >= 300 || decisions >= 10000
}

#[cfg(test)]
mod tests {
    #[test]
    fn card_and_choice_keys_preserve_overrides_and_card_details() {
        use sts2_sim::{card::Card,tables::CardId};
        let mut c=Card::from_factory(CardId::Bash,false,1).unwrap();
        let before=super::card(&c,None);
        c.cv.combat_star_cost_override=Some(2);
        assert_ne!(before,super::card(&c,None));
        let base=super::card(&c,None);
        c.cv.turn_star_cost_override=Some(0);
        assert_ne!(base,super::card(&c,None),"turn override precedes combat override");
        let before=super::pending(Some((vec![0],vec![("deck",c.clone())])));
        c.damage=Some(c.damage.unwrap()+3);
        assert_ne!(before,super::pending(Some((vec![0],vec![("deck",c)]))),
            "same model/selection but different option card must be progress");
    }


    #[test]
    fn exact_stall_and_decision_boundaries() {
        let mut stalled=0;
        for n in 1..300 { assert!(!super::update("same","same",&mut stalled,n)); }
        assert!(super::update("same","same",&mut stalled,300));
        assert!(!super::update("old","new",&mut stalled,9999));
        assert_eq!(stalled,0);
        assert!(super::update("old","new",&mut stalled,10000));
    }
    #[test]
    fn presentation_only_and_gameplay_changes_have_distinct_keys() {
        use sts2_sim::run::{RunManager,RunOptions};
        let mut r=RunManager::new(3000040,RunOptions::default());
        let before=super::signature(&r);
        let _=r.available_actions();
        assert_eq!(before,super::signature(&r), "publishing options is not progress");
        let action=r.available_actions()[0];
        r.take_action(action).unwrap();
        assert_ne!(before,super::signature(&r), "acquiring the relic is progress");
    }
}
