//! Frozen-r7 run layer: seed -> map -> combat -> rewards.

mod catalog;
mod card_pools;
pub(crate) mod potion_pools;
mod events;
mod manager;
pub mod mp;
mod reward_modifiers;
mod tanx;
mod map;
mod rest;
mod rooms;
mod seed_adapter;
mod shop;
mod treasure;
pub mod replay_view;
pub mod event_preview;

pub use manager::recorded_selector_key;
pub use manager::{ObservationChoiceCard, ObservationChoiceOperation, ObservationRunChoice, RunAction, RunActionKind, RunManager, RunOptions, RunPhase};
pub use map::{ActMap, Coord, MapPoint};
pub use seed_adapter::gym_seed_to_run_seed;
pub use events::EventDescription;
pub use events::native_offer::NativeEventOffer;
pub use events::native_identity::{EventOptionIdentity, EventPageIdentity, EventParameter, NativeEventOption};
pub use manager::conveyor::Dish as EventDish;
pub use manager::native_view::*;
pub use manager::merchant_fixture::{MerchantFixture, MerchantFixtureCard, MerchantFixtureOffer};
pub use rest::RestOptionKind;
