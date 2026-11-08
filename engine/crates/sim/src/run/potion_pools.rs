//! Fully unlocked C# *4Epoch.Potions followed by SharedPotionPool.GenerateAllPotions.
use crate::character::Character;
use crate::tables::PotionId;

pub(crate) fn cards(character: crate::character::Character) -> Option<Vec<PotionId>> {
    let names: &[&str] = match character {
        Character::Ironclad => &["BloodPotion", "SoldiersStew", "Ashwater"],
        Character::Silent => &["PoisonPotion", "GhostInAJar", "CunningPotion"],
        Character::Defect => &["FocusPotion", "EssenceOfDarkness", "PotionOfCapacity"],
        Character::Regent => &["StarPotion", "CosmicConcoction", "KingsCourage"],
        Character::Necrobinder => &["PotionOfDoom", "PotOfGhouls", "BoneBrew"],
    };
    const SHARED: &[&str] = &[
        "AttackPotion", "BeetleJuice", "BlessingOfTheForge", "BlockPotion",
        "BottledPotential", "Clarity", "ColorlessPotion", "CureAll", "DexterityPotion",
        "DistilledChaos", "DropletOfPrecognition", "Duplicator", "EnergyPotion",
        "EntropicBrew", "ExplosiveAmpoule", "FairyInABottle", "FirePotion", "FlexPotion",
        "Fortifier", "FruitJuice", "FyshOil", "GamblersBrew", "GigantificationPotion",
        "HeartOfIron", "LiquidBronze", "LiquidMemories", "LuckyTonic", "MazalethsGift",
        "OrobicAcid", "PotionOfBinding", "PowderedDemise", "PowerPotion",
        "RadiantTincture", "RegenPotion", "ShacklingPotion", "ShipInABottle",
        "SkillPotion", "SneckoOil", "SpeedPotion", "StableSerum", "StrengthPotion",
        "SwiftPotion", "TouchOfInsanity", "VulnerablePotion", "WeakPotion",
    ];
    names.iter().chain(SHARED).map(|name| PotionId::from_name(name)).collect()
}

#[test]
fn unlocked_pools_have_owner_epoch_then_shared_source_order() {
    for (owner, first) in [
        (Character::Ironclad, "BloodPotion"), (Character::Silent, "PoisonPotion"),
        (Character::Defect, "FocusPotion"), (Character::Regent, "StarPotion"),
        (Character::Necrobinder, "PotionOfDoom"),
    ] {
        let pool = cards(owner).unwrap();
        assert_eq!(pool.len(), 48);
        assert_eq!(pool[0], PotionId::from_name(first).unwrap());
        assert_eq!(pool[3], PotionId::from_name("AttackPotion").unwrap());
        assert_eq!(pool[47], PotionId::from_name("WeakPotion").unwrap());
        assert_eq!(pool.iter().collect::<std::collections::HashSet<_>>().len(), 48);
    }
}
