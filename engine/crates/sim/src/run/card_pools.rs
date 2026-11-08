//! C# Models/CardPools/*CardPool.GenerateAllCards, in source order.
//! These are the fully-unlocked run pools, not CardFactory.FilterForCombat.
//! Simulator aliases were matched through data/cards.json's canonical `entry`.
use crate::character::CardPool;

use crate::card::Card;
use crate::tables::{CardId, CardRarity, CardType, CARD_MP};

const IRONCLAD: &[&str] = &[
    "AGGRESSION_CARD", "ANGER", "ARMAMENTS", "ASHEN_STRIKE", "BARRICADE_CARD", "BASH",
    "BATTLE_TRANCE", "BLAZE", "BLOOD_WALL", "BLOODLETTING", "BLUDGEON", "BODY_SLAM",
    "BRAND", "BREAK", "BREAKTHROUGH", "BULLY", "BURNING_PACT", "CASCADE",
    "CINDER", "COLOSSUS_CARD", "CONFLAGRATION", "CORRUPTION_CARD", "CRIMSON_MANTLE", "CRUELTY_CARD",
    "DARK_EMBRACE_CARD", "DEFEND_IRONCLAD", "DEMON_FORM_CARD", "DEMONIC_SHIELD", "DISMANTLE", "DOMINATE",
    "DRUM_OF_BATTLE_CARD", "EVIL_EYE", "EXPECT_A_FIGHT", "FEED", "FEEL_NO_PAIN_CARD", "FIEND_FIRE",
    "FIGHT_ME", "FLAME_BARRIER_CARD", "FORGOTTEN_RITUAL", "HAVOC", "HEADBUTT", "HELLRAISER_CARD",
    "HEMOKINESIS", "HOWL_FROM_BEYOND", "IMPERVIOUS", "INFERNAL_BLADE", "INFERNO_CARD", "INFLAME",
    "IRON_WAVE", "JUGGERNAUT_CARD", "JUGGLING_CARD", "MANGLE", "MIDNIGHT", "MOLTEN_FIST",
    "NOT_YET", "OFFERING", "ONE_TWO_PUNCH_CARD", "OUTRAGE", "PACTS_END", "PERFECTED_STRIKE",
    "PILLAGE", "POMMEL_STRIKE", "PRIMAL_FORCE", "PYRE", "RAGE_CARD", "RAMPAGE",
    "RUPTURE_CARD", "SECOND_WIND", "SETUP_STRIKE_CARD", "SHRUG_IT_OFF", "SPITE", "STAMPEDE_CARD",
    "STOKE", "STOMP", "STONE_ARMOR", "STRIKE_IRONCLAD", "SWORD_BOOMERANG", "TANK_CARD",
    "TAUNT", "TEAR_ASUNDER", "THRASH", "THUNDERCLAP", "TREMBLE", "TRUE_GRIT",
    "TWIN_STRIKE", "UNMOVABLE", "UNRELENTING", "UPPERCUT", "VICIOUS_CARD", "WHIRLWIND",
];
const SILENT: &[&str] = &[
    "ABRASIVE", "ACCELERANT", "ACCURACY_CARD", "ACROBATICS", "ADRENALINE", "AFTERIMAGE_CARD",
    "ANTICIPATE", "ASSASSINATE", "BACKFLIP", "BACKSTAB", "BLADE_OF_INK", "BLADE_DANCE",
    "BLADE_SYMPHONY", "BLUR_CARD", "BOUNCING_FLASK", "BUBBLE_BUBBLE", "BULLET_TIME", "BURST",
    "CALCULATED_GAMBLE", "CLOAK_AND_DAGGER", "CORROSIVE_WAVE", "CONCOCT", "DAGGER_SPRAY", "DAGGER_THROW",
    "DASH", "DEADLY_POISON", "DEFEND_SILENT", "DEFLECT", "DODGE_AND_ROLL", "ECHOING_SLASH",
    "ENVENOM_CARD", "ESCAPE_PLAN", "EXPERTISE", "EXPOSE", "FADE", "FAN_OF_KNIVES_CARD",
    "FINISHER", "FLANKING", "FLECHETTES", "FLICK_FLACK", "SIDESTEP", "FOOTWORK",
    "GRAND_FINALE", "HAND_TRICK", "HAZE", "HIDDEN_DAGGERS", "INFINITE_BLADES_CARD", "KNIFE_TRAP",
    "LEADING_STRIKE", "LEG_SWEEP", "MALAISE", "MASTER_PLANNER", "MEMENTO_MORI", "MIRAGE",
    "MURDER", "NEUTRALIZE", "NIGHTMARE", "NOXIOUS_FUMES_CARD", "OUTBREAK_CARD", "PHANTOM_BLADES_CARD",
    "PIERCING_WAIL", "PINPOINT", "POISONED_STAB", "POUNCE", "PRECISE_CUT", "PREDATOR",
    "PREPARED", "REFLEX", "RICOCHET", "SERPENT_FORM_CARD", "SHADOW_STEP", "SHADOWMELD",
    "SKEWER", "SLICE", "SNAKEBITE", "SNEAKY_CARD", "SPEEDSTER_CARD", "STORM_OF_STEEL",
    "STRANGLE", "STRIKE_SILENT", "SUCKER_PUNCH", "SUPPRESS", "SURVIVOR", "TACTICIAN",
    "THE_HUNT", "TOOLS_OF_THE_TRADE", "TRACKING", "UNTOUCHABLE", "UP_MY_SLEEVE", "WELL_LAID_PLANS",
    "WRAITH_FORM",
];
const DEFECT: &[&str] = &[
    "ADAPTIVE_STRIKE", "ALL_FOR_ONE", "BALL_LIGHTNING", "BARRAGE", "BEAM_CELL", "BIASED_COGNITION_CARD",
    "BOOST_AWAY", "BOOT_SEQUENCE", "BUFFER_CARD", "BULK_UP", "CAPACITOR", "CHAOS",
    "CHARGE_BATTERY", "CHILL", "CLAW", "COLD_SNAP", "COMPACT", "COMPILE_DRIVER",
    "CONSUMING_SHADOW", "COOLANT", "COOLHEADED", "CREATIVE_AI_CARD", "DARKNESS_CARD", "DEFEND_DEFECT",
    "DEFRAGMENT", "DOUBLE_ENERGY", "DUALCAST", "ECHO_FORM_CARD", "ENERGY_SURGE", "FERAL",
    "FIGHT_THROUGH", "FLAK_CANNON", "FOCUSED_STRIKE_CARD", "FTL", "FUSION", "GENETIC_ALGORITHM",
    "GLACIER", "GLASSWORK", "GO_FOR_THE_EYES", "GUNK_UP", "HAILSTORM", "HELIX_DRILL",
    "HIBERNATE", "HOLOGRAM", "HOTFIX", "HYPERBEAM", "ICE_LANCE", "IGNITION",
    "IMITATION_LEARNING", "ITERATION_CARD", "LEAP", "LIGHTNING_ROD", "LOOP_CARD", "MACHINE_LEARNING_CARD",
    "METEOR_STRIKE", "MODDED", "MOMENTUM_STRIKE", "MULTI_CAST", "NULL_CARD", "ONE_FOR_ALL",
    "OVERCLOCK", "QUADCAST", "RAINBOW", "REBOOT", "REFRACT", "ROCKET_PUNCH",
    "SCAVENGE", "SCRAPE", "SHADOW_SHIELD", "SHATTER", "SIGNAL_BOOST", "SKIM",
    "SMOKESTACK", "SPINNER_CARD", "STORM_CARD", "STRIKE_DEFECT", "SUBROUTINE", "SUNDER",
    "SUPERCRITICAL", "SWEEPING_BEAM", "SYNCHRONIZE", "SYNTHESIS", "TEMPEST", "TESLA_COIL",
    "THUNDER_CARD", "TRASH_TO_TREASURE", "TURBO", "UPROAR", "VOLTAIC", "WHITE_NOISE",
    "ZAP",
];
const REGENT: &[&str] = &[
    "ALIGNMENT", "ARSENAL", "ASTRAL_PULSE", "BEAT_INTO_SHAPE", "BEGONE", "BIG_BANG",
    "BLACK_HOLE", "BOMBARDMENT", "BULWARK", "BUNDLE_OF_JOY", "CELESTIAL_MIGHT", "CHARGE",
    "CHILD_OF_THE_STARS", "CLOAK_OF_STARS", "COLLISION_COURSE", "COMET", "CONQUEROR", "CONSTELLATION",
    "CONVERGENCE", "COSMIC_INDIFFERENCE", "CRASH_LANDING", "CRESCENT_SPEAR", "CRUSH_UNDER", "DECISIONS_DECISIONS",
    "DEFEND_REGENT", "DEVASTATE", "DYING_STAR", "FALLING_STAR", "FOREGONE_CONCLUSION", "FURNACE",
    "GAMMA_BLAST", "GATHER_LIGHT", "GENESIS", "GLIMMER", "GLITTERSTREAM", "GLOW",
    "GUARDS", "GUIDING_STAR", "HAMMER_TIME", "HEAVENLY_DRILL", "HEGEMONY", "HEIRLOOM_HAMMER",
    "HIDDEN_CACHE", "I_AM_INVINCIBLE", "KINGLY_KICK", "KINGLY_PUNCH", "KNOCKOUT_BLOW", "KNOW_THY_PLACE",
    "LARGESSE", "LUNAR_BLAST", "MAKE_IT_SO", "MANIFEST_AUTHORITY", "METEOR_SHOWER", "MONARCHS_GAZE_CARD",
    "MONOLOGUE_CARD", "NEUTRON_AEGIS", "ORBIT", "PALE_BLUE_DOT", "PARRY_CARD", "PARTICLE_WALL",
    "PATTER", "PHOTON_CUT", "PLOT", "PILLAR_OF_CREATION", "PROPHESIZE", "QUASAR",
    "RADIATE", "REFINE_BLADE", "REFLECT_CARD", "RESONANCE", "ROYAL_GAMBLE", "ROYALTIES_CARD",
    "SEEKING_EDGE", "SEVEN_STARS", "SHINING_STRIKE", "SOLAR_STRIKE", "SPECTRUM_SHIFT", "SPOILS_OF_BATTLE",
    "STARDUST", "STRIKE_REGENT", "SUMMON_FORTH", "SUPERMASSIVE", "SWORD_SAGE", "TERRAFORMING",
    "THE_SEALED_THRONE", "THE_SMITH", "TUTOR", "TYRANNY_CARD", "VENERATE", "VOID_FORM",
    "WROUGHT_IN_WAR",
];
const NECROBINDER: &[&str] = &[
    "AFTERLIFE", "BANSHEES_CRY", "BLIGHT_STRIKE", "BODYGUARD", "BONE_SHARDS", "BORROWED_TIME",
    "BURY", "CACOPHONY", "CALCIFY_CARD", "CALL_OF_THE_VOID", "CAPTURE_SPIRIT", "CLEANSE",
    "COUNTDOWN_CARD", "DANSE_MACABRE", "DEATH_MARCH", "DEATHBRINGER", "DEATHS_DOOR", "DEBILITATE_CARD",
    "DEFEND_NECROBINDER", "DEFILE", "DEFY", "DELAY", "DEMESNE", "DEVOUR_LIFE_CARD",
    "DIRGE", "DRAIN_POWER", "DREDGE", "EIDOLON", "END_OF_DAYS", "ENFEEBLING_TOUCH",
    "ERADICATE", "FEAR", "FETCH", "FLATTEN", "FORBIDDEN_GRIMOIRE", "FRIENDSHIP",
    "GLIMPSE_BEYOND", "GRAVE_WARDEN", "GRAVEBLAST", "HANG", "HAUNT", "HIGH_FIVE",
    "INVOKE", "LEGION_OF_BONE", "LETHALITY_CARD", "MELANCHOLY", "MISERY", "NECRO_MASTERY_CARD",
    "NEGATIVE_PULSE", "NEUROSURGE", "NO_ESCAPE", "OBLIVION", "PAGESTORM", "PARSE",
    "POKE", "PROTECTOR", "PULL_AGGRO", "PULL_FROM_BELOW", "PUTREFY", "RATTLE",
    "REANIMATE", "REAP", "REAPER_FORM", "REAVE", "RIGHT_HAND_HAND", "SACRIFICE",
    "SCOURGE", "SCULPTING_STRIKE", "SEANCE", "SENTRY_MODE", "SEVERANCE", "SHARED_FATE",
    "SHROUD", "SIC_EM", "SLEIGHT_OF_FLESH", "SNAP", "SOUL_STORM", "SOULBOUND",
    "SOW", "SPIRIT_OF_ASH", "SPUR", "SQUEEZE", "STRIKE_NECROBINDER", "THE_SCYTHE",
    "TIMES_UP", "TRANSFIGURE", "UNDEATH", "UNDERWORLD", "UNLEASH", "VEILPIERCER",
    "WISP",
];
const COLORLESS: &[&str] = &[
    "ALCHEMIZE", "ANOINTED", "AUTOMATION", "BEACON_OF_HOPE", "BEAT_DOWN", "BELIEVE_IN_YOU",
    "BOLAS", "CALAMITY_CARD", "CATASTROPHE", "COORDINATE_CARD", "DARK_SHACKLES", "DISCOVERY",
    "DRAMATIC_ENTRANCE", "ENTROPY", "EQUILIBRIUM", "ETERNAL_ARMOR", "FASTEN", "FINESSE",
    "FISTICUFFS", "FLASH_OF_STEEL", "GANG_UP", "GOLD_AXE", "HAND_OF_GREED", "HIDDEN_GEM",
    "HUDDLE_UP", "IMPATIENCE", "INTERCEPT_CARD", "JACK_OF_ALL_TRADES", "JACKPOT", "KNOCKDOWN",
    "LIFT", "MASTER_OF_STRATEGY", "MAYHEM_CARD", "MIMIC", "MIND_BLAST", "NOSTALGIA_CARD",
    "OMNISLICE", "PANACHE_CARD", "PANIC_BUTTON", "PREP_TIME", "PRODUCTION", "PROLONG",
    "PROWESS", "PURITY", "RALLY", "REND", "RESTLESSNESS", "ROLLING_BOULDER", "SALVO",
    "SCRAWL", "SECRET_TECHNIQUE", "SECRET_WEAPON", "SEEKER_STRIKE", "SHOCKWAVE", "SPLASH",
    "STRATAGEM", "TAG_TEAM", "THE_BALL", "THE_BOMB_CARD", "THE_GAMBIT", "THINKING_AHEAD",
    "THRUMMING_HATCHET", "ULTIMATE_DEFEND", "ULTIMATE_STRIKE", "VOLLEY",
];
const CURSE: &[&str] = &[
    "ASCENDERS_BANE", "BAD_LUCK", "CLUMSY", "CURSE_OF_THE_BELL", "DEBT", "DECAY",
    "DOUBT", "ENTHRALLED", "FOLLY", "GREED", "GUILTY", "INJURY", "NORMALITY",
    "POOR_SLEEP", "REGRET", "SHAME", "SPORE_MIND", "WRITHE",
];
const STATUS: &[&str] = &[
    "BECKON", "BURN", "DAZED", "DEBRIS", "FRANTIC_ESCAPE", "INFECTION",
    "WITHER", "SLIMED", "SOOT", "TOXIC", "VOID", "WOUND",
];
// C# TokenCardPool.GenerateAllCards. The r7 schema represents these with STATUS
// rarity, so distinguish the source-format token models from real status cards.
const TOKEN: &[&str] = &[
    "DISINTEGRATION", "FUEL", "GIANT_ROCK", "LUMINESCE", "MIND_ROT",
    "MINION_DIVE_BOMB", "MINION_SACRIFICE", "MINION_STRIKE", "SHIV", "SLOTH_STATUS",
    "SOUL", "SOVEREIGN_BLADE", "SWEEPING_GAZE", "WASTE_AWAY",
];

const POOLS: &[(crate::character::CardPool, &[&str])] = &[
    (crate::character::CardPool::Ironclad, IRONCLAD), (crate::character::CardPool::Silent, SILENT), (crate::character::CardPool::Defect, DEFECT),
    (crate::character::CardPool::Regent, REGENT), (crate::character::CardPool::Necrobinder, NECROBINDER), (crate::character::CardPool::Colorless, COLORLESS),
    (crate::character::CardPool::Curse, CURSE), (crate::character::CardPool::Status, STATUS),
];

pub(super) fn cards(pool: crate::character::CardPool, multiplayer: bool) -> Result<Vec<CardId>, &'static str> {
    let names = POOLS.iter().find(|(name, _)| *name == pool)
        .ok_or("unported noncombat card pool")?.1;
    Ok(names.iter().map(|name| CardId::from_name(name).expect("C# pool member"))
        // C# CardPoolModel.GetUnlockedCards' player-count constraint.
        .filter(|id| CARD_MP[id.index()] != if multiplayer { 2 } else { 1 })
        .collect())
}

pub(super) fn transformations(original: &Card, multiplayer: bool) -> Result<Vec<CardId>, &'static str> {
    // C# CardFactory.GetDefaultTransformationOptions/GetFilteredTransformationOptions.
    let token = TOKEN.contains(&original.id.name());
    let pool = if original.card_type == CardType::Quest
        || token || matches!(original.rarity, CardRarity::Event | CardRarity::Ancient) {
        CardPool::Colorless
    } else {
        POOLS.iter().find(|(_, names)| names.contains(&original.id.name()))
            .ok_or("unported original card pool")?.0
    };
    Ok(cards(pool, multiplayer)?.into_iter().filter(|id| *id != original.id)
        .filter(|id| {
            (!token && matches!(original.rarity, CardRarity::Curse | CardRarity::Status))
                || crate::tables::card_info(*id, false).is_some_and(|info|
                    matches!(info.rarity, CardRarity::Common | CardRarity::Uncommon | CardRarity::Rare))
        }).collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn run_pools_include_noncombat_cards_and_keep_original_character_and_curses() {
        for &(pool, _) in POOLS {
            assert!(cards(pool, false).unwrap().iter()
                .all(|id| crate::tables::card_info(*id, false).is_some()));
        }
        assert!(cards(CardPool::Ironclad, false).unwrap().contains(&CardId::NotYet));
        for (original, pool) in [(CardId::Acrobatics, CardPool::Silent), (CardId::Decay, CardPool::Curse)] {
            let original = Card::from_factory(original, false, 1).unwrap();
            let options = transformations(&original, false).unwrap();
            assert!(!options.contains(&original.id));
            let expected = cards(pool, false).unwrap();
            assert!(options.iter().all(|id| expected.contains(id)));
            assert!(!options.is_empty());
        }
        assert!(cards(CardPool::Colorless, false).unwrap().iter().all(|id| CARD_MP[id.index()] != 1));
        let colorless = cards(CardPool::Colorless, false).unwrap();
        for token in TOKEN {
            let original = Card::from_factory(CardId::from_name(token).unwrap(), false, 1).unwrap();
            let token_options = transformations(&original, false).unwrap();
            assert!(!token_options.is_empty());
            assert!(token_options.iter().all(|id| colorless.contains(id)
                && matches!(crate::tables::card_info(*id, false).unwrap().rarity,
                    CardRarity::Common | CardRarity::Uncommon | CardRarity::Rare)));
        }
    }
}
