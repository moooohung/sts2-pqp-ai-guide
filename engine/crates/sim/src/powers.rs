//! Power behaviour (r7 `powers/*.py`), dispatched by `(PowerId, Hook)` in `hooks.rs`.

use crate::card::CId;
use crate::combat::Combat;
use crate::creature::Power;
use crate::tables::PowerId;

/// Powers whose every overridden r7 hook is ported. Applying any other power marks the
/// combat unported (fail closed).
pub const PORTED_POWERS: &[PowerId] = &[
    PowerId::CallOfTheVoid,
    PowerId::ForbiddenGrimoire,
    PowerId::Mayhem,
    PowerId::Stratagem,
    PowerId::Ambergris,
    PowerId::Gigantification,
    PowerId::Oblivion,
    PowerId::SentryMode,
    PowerId::SpiritOfAsh,
    PowerId::SummonNextTurn,
    PowerId::DanseMacabre,
    PowerId::Neurosurge,
    PowerId::Underworld,
    PowerId::Cacophony,
    PowerId::SleightOfFlesh,
    PowerId::EnfeeblingTouch,
    PowerId::Friendship,
    PowerId::NecroMastery,
    PowerId::Shroud,
    PowerId::Haunt,
    PowerId::Soulbound,
    PowerId::Calcify,
    PowerId::Demesne,
    PowerId::DevourLife,
    PowerId::Veilpiercer,
    PowerId::BorrowedTime,
    PowerId::Countdown,
    PowerId::Debilitate,
    PowerId::Pagestorm,
    PowerId::Nostalgia,
    PowerId::Calamity,
    PowerId::Entropy,
    PowerId::Panache,
    PowerId::RollingBoulder,
    // C# Models/Powers: complete combat hooks, including inherited temporary Strength.
    PowerId::Confused,
    PowerId::Fasten,
    PowerId::DarkShackles,
    PowerId::Coordinate,
    PowerId::Fade,
    PowerId::HelicalDart,
    PowerId::ImitationLearning,
    PowerId::Covered,
    PowerId::Intercept,
    PowerId::TagTeam,
    PowerId::Guarded,
    PowerId::SelfFormingClay,
    PowerId::PrepTime,
    PowerId::TheGambit,
    PowerId::Reflect,
    PowerId::ReptileTrinket,
    PowerId::BeaconOfHope,
    PowerId::Knockdown,
    PowerId::DieForYou,
    PowerId::Lethality,
    PowerId::ReaperForm,
    PowerId::Doom,
    PowerId::SicEm,
    PowerId::Hang,
    PowerId::BackAttackLeft,
    PowerId::BackAttackRight,
    PowerId::Barricade,
    PowerId::Colossus,
    PowerId::Corruption,
    PowerId::Cruelty,
    PowerId::Curious,
    PowerId::DemonForm,
    PowerId::Disintegration,
    PowerId::DrawCardsNextTurn,
    PowerId::EnergyNextTurn,
    PowerId::Enrage,
    PowerId::EscapeArtist,
    PowerId::FeedingFrenzy,
    PowerId::FeelNoPain,
    PowerId::FreeAttack,
    PowerId::HardToKill,
    PowerId::Hatch,
    PowerId::HighVoltage,
    PowerId::Juggernaut,
    PowerId::Mangle,
    PowerId::MindRot,
    PowerId::NoDraw,
    PowerId::OneTwoPunch,
    PowerId::Plating,
    PowerId::Pyre,
    PowerId::Rage,
    PowerId::Rampart,
    PowerId::Rebound,
    PowerId::Ritual,
    PowerId::SetupStrike,
    PowerId::Soar,
    PowerId::Strength,
    PowerId::Territorial,
    PowerId::Vicious,
    PowerId::WasteAway,
    PowerId::Dexterity,
    PowerId::Vulnerable,
    PowerId::Weak,
    PowerId::Frail,
    PowerId::Artifact,
    // Tower 2026-10-25: death lifecycle + damage-event hooks.
    PowerId::Shrink,
    PowerId::Ravenous,
    PowerId::CrabRage,
    PowerId::Minion,
    PowerId::Thorns,
    // fire: powers/turn_effects.py:642-674
    PowerId::TheBomb,
    // Packet sim_w8_powers2.
    PowerId::Suck,
    PowerId::FlexPotion,
    PowerId::Slow,
    PowerId::Inferno,
    PowerId::Imbalanced,
    PowerId::Regen,
    PowerId::ShacklingPotion,
    PowerId::HardenedShell,
    PowerId::Skittish,
    PowerId::FlameBarrier,
    PowerId::RetainHand,
    PowerId::Smoggy,
    PowerId::Stampede,
    PowerId::Swipe,
    PowerId::Surrounded,
    PowerId::Reattach,
    PowerId::Adaptable,
    PowerId::Illusion,
    PowerId::SteamEruption,
    PowerId::Stock,
    PowerId::DoorRevival,
    PowerId::VitalSpark,
    PowerId::Tainted,
    PowerId::Infested,
    PowerId::NoBlock,
    PowerId::Automation,
    PowerId::BattlewornDummyTimeLimit,
    PowerId::ChainsOfBinding,
    PowerId::Hex,
    PowerId::Ringing,
    PowerId::Tangled,
    PowerId::ToricToughness,
    PowerId::WitheringPresence,
    PowerId::Sandpit,
    PowerId::CrimsonMantle,
    PowerId::Aggression,
    PowerId::Asleep,
    PowerId::BlockNextTurn,
    PowerId::Clarity,
    PowerId::Duplication,
    PowerId::Shriek,
    PowerId::Slippery,
    PowerId::Intangible,
    PowerId::Radiance,
    PowerId::Thievery,
    PowerId::PainfulStabs,
    PowerId::Vigor,
    PowerId::Burrowed,
    PowerId::Slumber,
    PowerId::Constrict,
    PowerId::MagicBomb,
    PowerId::PossessSpeed,
    PowerId::PossessStrength,
    PowerId::CurlUp,
    PowerId::Demise,
    PowerId::Flutter,
    PowerId::Grapple,
    PowerId::HelloWorld,
    PowerId::PaperCuts,
    PowerId::PersonalHive,
    PowerId::Plow,
    PowerId::Nemesis,
    PowerId::SpeedPotion,
    PowerId::Tank,
    PowerId::Buffer,
    PowerId::Sloth,
    PowerId::Juggling,
    PowerId::Rupture,
    PowerId::DarkEmbrace,
    PowerId::Tender,
    PowerId::Strangle,
    PowerId::Dampen,
    PowerId::Galvanic,
    PowerId::Heist,
    PowerId::Hellraiser,
    PowerId::Improvement,
    PowerId::Surprise,
    PowerId::Unmovable,
    // c1: Silent card powers (powers_chars.rs).
    PowerId::Accelerant,
    PowerId::Accuracy,
    PowerId::Afterimage,
    PowerId::Anticipate,
    PowerId::Blur,
    PowerId::Burst,
    PowerId::Concoct,
    PowerId::CorrosiveWave,
    PowerId::DoubleDamage,
    PowerId::Envenom,
    PowerId::FanOfKnives,
    PowerId::Flanking,
    PowerId::FreeSkill,
    PowerId::InfiniteBlades,
    PowerId::MasterPlanner,
    PowerId::Nightmare,
    PowerId::NoxiousFumes,
    PowerId::PhantomBlades,
    PowerId::PiercingWail,
    PowerId::Poison,
    PowerId::SerpentForm,
    PowerId::ShadowStep,
    PowerId::Shadowmeld,
    PowerId::Sneaky,
    PowerId::Speedster,
    PowerId::TemporaryDexterity,
    PowerId::TheHunt,
    PowerId::ToolsOfTheTrade,
    PowerId::Tracking,
    PowerId::WellLaidPlans,
    PowerId::WraithForm,
    // c2: Defect (powers_defect.rs; Feral, Imitation Learning still unported)
    PowerId::BiasedCognition,
    PowerId::ConsumingShadow,
    PowerId::Coolant,
    PowerId::CreativeAi,
    PowerId::EchoForm,
    // c3: Regent star/forge listeners (crate::regent)
    PowerId::BlackHole,
    PowerId::ChildOfTheStars,
    PowerId::HammerTime,
    PowerId::Conqueror,
    PowerId::CrushUnder,
    PowerId::DyingStar,
    PowerId::StarNextTurn,
    PowerId::Genesis,
    PowerId::Furnace,
    PowerId::TheSealedThrone,
    PowerId::PillarOfCreation,
    PowerId::SpectrumShift,
    PowerId::ForegoneConclusion,
    PowerId::Tyranny,
    PowerId::MonarchsGaze,
    PowerId::MonarchsGazeStrengthDown,
    PowerId::Royalties,
    PowerId::VoidForm,
    PowerId::Orbit,
    PowerId::Monologue,
    PowerId::SwordSage,
    PowerId::PaleBlueDot,
    PowerId::Arsenal,
    PowerId::Parry,
    PowerId::SeekingEdge,
    PowerId::Feral,
    PowerId::Focus,
    PowerId::FocusedStrike,
    PowerId::FreePower,
    PowerId::Hailstorm,
    PowerId::Hibernate,
    PowerId::Hotfix,
    PowerId::HyperbeamFocusDown,
    PowerId::Iteration,
    PowerId::LightningRod,
    PowerId::Loop,
    PowerId::MachineLearning,
    PowerId::OneForAll,
    PowerId::SignalBoost,
    PowerId::Smokestack,
    PowerId::Spinner,
    PowerId::Storm,
    PowerId::Subroutine,
    PowerId::Synchronize,
    PowerId::Thunder,
    PowerId::TrashToTreasure,
];

#[inline]
pub fn is_ported(id: PowerId) -> bool {
    PORTED_POWERS.contains(&id)
}

pub const VULNERABLE_MULTIPLIER: f64 = 1.5;
pub const WEAK_MULTIPLIER: f64 = 0.75;
pub const FRAIL_MULTIPLIER: f64 = 0.75;

impl Combat {
    /// Possess{Strength,Speed}Power.cs:30-54: dictionary keyed by receiver,
    /// insertion order retained independently for every power instance.
    pub(crate) fn record_stolen_power(&mut self, power: crate::creature::PRef, receiver: CId, amount: i32) {
        if let Some(entry) = self.stolen_power_amounts.iter_mut().find(|e| e.0 == power && e.1 == receiver) {
            entry.2 += amount;
        } else {
            self.stolen_power_amounts.push((power, receiver, amount));
        }
    }
    /// C# CreatureCmd.Damage(IEnumerable): damage every target before processing deaths.
    /// These powers have no CardModel source, even when triggered inside a card play.
    pub(crate) fn power_damage_all_enemies(&mut self, owner: CId, amount: i32) {
        let source = self.active_card.take();
        let mut killed = Vec::new();
        for enemy in self.hittable_enemies() {
            let damage = self.calculate_damage(amount, Some(owner), enemy, crate::combat::Props::UNPOWERED);
            self.defer_kill = true;
            self.apply_damage(enemy, damage, crate::combat::Props::UNPOWERED, Some(owner));
            if self.cr(enemy).is_dead() {
                killed.push(enemy);
            }
        }
        for enemy in killed {
            self.kill_creature(enemy);
        }
        self.flush_pending_attack();
        self.active_card = source;
    }

    /// Instance field `slot` of `owner`'s `id` power (`Power::vars`); 0 without that power.
    pub(crate) fn power_private_int(&self, owner: CId, id: PowerId, slot: u8) -> i32 {
        self.find_power(owner, id).map_or(0, |p| self.powers[p as usize].vars[slot as usize])
    }

    pub(crate) fn set_power_private_int(&mut self, owner: CId, id: PowerId, slot: u8, value: i32) {
        match self.find_power(owner, id) {
            Some(p) => self.powers[p as usize].vars[slot as usize] = value,
            None if value != 0 => self.unported("power field without its power"),
            None => {}
        }
    }

    /// Exact compact storage for r7 `AutomationPower._instances`.
    /// Hidden entries never join a creature's power lists, so they do not affect listener order.
    fn automation_hidden(pw: &Power, owner: CId) -> bool {
        pw.owner == u8::MAX
            && pw.id == PowerId::Automation
            && pw.applier == Some(owner)
            && pw.skip_next_tick
            && pw.ignore_next_instance
    }

    pub(crate) fn automation_add_instance(&mut self, owner: CId, amount: i32) {
        if amount <= 0 {
            return;
        }
        if let Some(pw) = self
            .powers
            .iter_mut()
            .find(|pw| Self::automation_hidden(pw, owner) && pw.amount == 0 && pw.payload == 0)
        {
            pw.amount = amount;
            pw.payload = 10;
            return;
        }
        self.powers.push(Power {
            id: PowerId::Automation,
            amount,
            payload: 10,
            skip_next_tick: true,
            applier: Some(owner),
            ignore_next_instance: true,
            owner: u8::MAX,
            vars: [0; 4],
            target: None,
        });
    }

    pub(crate) fn automation_on_draw(&mut self, owner: CId) -> i32 {
        let mut energy = 0;
        let mut display = None;
        for pw in &mut self.powers {
            if !Self::automation_hidden(pw, owner) || pw.amount <= 0 || pw.payload <= 0 {
                continue;
            }
            pw.payload -= 1;
            if pw.payload <= 0 {
                energy += pw.amount;
                pw.payload = 10;
            }
            display = Some(pw.payload);
        }
        if let Some(display) = display {
            self.set_power_private_int(owner, PowerId::Automation, 0, display);
        }
        energy
    }

    pub(crate) fn automation_clear_instances(&mut self, owner: CId) {
        for pw in &mut self.powers {
            if Self::automation_hidden(pw, owner) {
                pw.amount = 0;
                pw.payload = 0;
            }
        }
    }

    #[inline]
    fn dampen_caster_entry(pw: &Power, owner: CId, caster: CId) -> bool {
        pw.owner == u8::MAX
            && pw.id == PowerId::Dampen
            && pw.applier == Some(owner)
            && pw.skip_next_tick
            && pw.ignore_next_instance
            && pw.payload == caster as i32 + 1
            && pw.amount > 0
    }

    pub(crate) fn dampen_add_caster(&mut self, owner: CId, caster: CId) {
        if self
            .powers
            .iter()
            .any(|pw| Self::dampen_caster_entry(pw, owner, caster))
        {
            return;
        }
        self.powers.push(Power {
            id: PowerId::Dampen,
            amount: 1,
            payload: caster as i32 + 1,
            skip_next_tick: true,
            applier: Some(owner),
            ignore_next_instance: true,
            owner: u8::MAX,
            vars: [0; 4],
            target: None,
        });
    }

    /// Removes one r7 Dampen caster and returns whether any caster remains.
    pub(crate) fn dampen_remove_caster(&mut self, owner: CId, caster: CId) -> bool {
        if let Some(pw) = self
            .powers
            .iter_mut()
            .find(|pw| Self::dampen_caster_entry(pw, owner, caster))
        {
            pw.amount = 0;
        }
        self.powers.iter().any(|pw| {
            pw.owner == u8::MAX
                && pw.id == PowerId::Dampen
                && pw.applier == Some(owner)
                && pw.skip_next_tick
                && pw.ignore_next_instance
                && pw.payload > 0
                && pw.amount > 0
        })
    }

    pub(crate) fn dampen_clear_casters(&mut self, owner: CId) {
        for pw in &mut self.powers {
            if pw.owner == u8::MAX
                && pw.id == PowerId::Dampen
                && pw.applier == Some(owner)
                && pw.skip_next_tick
                && pw.ignore_next_instance
                && pw.payload > 0
            {
                pw.amount = 0;
            }
        }
    }

    /// r7 InfernoPower/CrimsonMantlePower `increment_self_damage`.
    pub fn increment_power_self_damage(&mut self, owner: CId, id: PowerId) {
        debug_assert!(matches!(id, PowerId::Inferno | PowerId::CrimsonMantle));
        if self.find_power(owner, id).is_none() {
            return;
        }
        let next = self.power_private_int(owner, id, 0) + 1;
        self.set_power_private_int(owner, id, 0, next);
    }

    #[inline]
    pub(crate) fn power_self_damage(&self, p: crate::creature::PRef) -> i32 {
        let pw = &self.powers[p as usize];
        self.power_private_int(pw.owner, pw.id, 0)
    }
}
