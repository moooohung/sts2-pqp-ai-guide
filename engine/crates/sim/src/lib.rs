//! CPU-native STS2 simulator, ported directly from the frozen r7 Python `sts2_env`.
//! Design and phases: docs/cpu_sim_design.md.

pub mod card;
pub mod card_preview;
pub mod action_queue;
// fire: core/combat.py:3976-4039
pub mod autoplay;
pub mod cardgen;
pub mod character;
pub mod choice;
pub mod combat;
pub mod content;
pub mod creature;
pub mod damage;
pub mod determinize;
pub mod digest;
// relic fire: core/hooks.py:781
pub mod extra_turn;
pub mod flow;
pub mod piles;
pub mod hooks;
pub mod identity;
pub mod monster;
pub mod multiplayer;
pub mod obs;
pub mod orbs;
// relic fire: relics/shop_event.py:2683
pub mod paels_tooth;
// relic fire: core/combat.py:3319
pub mod pets;
pub mod between;
pub mod player;
pub mod potion;
pub mod powers;
// Other base characters' powers (Prism / Kaleidoscope card sources).
pub mod powers_chars;
pub mod powers_defect;
pub mod regent;
pub mod relic;
pub mod relics;
pub mod rng;
pub mod run;
pub mod tables;
