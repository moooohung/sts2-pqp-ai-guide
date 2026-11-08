//! r7 `PendingCardChoice` (core/combat.py `request_card_choice`, `request_multi_card_choice`,
//! `resolve_pending_choice`). r7 stores a resolver closure; here the resolver is plain data
//! (`Resolver`) dispatched by `content::resolve_choice_effect`, so a pending choice is `Clone`
//! and allocation-free once warm (option and selection buffers are reused).

use crate::card::{CId, KRef};
use crate::combat::{Combat, PileKind};
use crate::tables::{CardId, MonsterKind, PotionId, RelicId};

/// Who asked for the choice; content code matches on it to continue the effect.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ChoiceSrc {
    Card(CardId),
    Potion(PotionId),
    // relic fire: relics/rare.py:338
    Relic(RelicId),
    /// A monster move that makes players choose (Knowledge Demon's Curse of Knowledge);
    /// `Resolver::k` holds the monster's creature id.
    Monster(MonsterKind),
}

/// The continuation of a paused effect. `step` lets one source ask several times.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Resolver {
    pub src: ChoiceSrc,
    /// Card arena handle; for Potion/Relic continuations, the choosing player's seat.
    pub k: KRef,
    pub target: Option<CId>,
    pub step: u8,
    /// Free per-source argument (an amount, a count).
    pub arg: i32,
}

#[derive(Clone, Debug)]
pub struct PendingChoice {
    /// The selecting player (C# PlayerChoiceContext), independent of effect target.
    pub player: CId,
    pub resolver: Resolver,
    pub source: PileKind,
    pub allow_skip: bool,
    pub min: u16,
    pub max: u16,
    pub single: bool,
}

#[derive(Clone, Debug)]
pub(crate) struct DeferredSetupChoice {
    pub choice: PendingChoice,
    pub options: Vec<KRef>,
    pub draw: Option<(i32, bool, Option<usize>)>,
}

/// One suspended live hook action. Other seats may choose without overwriting
/// this action's selector buffers or its awaited listener continuation.
#[derive(Clone, Debug)]
pub(crate) struct LiveChoiceFrame {
    choice: PendingChoice,
    orb_context: Option<crate::between::OrbContext>,
    orb_spans: Vec<u64>,
    options: Vec<KRef>,
    selected: Vec<u16>,
    walk: Option<crate::hooks::TurnStartWalk>,
    turn_end: Option<crate::hooks::TurnEndWalk>,
    setup: Option<(CId, crate::combat::TurnStage)>,
    stage: Option<crate::combat::TurnStage>,
    play: Vec<crate::combat::PendingPlay>,
    autoplay: Vec<(usize, crate::autoplay::AutoplayWork)>,
    draws: Vec<(usize, CId, i32, bool, Option<usize>)>,
    drawn_hooks: Vec<(usize,KRef,bool)>,
    exhaust_walks: Vec<(usize, crate::hooks::ExhaustWalk)>,
    damage_walks: Vec<(usize, crate::hooks::DamageReceivedWalk)>,
    puzzle_draws: Vec<(usize, CId, i32)>,
    monster_attacks: Vec<(usize, crate::damage::MonsterAttackWalk)>,
    shuffle_relics: Option<CId>,
    potion_after: Option<(usize, CId, PotionId, Option<CId>)>,
}

impl PendingChoice {
    #[inline]
    pub fn is_multi(&self) -> bool {
        !self.single && (self.max != 1 || self.min != 1)
    }
}

#[cfg(test)]
mod parked_work_tests {
    use super::*;
    use crate::{autoplay::AutoplayWork, run::{RunManager, RunOptions}};

    #[test]
    fn hand_revalidation_preserves_c_then_a_selection_order(){
        for parked in [false,true]{
            let run=RunManager::new(1,RunOptions::default()).run_ctx().clone();
            let mut c=Combat::new(run,1);
            c.add_player(50,50,3,0,&[]);
            let mut cards=Vec::new();
            for id in [CardId::StrikeIronclad,CardId::DefendIronclad,CardId::Bash]{
                cards.push(c.new_card(1,id,false).unwrap());
            }
            let [a,_,cc]=cards[..] else{unreachable!()};
            c.players[1].hand.clear();c.players[1].hand.extend([a,cc]);
            c.pending_choice=Some(PendingChoice{player:1,
                resolver:Resolver{src:ChoiceSrc::Card(CardId::Brand),k:a,target:None,step:0,arg:0},
                source:PileKind::Hand,allow_skip:false,min:2,max:3,single:false});
            c.choice_options=cards;c.choice_selected=vec![2,0];
            if parked{c.park_live_choice();}
            c.revalidate_removed_player_hand_choice(1);
            assert!(c.activate_choice_for_player(1));
            assert_eq!(c.choice_options,vec![a,cc]);
            assert_eq!(c.choice_selected,vec![1,0]);
            assert_eq!(c.selected_cards(PileKind::Hand),vec![cc,a]);
        }
    }

    #[test]
    fn death_outside_an_effect_completes_active_or_parked_hand_choice_empty(){
        for parked in [false,true]{
            let run=RunManager::new(202611030989,RunOptions::default()).run_ctx().clone();
            let mut c=Combat::new(run,202611030989);
            c.add_player(50,50,3,0,&[]);
            c.setup_encounter("SLUDGE_SPINNER_WEAK",&mut crate::rng::Rng::new(1));c.start_combat();
            let brand=c.new_card(1,CardId::Brand,false).unwrap();
            let a=c.new_card(1,CardId::StrikeIronclad,false).unwrap();
            let b=c.new_card(1,CardId::DefendIronclad,false).unwrap();
            c.players[1].hand.clear();c.players[1].hand.extend([a,b]);
            c.request_card_choice_for_player(1,&[a,b],PileKind::Hand,
                Resolver{src:ChoiceSrc::Card(CardId::Brand),k:brand,target:Some(1),step:0,arg:0},false);
            assert!(c.pending_choice_for_player(1).is_some());
            let teammate=c.new_card(0,CardId::Bash,false).unwrap();
            if parked{
                c.park_live_choice();
                c.request_card_choice_for_player(0,&[teammate,a],PileKind::Draw,
                    Resolver{src:ChoiceSrc::Card(CardId::Headbutt),k:teammate,target:None,step:0,arg:0},false);
            }
            let teammate_choice=c.pending_choice_for_player(0).map(|c|c.resolver);
            assert!(c.kill_creature(1));
            assert!(c.players[1].hand.is_empty());
            assert!(c.pending_choice_for_player(1).is_none());
            assert!(c.choice_options_for_player(1).is_none());
            assert!(!c.activate_choice_for_player(1));
            assert!(!c.resolve_pending_choice_for_player(1,Some(0)));
            c.restore_first_live_choice();
            assert!(c.pending_choice_for_player(1).is_none());
            assert_eq!(c.pending_choice_for_player(0).map(|c|c.resolver),teammate_choice);
            assert!(!c.is_over);assert!(c.fault.is_none());
        }
    }

    #[test]
    fn seed_202611030989_lethal_brand_keeps_its_dead_owners_awaited_choice_resolvable(){
        // NativeObs scenario 989, generation 213: dead seat 1 still awaits
        // Brand's mandatory hand exhaust. HandlePlayerDeath defers pile removal
        // until EndCardOrPotionEffect; hook deactivation is not choice cancellation.
        let run=RunManager::new(202611030989,RunOptions::default()).run_ctx().clone();
        let mut c=Combat::new(run,202611030989);
        for _ in 1..4{c.add_player(50,50,3,0,&[]);}
        c.setup_encounter("SLUDGE_SPINNER_WEAK",&mut crate::rng::Rng::new(1));c.start_combat();
        c.players[1].hand.clear();c.players[1].energy=3;
        let brand=c.new_card(1,CardId::Brand,false).unwrap();
        let strike=c.new_card(1,CardId::StrikeIronclad,false).unwrap();
        let defend=c.new_card(1,CardId::DefendIronclad,false).unwrap();
        c.players[1].hand.push(brand);c.players[1].hand.push(strike);c.players[1].hand.push(defend);c.cr_mut(1).hp=1;
        assert!(c.play_card(1,0,None));
        assert_eq!(c.cr(1).hp,0);assert!(!c.players[1].hooks_active);
        assert!(c.pending_choice_for_player(1).is_some());
        c.park_live_choice();c.restore_first_live_choice();
        assert!(c.pending_choice_for_player(1).is_some());
        assert!(c.resolve_pending_choice_for_player(1,Some(0)));
        assert!(c.pending_choice_for_player(1).is_none());
        assert!(c.players[1].hand.is_empty());assert!(c.players[1].play.is_empty());
        assert!(!c.is_over);
    }

    #[test]
    fn death_during_a_parked_brand_effect_defers_card_removal_until_resolution(){
        let run=RunManager::new(202611030989,RunOptions::default()).run_ctx().clone();
        let mut c=Combat::new(run,202611030989);
        c.add_player(50,50,3,0,&[]);
        c.setup_encounter("SLUDGE_SPINNER_WEAK",&mut crate::rng::Rng::new(1));c.start_combat();
        c.players[1].hand.clear();c.players[1].energy=3;
        for id in [CardId::Brand,CardId::StrikeIronclad,CardId::DefendIronclad]{
            let k=c.new_card(1,id,false).unwrap();c.players[1].hand.push(k);
        }
        c.cr_mut(1).hp=2;assert!(c.play_card(1,0,None));c.park_live_choice();
        assert!(c.kill_creature(1));
        assert_eq!(c.players[1].hand.len(),2);
        assert!(c.pending_choice_for_player(1).is_some());
        assert!(c.resolve_pending_choice_for_player(1,Some(0)));
        assert!(c.pending_choice_for_player(1).is_none());
        assert!(c.players[1].hand.is_empty());assert!(c.players[1].play.is_empty());
        assert!(!c.is_over);assert!(c.fault.is_none());
    }

    #[test]
    fn live_seat_switch_restores_all_awaited_work_without_leaking_to_other_seat() {
        let run = RunManager::new(1, RunOptions::default()).run_ctx().clone();
        let mut c = Combat::new(run, 1);
        c.add_player(50, 50, 3, 0, &[]);
        let choice = |player| PendingChoice {
            player, resolver: Resolver { src: ChoiceSrc::Card(CardId::Stratagem), k: player as KRef,
                target: Some(player), step: 1, arg: 0 },
            source: PileKind::Draw, allow_skip: false, min: 1, max: 1, single: false,
        };
        c.pending_choice = Some(choice(0));
        c.autoplay_snapshots.push((0, AutoplayWork::Snapshot(vec![10, 11], false)));
        c.pending_draws.push((0, 0, 3, false,None));
        c.pending_drawn_hooks.push((0,10,false));
        c.pending_shuffle_relics = Some(0);
        c.pending_potion_after = Some((0, 0, PotionId::Distilledchaos, Some(0)));
        c.park_live_choice();
        assert!(c.autoplay_snapshots.is_empty() && c.pending_draws.is_empty());
        assert!(c.pending_drawn_hooks.is_empty());
        assert_eq!(c.pending_shuffle_relics, None);
        assert_eq!(c.pending_potion_after, None);
        c.pending_choice = Some(choice(1));
        c.pending_draws.push((0, 1, 2, true,None));
        c.pending_drawn_hooks.push((0,11,true));
        assert!(c.activate_choice_for_player(0));
        assert_eq!(c.pending_draws, vec![(0, 0, 3, false,None)]);
        assert_eq!(c.pending_drawn_hooks,vec![(0,10,false)]);
        assert!(matches!(&c.autoplay_snapshots[0].1, AutoplayWork::Snapshot(cards, false) if cards == &[10,11]));
        assert_eq!(c.pending_shuffle_relics, Some(0));
        assert_eq!(c.pending_potion_after, Some((0, 0, PotionId::Distilledchaos, Some(0))));
        assert!(c.activate_choice_for_player(1));
        assert_eq!(c.pending_draws, vec![(0, 1, 2, true,None)]);
        assert_eq!(c.pending_drawn_hooks,vec![(0,11,true)]);
        assert!(c.autoplay_snapshots.is_empty());
        assert_eq!(c.pending_shuffle_relics, None);
        assert_eq!(c.pending_potion_after, None);
    }

    #[test]
    fn invalid_live_reentry_never_replaces_a_seats_awaited_selector_or_snapshot() {
        use crate::{combat::{ChoiceScheduling, TurnStage}, tables::Hook};
        for seats in 2..=4 {
            for owner in 0..seats {
                let mut run = RunManager::new(1, RunOptions::default()).run_ctx().clone();
                run.choice_scheduling = ChoiceScheduling::Live;
                let mut c = Combat::new(run, 1);
                for _ in 1..seats { c.add_player(50, 50, 3, 0, &[]); }
                let cards: Vec<_> = (0..2).map(|_| c.new_card(owner, CardId::Bash, false).unwrap()).collect();
                let resolver = Resolver { src: ChoiceSrc::Card(CardId::Headbutt),
                    k: cards[0], target: None, step: 0, arg: 0 };
                c.request_card_choice_for_player(owner, &cards, PileKind::Discard, resolver, false);
                let first = c.pending_choice.clone().unwrap();
                c.request_card_choice_for_player(owner, &cards[1..], PileKind::Draw, resolver, true);
                assert_eq!(c.fault, Some("choice continuation would overwrite an unresolved choice"));
                assert_eq!(c.pending_choice.as_ref().unwrap().resolver, first.resolver);
                assert_eq!(c.choice_options, cards);
                // Hook.cs:714-740 awaits each callback. A second invocation
                // without parking/finishing the first is an invalid executor call,
                // not a normal second seat. Valid concurrent dispatch is exercised
                // by concurrent_autoplay and the original-DLL v10 captures.
                c.fault = None;
                c.fire_setup_hooks(owner, &[Hook::AfterPlayerTurnStart], TurnStage::PostAfterPlayerTurnStart);
                let snapshot = format!("{:?}", c.pending_player_turn_hooks);
                c.fire_setup_hooks((owner + 1) % seats, &[Hook::BeforeHandDraw], TurnStage::Draw);
                assert_eq!(c.fault, Some("concurrent player turn-start selectors require seat scheduler"));
                assert_eq!(format!("{:?}", c.pending_player_turn_hooks), snapshot);
                c.fault = None;
                c.pending_player_turn_hooks = None;
                c.pending_choice = None;
                c.collect_setup_choices = true;
                c.request_card_choice_for_player(owner, &cards, PileKind::Hand, resolver, false);
                c.request_card_choice_for_player(owner, &cards, PileKind::Hand, resolver, false);
                assert_eq!(c.fault, Some("setup callback requested two choices without awaiting"));
                assert_eq!(c.signalled_setup_choice.as_ref().unwrap().choice.resolver, resolver);
                c.fault = None;
                c.fire_setup_hooks(owner, &[Hook::AfterPlayerTurnStart], TurnStage::PostAfterPlayerTurnStart);
                // Deferred callback is captured and removed from the active slot.
                assert!(c.pending_player_turn_hooks.is_none());
            }
        }
    }
}

impl Combat {
    pub(crate) fn has_parked_turn_end_tasks(&self) -> bool {
        self.live_choices.iter().any(|f| f.turn_end.is_some())
    }
    /// Owner choices block OnPlay. Teammate choices require a queued hook action
    /// after the parent's after-play work, not a join of cloned parent frames.
    pub(crate) fn draw_team_without_blocking(&mut self, count: i32) {
        self.continue_team_draw(0, count);
    }

    pub(crate) fn continue_team_draw(&mut self, next: CId, count: i32) {
        let collecting = self.collect_setup_choices;
        for player in next..self.players.len() as CId {
            if !self.cr(player).is_alive() { continue; }
            self.collect_setup_choices = true;
            self.draw_cards_for_branch(player, count);
            self.collect_setup_choices = collecting;
            if self.fault.is_some() { return; }
            let Some(choice) = self.signalled_setup_choice.take() else { continue };
            // HookPlayerChoiceContext queues this branch on the chooser's queue;
            // options must be read only when that queued action starts.
            if self.run.choice_scheduling.is_live()
                && !self.pending_play.last().is_some_and(|p| p.owner == player)
            {
                self.deferred_setup_choices.push((player, None, choice));
                continue;
            }
            self.present_setup_choice(choice);
            if self.pending_choice.is_some() {
                // BranchingPlayerChoiceContext only releases another player's
                // branch. The card owner's own selector still blocks OnPlay.
                self.pending_play.last_mut().unwrap().team_draw_after_owner = Some((player + 1, count));
                return;
            }
        }
    }

    /// Reuse the live hook queue and parked stacks. Parent after-play, result
    /// placement and empty-hand callbacks finish before a teammate UI opens.
    pub(crate) fn with_live_queue_action<T>(&mut self, action: impl FnOnce(&mut Self) -> T) -> T {
        self.live_action_depth += 1;
        let result = action(self);
        self.live_action_depth -= 1;
        self.dispatch_queued_team_draws();
        result
    }

    pub(crate) fn dispatch_queued_team_draws(&mut self) {
        if !self.run.choice_scheduling.is_live()
            || self.live_action_depth != 0
            || !self.pending_play.is_empty() || self.pending_choice.is_some()
            || self.is_over || self.fault.is_some() { return; }
        while let Some(index) = self.deferred_setup_choices.iter().position(|(owner, walk, _)|
            walk.is_none() && self.pending_choice_for_player(*owner).is_none())
        {
            let (_, _, deferred) = self.deferred_setup_choices.remove(index);
            self.present_setup_choice(deferred);
            if self.pending_choice.is_some() { self.park_live_choice(); }
            if self.is_over || self.fault.is_some() { return; }
        }
        self.restore_first_live_choice();
    }

    pub fn pending_choice_for_player(&self, player: CId) -> Option<&PendingChoice> {
        if !self.players.is_player(player) { return None; }
        self.pending_choice.as_ref().filter(|c| c.player == player)
            .or_else(|| self.live_choices.iter().find(|f| f.choice.player == player).map(|f| &f.choice))
    }

    pub(crate) fn pending_choice_count_for_player(&self, player: CId) -> usize {
        usize::from(self.pending_choice.as_ref().is_some_and(|choice| choice.player == player))
            + self.live_choices.iter().filter(|frame| frame.choice.player == player).count()
    }

    pub fn choice_options_for_player(&self, player: CId) -> Option<&[KRef]> {
        if self.pending_choice.as_ref().is_some_and(|c| c.player == player) {
            Some(&self.choice_options)
        } else {
            self.live_choices.iter().find(|f| f.choice.player == player).map(|f| f.options.as_slice())
        }
    }

    /// Borrow the authenticated owner's selection without activating a parked frame.
    pub fn choice_selected_for_player(&self, player: CId) -> Option<&[u16]> {
        if self.pending_choice.as_ref().is_some_and(|c| c.player == player) {
            Some(&self.choice_selected)
        } else {
            self.live_choices.iter().find(|f| f.choice.player == player).map(|f| f.selected.as_slice())
        }
    }

    pub(crate) fn park_live_choice(&mut self) {
        if let Some(choice) = self.pending_choice.take() {
            let frame=self.take_live_choice_frame(choice);
            self.live_choices.push(frame);
        }
    }

    pub(crate) fn cancel_live_orb_walks(&mut self) {
        let frames = std::mem::take(&mut self.live_choices);
        for frame in frames {
            self.finish_orb_context(&frame.orb_spans, "cancel");
            for (_, walk) in frame.damage_walks {
                self.cancel_damage_received_walk(walk);
            }
        }
    }

    fn take_live_choice_frame(&mut self,choice:PendingChoice)->LiveChoiceFrame{
        LiveChoiceFrame {
                choice, orb_context: self.pending_choice_orb_context.take(),
                orb_spans: std::mem::take(&mut self.pending_choice_orb_spans),
                options: std::mem::take(&mut self.choice_options),
                selected: std::mem::take(&mut self.choice_selected),
                walk: self.pending_player_turn_hooks.take(),
                turn_end: self.pending_turn_end_hooks.take(),
                setup: self.pending_live_setup.take(), stage: self.pending_turn_setup.take(),
                play: std::mem::take(&mut self.pending_play),
                autoplay: std::mem::take(&mut self.autoplay_snapshots),
                draws: std::mem::take(&mut self.pending_draws),
                drawn_hooks: std::mem::take(&mut self.pending_drawn_hooks),
                exhaust_walks: std::mem::take(&mut self.pending_exhaust_walks),
                damage_walks: std::mem::take(&mut self.pending_damage_walks),
                puzzle_draws: std::mem::take(&mut self.pending_puzzle_draws),
                monster_attacks: std::mem::take(&mut self.pending_monster_attacks),
                shuffle_relics: std::mem::take(&mut self.pending_shuffle_relics),
                potion_after: self.pending_potion_after.take(),
        }
    }

    fn install_live_choice_frame(&mut self,frame:LiveChoiceFrame){
        self.pending_choice = Some(frame.choice);
        self.pending_choice_orb_context = frame.orb_context;
        self.pending_choice_orb_spans = frame.orb_spans;
        self.choice_options = frame.options;
        self.choice_selected = frame.selected;
        self.pending_player_turn_hooks = frame.walk;
        self.pending_turn_end_hooks = frame.turn_end;
        self.pending_live_setup = frame.setup;
        self.pending_turn_setup = frame.stage;
        self.pending_play = frame.play;
        self.autoplay_snapshots = frame.autoplay;
        self.pending_draws = frame.draws;
        self.pending_drawn_hooks = frame.drawn_hooks;
        self.pending_exhaust_walks = frame.exhaust_walks;
        self.pending_damage_walks = frame.damage_walks;
        self.pending_puzzle_draws = frame.puzzle_draws;
        self.pending_monster_attacks = frame.monster_attacks;
        self.pending_shuffle_relics = frame.shuffle_relics;
        self.pending_potion_after = frame.potion_after;
    }

    pub(crate) fn has_executing_player_effect(&self,player:CId)->bool{
        self.pending_play.iter().any(|f|f.owner==player)
            ||self.pending_potion_after.is_some_and(|(_,owner,_,_)|owner==player)
            ||self.live_choices.iter().any(|f|f.play.iter().any(|p|p.owner==player)
                ||f.potion_after.is_some_and(|(_,owner,_,_)|owner==player))
    }

    /// HandlePlayerDeath removes piles outside an executing effect.
    /// NPlayerHand.RevalidateSelectionAfterStateChange removes stale candidates
    /// and completes an emptied selector with [] regardless of MinSelect.
    pub(crate) fn revalidate_removed_player_hand_choice(&mut self,player:CId){
        let hand=self.players[player].hand.to_vec();
        fn retain_live(options:&mut Vec<KRef>,selected:&mut Vec<u16>,hand:&[KRef]){
            let chosen:Vec<_>=selected.iter().filter_map(|&i|options.get(i as usize).copied()).collect();
            options.retain(|k|hand.contains(k));
            *selected=chosen.into_iter().filter_map(|k|
                options.iter().position(|&option|option==k).map(|i|i as u16)).collect();
        }
        if self.pending_choice.as_ref().is_some_and(|c|c.player==player&&c.source==PileKind::Hand){
            retain_live(&mut self.choice_options,&mut self.choice_selected,&hand);
            if self.choice_options.is_empty(){
                let choice=self.pending_choice.take().unwrap();
                self.complete_pending_choice(choice,&[]);
            }
        }
        for f in &mut self.live_choices{
            if f.choice.player==player&&f.choice.source==PileKind::Hand{
                retain_live(&mut f.options,&mut f.selected,&hand);
            }
        }
        while let Some(index)=self.live_choices.iter().position(|f|
            f.choice.player==player&&f.choice.source==PileKind::Hand&&f.options.is_empty()){
            let frame=self.live_choices.remove(index);
            // A different player's action may be inside CreatureCmd.Kill.
            // Save ALL its work even if it has no selector of its own.
            let active=self.pending_choice.take();
            let saved=self.take_live_choice_frame(active.clone().unwrap_or_else(||frame.choice.clone()));
            self.install_live_choice_frame(frame);
            self.live_action_depth+=1;
            let choice=self.pending_choice.take().unwrap();
            self.complete_pending_choice(choice,&[]);
            self.park_live_choice();
            self.install_live_choice_frame(saved);
            self.pending_choice=active;
            self.live_action_depth-=1;
        }
    }

    /// Select an already-open UI; this does not present it again or rebuild its
    /// frozen options. Decision/observation callers must identify their chooser.
    pub fn activate_choice_for_player(&mut self, player: CId) -> bool {
        if self.is_over || !self.players.is_player(player) { return false; }
        // CombatManager.HandlePlayerDeath deactivates the dead player's hooks,
        // but an already-running card/potion effect is allowed to finish before
        // EndCardOrPotionEffect removes that player's combat cards. Therefore an
        // already-open CardSelectCmd UI remains resolvable after its owner dies.
        if self.pending_choice.as_ref().is_some_and(|c| c.player == player) { return true; }
        let Some(index) = self.live_choices.iter().position(|f| f.choice.player == player) else { return false; };
        let frame = self.live_choices.remove(index);
        self.park_live_choice();
        self.install_live_choice_frame(frame);
        true
    }

    pub fn resolve_pending_choice_for_player(&mut self, player: CId, index: Option<usize>) -> bool {
        self.activate_choice_for_player(player) && self.resolve_pending_choice(index)
    }

    pub(crate) fn restore_first_live_choice(&mut self) {
        if self.pending_choice.is_none() {
            if let Some(frame) = self.live_choices.first() {
                self.activate_choice_for_player(frame.choice.player);
            }
        }
    }

    fn selector_player(&mut self, resolver: Resolver) -> Option<CId> {
        match resolver.src {
            ChoiceSrc::Card(_) => self.cards.get(resolver.k as usize).map(|c| c.owner),
            ChoiceSrc::Potion(_) | ChoiceSrc::Relic(_) => {
                let player = resolver.k as CId;
                if resolver.k < self.players.len() as KRef { Some(player) }
                else { self.unported("inventory selector requires a valid choosing player"); None }
            }
            ChoiceSrc::Monster(_) => {
                self.unported("monster selector requires explicit choosing player");
                None
            }
        }
    }

    /// `request_card_choice`: pause until one card is chosen (or skipped).
    pub fn request_card_choice(&mut self, cards: &[KRef], source: PileKind, resolver: Resolver, allow_skip: bool) {
        let Some(player) = self.selector_player(resolver) else { return };
        self.request_card_choice_for_player(player, cards, source, resolver, allow_skip);
    }

    pub fn request_card_choice_for_player(&mut self, player: CId, cards: &[KRef], source: PileKind, resolver: Resolver, allow_skip: bool) {
        if self.pending_choice.is_some() {
            self.unported("choice continuation would overwrite an unresolved choice");
            return;
        }
        if self.is_over_or_ending() {
            return;
        }
        let choice = PendingChoice { player, resolver, source, allow_skip, min: if allow_skip { 0 } else { 1 }, max: 1, single: true };
        if self.defer_setup_choice(cards, choice.clone()) { return; }
        if !allow_skip && cards.len() <= 1 {
            let picked: Option<KRef> = cards.first().copied();
            self.resolve_choice_effect(resolver, picked.as_slice());
            return;
        }
        self.open_choice(cards, choice);
    }

    /// `request_multi_card_choice`: pause until `min..=max` cards are selected and confirmed.
    pub fn request_multi_card_choice(&mut self, cards: &[KRef], source: PileKind, resolver: Resolver,
                                     min: u16, max: Option<u16>, allow_skip: bool) {
        let Some(player) = self.selector_player(resolver) else { return };
        self.request_multi_card_choice_for_player(player, cards, source, resolver, min, max, allow_skip);
    }

    pub fn request_multi_card_choice_for_player(&mut self, player: CId, cards: &[KRef], source: PileKind, resolver: Resolver,
                                     min: u16, max: Option<u16>, allow_skip: bool) {
        if self.pending_choice.is_some() {
            self.unported("choice continuation would overwrite an unresolved choice");
            return;
        }
        if self.is_over_or_ending() {
            return;
        }
        let max = max.unwrap_or(min);
        let choice = PendingChoice { player, resolver, source, allow_skip, min, max, single: false };
        if self.defer_setup_choice(cards, choice.clone()) { return; }
        if !allow_skip && min == max && cards.len() <= min as usize {
            let picked: Vec<KRef> = cards.to_vec();
            self.resolve_choice_effect(resolver, &picked);
            return;
        }
        self.open_choice(cards, choice);
    }

    fn defer_setup_choice(&mut self, cards: &[KRef], choice: PendingChoice) -> bool {
        if !self.collect_setup_choices { return false; }
        // CardSelectCmd.cs:379-382 signals only without an installed Selector.
        // MpOracle.InstallLiveSelectors patches FromHand/Choose separately from
        // FromCombatPile. A synchronous pile selector must finish this owner's
        // draw before any later seat starts setup, not join the live hook queue.
        if !self.run.choice_scheduling.signals_choice(choice.source) {
            return false;
        }
        if self.signalled_setup_choice.is_some() {
            self.unported("setup callback requested two choices without awaiting");
        } else {
            // SignalPlayerChoiceBegun releases the setup task BEFORE FromHand
            // enumerates its options, even if that hand will be empty/automatic.
            self.signalled_setup_choice = Some(DeferredSetupChoice {
                choice, options: cards.to_vec(), draw: None,
            });
        }
        true
    }

    pub(crate) fn present_setup_choice(&mut self, deferred: DeferredSetupChoice) {
        let draw_stop = self.pending_play.len();
        let choice = deferred.choice;
        let drawing = deferred.draw.is_some();
        if let Some((count, from_hand_draw,collector)) = deferred.draw {
            self.pending_draws.push((self.pending_play.len(), choice.player, count, from_hand_draw,collector));
            self.pending_shuffle_relics = Some(choice.player);
        }
        let cards = if choice.source == PileKind::Hand {
            self.players[choice.player].hand.to_vec()
        } else if drawing {
            crate::content::draw_selector_order_pub(self, choice.player)
        } else { deferred.options };
        if cards.is_empty() {
            self.resolve_choice_effect(choice.resolver, &[]);
            if drawing { self.resume_pending_play(draw_stop); }
            return;
        }
        if choice.single {
            self.request_card_choice_for_player(choice.player, &cards, choice.source, choice.resolver, choice.allow_skip);
        } else {
            let max = if choice.allow_skip { choice.max.max(cards.len() as u16) } else { choice.max };
            self.request_multi_card_choice_for_player(choice.player, &cards, choice.source,
                choice.resolver, choice.min, Some(max), choice.allow_skip);
        }
        if drawing && self.pending_choice.is_none() { self.resume_pending_play(draw_stop); }
    }

    fn open_choice(&mut self, cards: &[KRef], choice: PendingChoice) {
        if self.pending_choice.is_some() {
            self.unported("choice continuation would overwrite an unresolved choice");
            return;
        }
        // Only an actually open synchronous selector parks this setup task.
        // Empty/automatic pile choices must not suppress a later hand signal.
        if !self.run.choice_scheduling.signals_choice(choice.source) {
            self.collect_setup_choices = false;
        }
        self.choice_options.clear();
        self.choice_options.extend_from_slice(cards);
        self.choice_selected.clear();
        let orb_context = self.current_orb_context();
        self.pending_choice_orb_spans = self.suspend_orb_context(orb_context);
        self.pending_choice_orb_context = orb_context;
        self.pending_choice = Some(choice);
        if self.players.len()>1 && self.run.choice_scheduling.is_live() {self.register_queue_choices();}
        if let Some(probe) = self.on_choice_presented { probe(self); }
    }

    /// Selected cards in r7 order: selection order when every option came from the hand,
    /// else option order.
    fn selected_cards(&self, source: PileKind) -> Vec<KRef> {
        let ordered_pile_selection = self.pending_choice.as_ref().is_some_and(|choice|
            matches!(choice.resolver.src, ChoiceSrc::Card(crate::tables::CardId::Stratagem | crate::tables::CardId::Dredge)));
        // StratagemPower.AfterShuffle awaits FromCombatPile, then adds its
        // returned cards sequentially. C# preserves the chosen order.
        if source == PileKind::Hand || ordered_pile_selection {
            self.choice_selected.iter().map(|&i| self.choice_options[i as usize]).collect()
        } else {
            let mut idx: Vec<u16> = self.choice_selected.clone();
            idx.sort_unstable();
            idx.iter().map(|&i| self.choice_options[i as usize]).collect()
        }
    }

    /// `resolve_pending_choice(index)`: `None` confirms (multi) or skips (single).
    pub fn resolve_pending_choice(&mut self, index: Option<usize>) -> bool {
        self.with_live_queue_action(|c| c.resolve_pending_choice_action(index))
    }

    fn resolve_pending_choice_action(&mut self, index: Option<usize>) -> bool {
        let Some(choice) = self.pending_choice.clone() else {
            return false;
        };
        let n = self.choice_options.len();
        let selected: Vec<KRef>;
        if choice.is_multi() {
            match index {
                None => {
                    let count = self.choice_selected.len() as u16;
                    if !(count >= choice.min || (choice.allow_skip && count == 0)) {
                        return false;
                    }
                    selected = self.selected_cards(choice.source);
                    self.pending_choice = None;
                }
                Some(i) => {
                    // toggle
                    if i >= n {
                        return false;
                    }
                    if let Some(pos) = self.choice_selected.iter().position(|&s| s as usize == i) {
                        // NCombatPileCardSelectScreen uses HashSet slot reuse:
                        // A,B,-A,C enumerates C,B, not B,C. Until slot/free-list
                        // state is represented, reject deselection before mutation.
                        if matches!(choice.resolver.src, ChoiceSrc::Card(crate::tables::CardId::Stratagem)) {
                            self.unported("Stratagem deselection requires HashSet slot order");
                            return false;
                        }
                        self.choice_selected.remove(pos);
                        return true;
                    }
                    if self.choice_selected.len() as u16 >= choice.max {
                        return false;
                    }
                    self.choice_selected.push(i as u16);
                    // Dredge uses fixed-count CardSelectorPrefs, so the combat
                    // pile screen completes at MaxSelect without confirmation.
                    if matches!(choice.resolver.src, ChoiceSrc::Card(crate::tables::CardId::Dredge))
                        && self.choice_selected.len() as u16 == choice.max
                    {
                        selected = self.selected_cards(choice.source);
                        self.pending_choice = None;
                    } else {
                        return true;
                    }
                }
            }
        } else {
            match index {
                None if !choice.allow_skip => return false,
                None => selected = Vec::new(),
                Some(i) if i >= n => return false,
                Some(i) => selected = vec![self.choice_options[i]],
            }
            self.pending_choice = None;
        }
        self.complete_pending_choice(choice,&selected);
        true
    }

    fn complete_pending_choice(&mut self,choice:PendingChoice,selected:&[KRef]){
        let context = self.pending_choice_orb_context.take();
        let spans = std::mem::take(&mut self.pending_choice_orb_spans);
        let _orb_context = self.restore_orb_context(context, &spans);
        self.resolve_choice_effect(choice.resolver, selected);
        // r7 order: _resume_pending_draw (not used by ported content), pending play, turn setup.
        if self.pending_choice.is_none() && (!self.pending_play.is_empty()
            || !self.pending_draws.is_empty() || !self.pending_drawn_hooks.is_empty()
            || !self.autoplay_snapshots.is_empty()
            || !self.pending_exhaust_walks.is_empty()
            || !self.pending_damage_walks.is_empty() || !self.pending_puzzle_draws.is_empty()
            || !self.pending_monster_attacks.is_empty()
            || self.pending_shuffle_relics.is_some() || self.pending_potion_after.is_some()) {
            self.resume_pending_play(0);
        }
        if self.pending_choice.is_none() {
            self.resume_player_turn_hooks();
        }
        if self.pending_choice.is_none() && let Some(stage) = self.pending_turn_setup.take() {
            self.resume_turn_after_choice(stage);
        }
        self.check_combat_end();
        self.dispatch_queued_team_draws();
        self.finish_orb_context(&spans, "end");
    }
}
