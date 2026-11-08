//! Offline checkpoint inspection, deliberately separate from learned inputs.
//! These immutable borrows/copies expose the state projected by the pinned
//! game's HumanPlayRecorder.ReplayCheckpoints.Capture. No method advances play.
use crate::{card::Card, combat::{Combat,RunCtx}, relic::Relic, rng::Rng,
    tables::{PotionId,RelicId}};
use super::{ActMap,EventOptionIdentity,NativeEventOffer,NativeReward,NativeShopOffer};

#[derive(Clone,Debug)]
pub struct ReplayPlayerRngs<'a> {
    pub rewards: &'a Rng,
    pub shops: &'a Rng,
    pub transformations: &'a Rng,
}

#[derive(Debug)]
pub struct ReplaySeatView<'a> {
    pub character: super::mp::Character,
    pub hp: i32,
    pub max_hp: i32,
    pub gold: i32,
    pub deck: &'a [Card],
    pub relics: Vec<&'a Relic>,
    pub potions: &'a [Option<PotionId>],
    pub potion_slots: usize,
    pub rng: ReplayPlayerRngs<'a>,
    pub card_rarity_odds: f32,
    pub potion_reward_odds: f32,
    /// Common, Uncommon, Rare, Shop; ordered deque contents.
    pub relic_bag: &'a [Vec<RelicId>;4],
}

#[derive(Clone,Debug)]
pub struct ReplayEventOption {
    pub identity: EventOptionIdentity,
    pub enabled: bool,
    pub offer: Option<NativeEventOffer>,
}

#[derive(Clone,Debug)]
pub struct ReplayEventView {
    pub id: crate::identity::EventId,
    pub rng: Rng,
    pub options: Vec<ReplayEventOption>,
}

#[derive(Debug)]
pub enum ReplayRewards<'a> {
    Solo(Vec<NativeReward<'a>>),
    Party(Option<&'a [super::mp::PlayerRewards]>),
}

#[derive(Debug)]
pub struct ReplayCheckpointView<'a> {
    pub run: &'a RunCtx,
    pub room_loaded: bool,
    pub combat: Option<&'a Combat>,
    /// PlayerCombatState survives room exit; enemies/round do not.
    pub player_combat: Option<&'a Combat>,
    pub seats: Vec<ReplaySeatView<'a>>,
    pub shared_relic_bag: &'a [Vec<RelicId>;4],
    /// Initial Event/Ancient shared pools. Native ordinary reward draws do not
    /// consume these pools; replay must reject unsupported auxiliary-pool flows.
    pub initial_shared_auxiliary: [Vec<RelicId>;2],
    pub map: Option<&'a ActMap>,
    pub act_floor: i32,
    pub act_order: Vec<&'static str>,
    pub room_model: Option<&'a str>,
    pub event: Option<ReplayEventView>,
    pub shops: Vec<Vec<NativeShopOffer<'a>>>,
    pub rewards: ReplayRewards<'a>,
}

/// Reproduce the already-consumed initialization prefix on a local RNG only.
/// This exposes discarded auxiliary-pool order without advancing live streams.
pub fn initial_shared_auxiliary(seed:u64)->[Vec<RelicId>;2] {
    use super::catalog::*;
    let mut rng=Rng::named(seed,"up_front");
    for names in [SHARED_RELICS_UNCOMMON,SHARED_RELICS_COMMON,SHARED_RELICS_RARE,SHARED_RELICS_SHOP] {
        let mut pool:Vec<_>=names.iter().map(|s|RelicId::from_name(s).expect("audited shared relic")).collect();
        rng.shuffle(&mut pool);
    }
    let mut event=vec![RelicId::FresnelLens];
    let mut ancient=vec![RelicId::LoomingFruit,RelicId::VeryHotCocoa];
    rng.shuffle(&mut event);rng.shuffle(&mut ancient);
    [event,ancient]
}
