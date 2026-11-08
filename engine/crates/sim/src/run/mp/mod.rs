//! Multiplayer run foundations. Not yet a playable run manager.
//! Shared-world progression and inventory adapters must not clone a solo manager.
mod rng;
mod state;
mod ascension;
mod combat_bridge;
mod map_vote;
mod pickup;
mod neow;
mod world;
mod driver;
mod noncombat;
mod merchant;
mod merchant_pickups;
pub mod census;
pub use merchant_pickups::MerchantPickupChoice;
pub use merchant::{MerchantAction, MerchantOffer};
mod events;
mod philosophers;
mod wongo;
mod shared_event_vote;
mod reflections;
mod battleworn_dummy;
mod doll_room;
mod spirit_grafter;
mod lantern_key;
#[cfg(test)]
mod doll_lantern_graft_tests;
mod field_of_man_sized_holes;
mod amalgamator;
mod infested_automaton;
#[cfg(test)]
mod act2_owned_tests;
mod potion_courier;
mod round_tea_party;
mod grave_of_the_forgotten;
mod crystal_sphere;
mod architect;
mod symbiote;
mod rest_site;
mod treasure;
mod late_treasure;
mod rewards;
mod card_removal_reward;
mod cards;
mod paels_growth;
mod precarious_shears;
mod new_leaf;
mod neow_pickups;
mod pandoras_box;
mod relic_rewards;
mod unlocks;
#[cfg(test)]
mod owned_events_tests;

pub use rng::PlayerRngs;
pub use state::{Character, MpPlayer, MpRoster, PlayerSpec};
pub use state::MpExtraPlayerFields;
pub use unlocks::MpUnlockState;
pub use map_vote::{MapVote, MapVoting};
pub use world::MpWorld;
pub use driver::{MpRun, MpPhase};
pub use rewards::PlayerRewards;
mod native_view;
pub use native_view::NativeMpEventChoice;
