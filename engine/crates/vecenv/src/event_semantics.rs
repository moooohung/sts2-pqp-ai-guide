//! Frozen structured_simulator_teacher.py::_visible_event_semantics.
//! Cached linear-time regex plus an explicit noun grammar avoids backtracking.
use regex::Regex;
use std::sync::OnceLock;
use sts2_sim::{obs::{ObsBuffers, ObsError}, run::{RunAction, RunActionKind, RunManager}};

fn patterns() -> &'static Vec<Regex> {
    static RE: OnceLock<Vec<Regex>> = OnceLock::new();
    RE.get_or_init(|| [
        r"\b(?:if|chance|may|might|could)\b|\d+(?:\.\d+)?\s*%",
        r"\b(?:take|lose|pay)\s+(\d{1,4})\s*(?:hp|damage)\b",
        r"\bheal\s+(\d{1,4})\s*hp\b",
        r"(?:^|[^\w])([+-]\d{1,4})\s*max\s*hp\b",
        r"\bgain\s+(\d{1,4})\s*max\s*hp\b",
        r"\blose\s+(\d{1,4})\s*max\s*hp\b",
        r"\b(?:pay|lose)\s+(\d{1,6})\s*(?:g|gold)\b",
        r"\bgain\s+(\d{1,6})\s*(?:g|gold)\b",
        r"\bremove\s+(?:(\d{1,3})|a|one)\s+cards?\b",
        r"\bupgrade\s+(?:(\d{1,3})|a|one)\s+cards?\b",
        r"(?:\bgain\b|(?:\+|,|\band\b)\s*)\s*(?:(\d{1,3})|a|one)\s+",
        r"^([A-Za-z][A-Za-z0-9_-]*)(\s+|$|[^\w])",
    ].iter().map(|p| Regex::new(&format!("(?i){p}")).expect("frozen regex")).collect())
}
fn conditional(text: &str, start: usize, end: usize) -> bool {
    let lo = text[..start].rfind(['.', ';']).map_or(0, |i| i + 1);
    let hi = text[end..].find(['.', ';']).map_or(text.len(), |i| end + i);
    patterns()[0].is_match(&text[lo..hi])
}
fn integer(text: &str, pattern: usize, conditional_guard: bool) -> Option<i32> {
    patterns()[pattern].captures_iter(text).find_map(|c| {
        let m = c.get(0).unwrap();
        (!conditional_guard || !conditional(text, m.start(), m.end()))
            .then(|| c.get(1).map_or(Some(1), |v| v.as_str().parse().ok())).flatten()
    })
}
fn gain(text: &str, noun: &str) -> Option<i32> {
    for c in patterns()[10].captures_iter(text) {
        let head = c.get(0).unwrap();
        let mut offset = head.end();
        for modifiers in 0..=4 {
            let Some(word) = patterns()[11].captures(&text[offset..]) else { break; };
            let w = word.get(1).unwrap();
            let value = w.as_str().to_ascii_lowercase();
            let end = offset + w.end();
            if value == noun || value == format!("{noun}s") {
                if !conditional(text, head.start(), end) {
                    return c.get(1).map_or(Some(1), |v| v.as_str().parse().ok());
                }
                break;
            }
            if modifiers == 4 || matches!(value.as_str(),
                "gold"|"g"|"hp"|"max"|"damage"|"relic"|"relics"|"potion"|"potions")
            { break; }
            let separator = word.get(2).unwrap().as_str();
            if separator.is_empty() || !separator.chars().all(char::is_whitespace) { break; }
            offset += word.get(0).unwrap().end();
        }
    }
    None
}

fn identity(id: &str) -> Result<&'static str, ObsError> {
    // Exact events/{act1,act2,act3,shared}.py event_id declarations.
    // Typed-offer events remain guarded: their targets cannot be inferred
    // from the option label or from an ordinal option ID.
    Ok(match id {
        "BRAIN_LEECH" => "BrainLeech",
        "SELF_HELP_BOOK" => "SelfHelpBook",
        "SAPPHIRE_SEED" => "SapphireSeed",
        "JUNGLE_MAZE_ADVENTURE" => "JungleMazeAdventure",
        "TABLET_OF_TRUTH" => "TabletOfTruth",
        "WHISPERING_HOLLOW" => "WhisperingHollow",
        "LUMINOUS_CHOIR" => "LuminousChoir",
        "ROOM_FULL_OF_CHEESE" => "RoomFullOfCheese",
        "THE_LEGENDS_WERE_TRUE" => "TheLegendsWereTrue",
        "TEA_MASTER" => "TeaMaster",
        "TRASH_HEAP" => "TrashHeap",
        "AMALGAMATOR" => "Amalgamator",
        "AROMA_OF_CHAOS" => "AromaOfChaos",
        "BATTLEWORN_DUMMY" => "BattlewornDummy",
        "BUGSLAYER" => "Bugslayer",
        "BYRDONIS_NEST" => "ByrdonisNest",
        "COLOSSAL_FLOWER" => "ColossalFlower",
        "DENSE_VEGETATION" => "DenseVegetation",
        "DOORS_OF_LIGHT_AND_DARK" => "DoorsOfLightAndDark",
        "DROWNING_BEACON" => "DrowningBeacon",
        "GRAVE_OF_THE_FORGOTTEN" => "GraveOfTheForgotten",
        "HUNGRY_FOR_MUSHROOMS" => "HungryForMushrooms",
        "LOST_WISP" => "LostWisp",
        "MORPHIC_GROVE" => "MorphicGrove",
        "POTION_COURIER" => "PotionCourier",
        "SPIRALING_WHIRLPOOL" => "SpiralingWhirlpool",
        "SUNKEN_STATUE" => "SunkenStatue",
        "SUNKEN_TREASURY" => "SunkenTreasury",
        "SYMBIOTE" => "Symbiote",
        "UNREST_SITE" => "UnrestSite",
        "WATERLOGGED_SCRIPTORIUM" => "WaterloggedScriptorium",
        "WOOD_CARVINGS" => "WoodCarvings",
        "ZEN_WEAVER" => "ZenWeaver",
        "THIS_OR_THAT" => "ThisOrThat",
        "ABYSSAL_BATHS" => "AbyssalBaths",
        "WELLSPRING" => "Wellspring",
        "TEZCATARA" => "Tezcatara",
        "OROBAS" => "Orobas",
        "PAEL" => "Pael",
        "DARV" => "Darv",
        "TANX" => "Tanx",
        "ROUND_TEA_PARTY" => "RoundTeaParty",
        "SLIPPERY_BRIDGE" => "SlipperyBridge",
        "PUNCH_OFF" => "PunchOff",
        "ENDLESS_CONVEYOR" => "EndlessConveyor",
        "THE_FUTURE_OF_POTIONS" => "TheFutureOfPotions",
        "CRYSTAL_SPHERE" => "CrystalSphere",
        "DOLL_ROOM" => "DollRoom",
        "FIELD_OF_MAN_SIZED_HOLES" => "FieldOfManSizedHoles",
        "INFESTED_AUTOMATON" => "InfestedAutomaton",
        "SPIRIT_GRAFTER" => "SpiritGrafter",
        "THE_LANTERN_KEY" => "TheLanternKey",
        "COLORFUL_PHILOSOPHERS" => "ColorfulPhilosophers",
        "FAKE_MERCHANT" => "FakeMerchant",
        "RELIC_TRADER" => "RelicTrader",
        "STONE_OF_ALL_TIME" => "StoneOfAllTime",
        "RANWID_THE_ELDER" => "RanwidTheElder",
        "VAKUU" => "Vakuu",
        "NONUPEIPE" => "Nonupeipe",
        "WELCOME_TO_WONGOS" => "WelcomeToWongos",
        _ => return Err(ObsError::UnsupportedState("event identity/target metadata is unported")),
    })
}

pub(crate) fn write(run: &RunManager, actions: &[RunAction], b: &mut ObsBuffers<'_>) -> Result<(), ObsError> {
    write_inner(run,actions,b,false)
}

#[cfg(feature="selector-proof")]
pub fn write_event_proof(run:&RunManager, actions:&[RunAction], b:&mut ObsBuffers<'_>) -> Result<(),ObsError> {
    write_inner(run,actions,b,true)
}

// Ordered option IDs, legality and all text consumed by the wire writer are
// part of the proof target. Inventory candidates also keep their slot/model.
pub fn event_option_table_shape(run:&RunManager, actions:&[RunAction]) -> Option<String> {
    Some(option_table_key(run.current_event_id()?,actions))
}

fn option_table_key(id:&str, actions:&[RunAction]) -> String {
    let table:Vec<_>=actions.iter().map(|a|
        (a.kind,a.option_id,a.enabled,a.label,a.description_text(),a.potion_id,a.slot_index)).collect();
    format!("event:{id};options={table:?}")
}

#[cfg(test)]
fn table_admitted(key:&str, index:&str) -> bool {
    index.lines().filter_map(|l|l.split_once('\t')).any(|(k,_)|k==key)
}

fn metadata_requires_table_proof(id: &str) -> bool {
    matches!(id, "CRYSTAL_SPHERE" | "DOLL_ROOM" | "FIELD_OF_MAN_SIZED_HOLES"
        | "INFESTED_AUTOMATON" | "SPIRIT_GRAFTER" | "THE_LANTERN_KEY" | "COLORFUL_PHILOSOPHERS"
        | "LUMINOUS_CHOIR" | "FAKE_MERCHANT" | "RELIC_TRADER" | "STONE_OF_ALL_TIME"
        | "RANWID_THE_ELDER" | "VAKUU" | "NONUPEIPE" | "WELCOME_TO_WONGOS")
}

// Same whole-suffix contract as sel_events 0e199d6; preserves holes, order,
// AnyTime eligibility and player target. Never compose per-row certificates.
fn potion_suffix_proven(run: &RunManager, actions: &[RunAction]) -> bool {
    potion_inventory_suffix_proven(&run.run_ctx().potions, actions)
}

fn potion_inventory_suffix_proven(potions: &[Option<sts2_sim::tables::PotionId>], actions: &[RunAction]) -> bool {
    use sts2_sim::tables::{POTION_INFO, PotionId, PotionTargetType, PotionUsageType};
    let mut ordinal = 0;
    for (slot, potion) in potions.iter().copied().enumerate() {
        let Some(id) = potion else { continue };
        let info = &POTION_INFO[id.index()];
        if info.usage == PotionUsageType::AnyTime && id != PotionId::Foulpotion {
            let Some(action) = actions.get(ordinal) else { return false };
            if action.kind != RunActionKind::UsePotion || !action.enabled
                || action.slot_index != Some(slot) || action.potion_id != Some(id)
                || action.option_id.is_some() || action.label.is_some()
                || action.description_text().is_some()
                || action.target_player_id != if info.target == PotionTargetType::AnyPlayer { Some(1) } else { None }
            { return false; }
            ordinal += 1;
        }
        let Some(action) = actions.get(ordinal) else { return false };
        if action.kind != RunActionKind::DiscardPotion || !action.enabled
            || action.slot_index != Some(slot) || action.potion_id != Some(id)
            || action.option_id.is_some() || action.label.is_some()
            || action.description_text().is_some() || action.target_player_id.is_some()
        { return false; }
        ordinal += 1;
    }
    ordinal == actions.len()
}

fn metadata_row_proven(action: &RunAction, option: &str, label: &str, text: &str) -> bool {
    metadata_target_row_proven(action, option, label, text, None)
}

fn metadata_target_row_proven(action: &RunAction, option: &str, label: &str, text: &str,
    relic: Option<sts2_sim::tables::RelicId>) -> bool {
    action.kind == RunActionKind::EventChoice && action.enabled
        && action.option_id == Some(option) && action.label == Some(label)
        && action.description_text().as_deref() == Some(text)
        && action.card_id.is_none() && action.relic_id == relic
        && action.potion_id.is_none() && action.slot_index.is_none()
        && action.target_player_id.is_none() && action.target_index.is_none()
}

fn choir_rows_proven(gold: i32, price: i32, actions: &[RunAction]) -> bool {
    if !(100..=149).contains(&price) || actions.len() != if gold >= price { 2 } else { 1 } {
        return false;
    }
    metadata_row_proven(&actions[0], "reach", "Reach Into the Flesh", "Remove 2 cards, gain Spore Mind curse")
        && (gold < price || (metadata_row_proven(&actions[1], "tribute", "Offer Tribute", "Gain a relic")
            && actions[1].price == Some(price)))
}

fn ranwid_rows_proven(potion:bool,relic:bool,actions:&[RunAction])->bool {
    let rows=[
        ("potion","Give a Potion","Lose a potion, gain a relic",potion),
        ("gold","Give 100 Gold","Lose 100g, gain a relic",true),
        ("relic","Give a Relic","Lose a relic, gain 2 relics",relic)];
    let wanted:Vec<_>=rows.iter().filter(|r|r.3).collect();
    actions.len()==wanted.len() && actions.iter().zip(wanted).all(|(a,r)|
        metadata_row_proven(a,r.0,r.1,r.2))
}

fn ancient_offer_rows_proven(id:&str,relics:&[sts2_sim::tables::RelicId],
    bracelet:bool,actions:&[RunAction])->bool {
    use sts2_sim::tables::RelicId as R;
    if relics.len()!=3 || actions.len()!=3 {return false;}
    let vakuu:&[&[(R,&str)]]=&[
        &[(R::BloodSoakedRose,"Blood-Soaked Rose"),(R::WhisperingEarring,"Whispering Earring"),(R::Fiddle,"Fiddle")],
        &[(R::PreservedFog,"Preserved Fog"),(R::SereTalon,"Sere Talon"),(R::DistinguishedCape,"Distinguished Cape")],
        &[(R::ChoicesParadox,"Choices Paradox"),(R::MusicBox,"Music Box"),(R::LordsParasol,"Lord's Parasol"),(R::JeweledMask,"Jeweled Mask")]];
    let nonupeipe=&[(R::BlessedAntler,"Blessed Antler"),(R::BrilliantScarf,"Brilliant Scarf"),
        (R::DelicateFrond,"Delicate Frond"),(R::DiamondDiadem,"Diamond Diadem"),
        (R::FurCoat,"Fur Coat"),(R::Glitter,"Glitter"),(R::JewelryBox,"Jewelry Box"),
        (R::LoomingFruit,"Looming Fruit"),(R::SignetRing,"Signet Ring"),
        (R::BeautifulBracelet,"Beautiful Bracelet")];
    for (index,(&relic,action)) in relics.iter().zip(actions).enumerate() {
        let pool=if id=="VAKUU" {vakuu[index]} else if id=="NONUPEIPE" {
            &nonupeipe[..if bracelet {10}else{9}]
        } else {return false};
        let Some((_,label))=pool.iter().find(|(r,_)|*r==relic) else {return false};
        let text=if id=="VAKUU" {"Gain a relic"} else {"Gain an ancient relic"};
        if relics[..index].contains(&relic) || !metadata_row_proven(action,
            ["relic_1","relic_2","relic_3"][index],label,text) {return false;}
    }
    true
}

fn trader_rows_proven(offers: &[(sts2_sim::tables::RelicId, sts2_sim::tables::RelicId)],
    actions: &[RunAction]) -> bool {
    // All offers have concrete owned/new models from GetRelicHoverTips. Neither
    // model is inferred from TOP/MIDDLE/BOTTOM, and no unchosen pickup is exposed.
    if offers.is_empty() {
        return actions.len() == 1 && metadata_row_proven(&actions[0], "proceed", "Proceed", "");
    }
    offers.len() <= 3 && offers.len() == actions.len()
        && actions.iter().zip(["trade_0","trade_1","trade_2"]).enumerate().all(|(i,(a,id))|
            metadata_row_proven(a, id, ["Trade Relic 1","Trade Relic 2","Trade Relic 3"][i],
                "Swap an owned relic for a new one"))
}

fn stone_rows_proven(potion: Option<(usize, sts2_sim::tables::PotionId)>, can_push: bool,
    actions: &[RunAction]) -> bool {
    // Locked options are not executable candidates. BeforeEventStarted forbids
    // every use/discard row, even while the enchant selector is outstanding.
    let mut index = 0;
    if potion.is_some() {
        if !actions.get(index).is_some_and(|a| metadata_row_proven(a, "lift", "Lift",
            "Discard a potion, gain 10 Max HP")) { return false; }
        index += 1;
    }
    if can_push {
        if !actions.get(index).is_some_and(|a| metadata_row_proven(a, "push", "Push",
            "Take 6 damage, enchant a card with Vigorous +8")) { return false; }
        index += 1;
    }
    index == actions.len()
}

fn fake_merchant_rows_proven(inventory: &[(sts2_sim::tables::RelicId, i32, bool)],
    gold: i32, actions: &[RunAction]) -> bool {
    use sts2_sim::tables::RelicId;
    if inventory.len() != 6 { return false; }
    let mut ordinal = 0;
    for (i, &(id, price, stocked)) in inventory.iter().enumerate() {
        if inventory[..i].iter().any(|&(other,_,_)| other == id) { return false; }
        let label = match id {
            RelicId::FakeAnchor => "Fake Anchor", RelicId::FakeBloodVial => "Fake Blood Vial",
            RelicId::FakeHappyFlower => "Fake Happy Flower", RelicId::FakeLeesWaffle => "Fake Lee's Waffle",
            RelicId::FakeMango => "Fake Mango", RelicId::FakeOrichalcum => "Fake Orichalcum",
            RelicId::FakeSneckoEye => "Fake Snecko Eye", RelicId::FakeStrikeDummy => "Fake Strike Dummy",
            RelicId::FakeVenerableTeaSet => "Fake Venerable Tea Set", _ => return false,
        };
        if !stocked || gold < price { continue; }
        let Some(a) = actions.get(ordinal) else { return false; };
        if !metadata_target_row_proven(a, ["buy_0","buy_1","buy_2","buy_3","buy_4","buy_5"][i],
            label, &format!("Pay {price} gold for this relic"), Some(id)) || a.price != Some(price)
        { return false; }
        ordinal += 1;
    }
    actions.len() == ordinal + 1 && metadata_row_proven(&actions[ordinal], "leave", "Leave", "")
}

fn fake_merchant_suffix_proven(potions: &[Option<sts2_sim::tables::PotionId>],
    actions: &[RunAction]) -> bool {
    use sts2_sim::tables::PotionId;
    let foul: Vec<_> = potions.iter().enumerate()
        .filter_map(|(i,&p)| (p == Some(PotionId::Foulpotion)).then_some(i)).collect();
    let Some(split) = actions.len().checked_sub(foul.len()) else { return false; };
    potion_inventory_suffix_proven(potions, &actions[..split])
        && actions[split..].iter().zip(foul).all(|(a,slot)|
            a.kind == RunActionKind::UsePotion && a.enabled && a.potion_id == Some(PotionId::Foulpotion)
                && a.slot_index == Some(slot) && a.target_player_id.is_none() && a.target_index.is_none()
                && a.option_id.is_none() && a.label.is_none() && a.description_text().is_none())
}

// ColorfulPhilosophers.GenerateInitialOptions: source color order is retained
// after removing the owner's pool and randomly deleting excess choices.
fn philosopher_rows_proven(pools: &[sts2_sim::character::Character], actions: &[RunAction]) -> bool {
    const POOLS: [sts2_sim::character::Character; 5] = [sts2_sim::character::Character::Necrobinder, sts2_sim::character::Character::Ironclad, sts2_sim::character::Character::Regent, sts2_sim::character::Character::Silent, sts2_sim::character::Character::Defect];
    let mut previous = None;
    if pools.len() != 3 || actions.len() != 3 { return false; }
    for (i, (&pool, action)) in pools.iter().zip(actions).enumerate() {
        let Some(index) = POOLS.iter().position(|&p| p == pool) else { return false };
        if previous.is_some_and(|p| p >= index) { return false; }
        previous = Some(index);
        let (label, text) = match pool {
            sts2_sim::character::Character::Necrobinder => ("Necrobinder Philosopher", "Card rewards from Necrobinder pool"),
            sts2_sim::character::Character::Ironclad => ("Ironclad Philosopher", "Card rewards from Ironclad pool"),
            sts2_sim::character::Character::Regent => ("Regent Philosopher", "Card rewards from Regent pool"),
            sts2_sim::character::Character::Silent => ("Silent Philosopher", "Card rewards from Silent pool"),
            sts2_sim::character::Character::Defect => ("Defect Philosopher", "Card rewards from Defect pool"),
        };
        if !metadata_row_proven(action, ["pool_1","pool_2","pool_3"][i], label, text) { return false; }
    }
    true
}

fn doll_rows_proven(relics: &[sts2_sim::tables::RelicId], actions: &[RunAction]) -> bool {
    use sts2_sim::tables::RelicId;
    if !matches!(relics.len(), 2 | 3) || actions.len() != relics.len() { return false; }
    for (i, (&id, action)) in relics.iter().zip(actions).enumerate() {
        if relics[..i].contains(&id) { return false; }
        let label = match id {
            RelicId::BingBong => "Bing Bong",
            RelicId::MrStruggles => "Mr Struggles",
            RelicId::DaughterOfTheWind => "Daughter of the Wind",
            _ => return false,
        };
        if !metadata_row_proven(action, ["doll_1","doll_2","doll_3"][i], label, "Take this doll relic") {
            return false;
        }
    }
    true
}

// CrystalSphereMinigame exposes the remaining covered cells and Big/Small
// tools, never the item behind a cell. No reward target can be fabricated.
fn sphere_rows_proven(hidden: &[(usize, usize)], actions: &[RunAction]) -> bool {
    if hidden.is_empty() || actions.len() != hidden.len() * 2 { return false; }
    for (i, &(x, y)) in hidden.iter().enumerate() {
        if x >= 11 || y >= 11 || (i > 0 && hidden[i - 1] >= (x, y)) { return false; }
        for (j, (tool, label)) in [("big","Big"),("small","Small")].into_iter().enumerate() {
            if !metadata_row_proven(&actions[2*i+j], &format!("sphere_{tool}_{x}_{y}"),
                &format!("{label} ({x}, {y})"), "") { return false; }
        }
    }
    true
}

// C#: these six GenerateInitialOptions methods enumerate exactly these
// ordered choices; canonical vars fix SpiritGrafter's 25/10, Field's 2,
// and DollRoom's 5/15. Display text is the frozen r7 event menu contract.
fn metadata_source_table_proven(id: &str, run: &RunManager, actions: &[RunAction]) -> bool {
    // C# gameplay evidence alone does not certify the frozen policy's wire.
    // Re-enable only screens accompanied by independently exported wire proof.
    const FROZEN_VERIFIED_EVENTS:&[&str]=&[];
    FROZEN_VERIFIED_EVENTS.contains(&id) && metadata_source_rows_proven(id,run,actions)
}

fn metadata_source_rows_proven(id: &str, run: &RunManager, actions: &[RunAction]) -> bool {
    if id=="RANWID_THE_ELDER" {
        // BeforeEventStarted locks every potion use/removal until completion.
        return run.observation_ranwid_offers().is_some_and(|(p,r)|
            ranwid_rows_proven(p.is_some(),r.is_some(),actions));
    }
    if id == "STONE_OF_ALL_TIME" {
        return run.observation_stone_offers().is_some_and(|(potion,push)|
            stone_rows_proven(potion, push, actions));
    }
    let count = actions.iter().take_while(|a| a.kind == RunActionKind::EventChoice).count();
    if id == "FAKE_MERCHANT" {
        return run.observation_fake_merchant_inventory().is_some_and(|stock|
            fake_merchant_rows_proven(&stock, run.run_ctx().gold, &actions[..count])
                && fake_merchant_suffix_proven(&run.run_ctx().potions, &actions[count..]));
    }
    if !potion_suffix_proven(run, &actions[count..]) { return false; }
    let actions = &actions[..count];
    if matches!(id,"VAKUU"|"NONUPEIPE") {
        let relics:Option<Vec<_>>=actions.iter().map(|a|
            run.observation_ancient_relic(a.option_id?)).collect();
        let bracelet=run.observation_nonupeipe_bracelet_allowed();
        return relics.is_some_and(|r|ancient_offer_rows_proven(id,&r,bracelet,actions));
    }
    if id == "LUMINOUS_CHOIR" {
        return run.observation_choir_price().is_some_and(|p|
            choir_rows_proven(run.run_ctx().gold, p, actions));
    }
    if id == "RELIC_TRADER" {
        return run.observation_relic_trader_offers().is_some_and(|offers| trader_rows_proven(&offers, actions));
    }
    if id == "COLORFUL_PHILOSOPHERS" {
        let pools: Option<Vec<_>> = (0..count).map(|i| run.observation_philosopher_pool(i)).collect();
        return pools.is_some_and(|p| philosopher_rows_proven(&p, actions));
    }
    if id == "DOLL_ROOM" && actions.first().is_some_and(|a| a.option_id == Some("doll_1")) {
        let relics: Option<Vec<_>> = actions.iter().map(|a|
            run.observation_ancient_relic(a.option_id?)).collect();
        return relics.is_some_and(|r| doll_rows_proven(&r, actions));
    }
    if id == "CRYSTAL_SPHERE" && run.observation_sphere_hidden_cells().is_some() {
        return run.observation_sphere_hidden_cells().is_some_and(|h| sphere_rows_proven(&h, actions));
    }
    if id == "THE_LANTERN_KEY" && actions.len() == 1 {
        return metadata_row_proven(&actions[0], "fight", "Fight", "Fight for Lantern Key card");
    }
    let rows: &[(&str, &str, &str)] = match id {
        "CRYSTAL_SPHERE" => &[
            ("pay", "Uncover Future", "3 Prophesize picks"),
            ("debt", "Payment Plan", "6 picks, gain Debt curse"),
        ],
        "DOLL_ROOM" => &[
            ("random", "Random", "Get a random doll relic"),
            ("take_time", "Take Some Time", "Take 5 damage, choose from 2 dolls"),
            ("examine", "Examine", "Take 15 damage, choose from 3 dolls"),
        ],
        "FIELD_OF_MAN_SIZED_HOLES" => &[
            ("resist", "Resist", "Remove 2 cards, gain Normality curse"),
            ("enter", "Enter Your Hole", "Enchant a card with Perfect Fit"),
        ],
        "INFESTED_AUTOMATON" => &[
            ("study", "Study", "Gain a random Power card"),
            ("touch_core", "Touch Core", "Gain a random 0-cost card"),
        ],
        "SPIRIT_GRAFTER" => &[
            ("let_it_in", "Let It In", "Heal 25 HP, gain Metamorphosis card"),
            ("rejection", "Rejection", "Upgrade 1 card, take 10 damage"),
        ],
        "THE_LANTERN_KEY" => &[
            ("return_key", "Return the Key", "Gain 100 gold"),
            ("keep_key", "Keep the Key", "Fight for Lantern Key card"),
        ],
        _ => return false,
    };
    actions.len() == rows.len() && actions.iter().zip(rows).all(|(a,(option,label,text))|
        metadata_row_proven(a, option, label, text))
}

fn scoped_source_table_proven(id:&str, run:&RunManager, actions:&[RunAction]) -> bool {
    match id {
        "SLIPPERY_BRIDGE" => bridge_table_proven(run,actions),
        "PUNCH_OFF" => punch_table_proven(actions),
        "ENDLESS_CONVEYOR" => conveyor_table_proven(run,actions),
        "SAPPHIRE_SEED" => fixed_table_with_potions(run,actions,&[
            ("eat","Eat","Heal 9 HP, upgrade 1 card"),
            ("plant","Plant the Seed","Enchant a card with Sown"),
        ]),
        "WHISPERING_HOLLOW" => fixed_table_with_potions(run,actions,&[
            ("gold","Pay Gold","Gain 2 potions"),
            ("hug","Hug","Take 9 damage, transform 1 card"),
        ]),
        _ => false,
    }
}

fn fixed_table_with_potions(
    run:&RunManager, actions:&[RunAction], rows:&[(&str,&str,&str)],
) -> bool {
    if actions.len()<rows.len() { return false; }
    for (action,(id,label,description)) in actions[..rows.len()].iter().zip(rows) {
        if action.kind!=RunActionKind::EventChoice || !action.enabled
            || action.option_id!=Some(*id) || action.label!=Some(*label)
            || action.description_text().as_deref()!=Some(*description)
        { return false; }
    }
    potion_suffix_proven(run,&actions[rows.len()..])
}

pub fn event_option_label(run:&RunManager,action:&RunAction)->Result<String,ObsError> {
    let label=action.label.unwrap_or("");
    let price=match (run.current_event_id(),action.option_id) {
        (Some("CRYSTAL_SPHERE"),Some("pay"))=>Some(run.observation_crystal_price()
            .ok_or(ObsError::UnsupportedState("Crystal Sphere retained price missing"))?),
        (Some("LUMINOUS_CHOIR"),Some("tribute"))=>Some(run.observation_choir_price()
            .ok_or(ObsError::UnsupportedState("Choir retained price missing"))?),
        (Some("WELCOME_TO_WONGOS"),Some("bargain_bin"))=>Some(100),
        (Some("WELCOME_TO_WONGOS"),Some("featured"))=>Some(200),
        (Some("WELCOME_TO_WONGOS"),Some("mystery"))=>Some(300),
        _=>None,
    };
    Ok(price.map_or_else(||label.to_owned(),|p|format!("{label} ({p}g)")))
}

fn write_inner(run:&RunManager, actions:&[RunAction], b:&mut ObsBuffers<'_>, proof:bool) -> Result<(),ObsError> {
    let id=run.current_event_id().unwrap_or("");
    let rule_admitted = !proof
        && sts2_sim::obs::rule_admitted("event_option_table",&format!("event:{id}"));
    // The int1-only exact keys predate the natural-frozen seed1525 mismatch.
    // This source needs a fresh whole-screen rule proof, not those old keys.
    if !proof && id=="JUNGLE_MAZE_ADVENTURE" && !rule_admitted {
        return Err(ObsError::UnsupportedState("event identity/target metadata is unported"));
    }
    let exact_table_admitted = !proof && !rule_admitted && run.current_event_id().is_some()
        && sts2_sim::run::recorded_selector_key(&option_table_key(id,actions));
    let source_table_admitted = !proof && !rule_admitted && !exact_table_admitted
        && scoped_source_table_proven(id,run,actions);
    if !proof && metadata_requires_table_proof(id) && !rule_admitted
        && !exact_table_admitted
        && !metadata_source_table_proven(id, run, actions) {
        return Err(ObsError::UnsupportedState("event identity/target metadata is unported"));
    }
    if !proof && !rule_admitted
        && matches!(id,"SAPPHIRE_SEED"|"JUNGLE_MAZE_ADVENTURE"|"TABLET_OF_TRUTH"|"WHISPERING_HOLLOW")
        && !exact_table_admitted && !source_table_admitted
    {
        return Err(ObsError::UnsupportedState("event identity/target metadata is unported"));
    }
    let event = identity(run.current_event_id().unwrap_or(""))?;
    if !proof && !rule_admitted && !exact_table_admitted && !source_table_admitted && event == "SlipperyBridge" {
        return Err(ObsError::UnsupportedState("unproven Slippery Bridge option table"));
    }
    if !proof && !rule_admitted && !exact_table_admitted && !source_table_admitted && event == "PunchOff" {
        return Err(ObsError::UnsupportedState("unproven Punch Off option table"));
    }
    if !proof && !rule_admitted && !exact_table_admitted && !source_table_admitted && event == "EndlessConveyor" {
        return Err(ObsError::UnsupportedState("unproven Endless Conveyor option table"));
    }
    if !proof && !rule_admitted && !exact_table_admitted && event == "TheFutureOfPotions" && actions.len() != 3 {
        return Err(ObsError::UnsupportedState("unproven Future of Potions option table"));
    }
    for (ordinal, action) in actions.iter().enumerate() {
        if !proof && !rule_admitted && !exact_table_admitted && event == "TheFutureOfPotions" && (action.kind != RunActionKind::EventChoice
            || action.option_id != Some(["trade_0", "trade_1", "trade_2"][ordinal])
            || !action.enabled || action.label != Some("Trade")
            || action.description_text().as_deref() != Some("Discard potion for upgraded card reward"))
        {
            return Err(ObsError::UnsupportedState("unproven Future of Potions option table"));
        }
        if matches!(action.kind,RunActionKind::UsePotion|RunActionKind::DiscardPotion) {
            let slot=b.push_candidate()?;
            sts2_sim::obs::write_run_potion_candidate(run,action,slot,b)?;
            continue;
        }
        if action.kind != RunActionKind::EventChoice {
            return Err(ObsError::UnsupportedState("ordinary-event auxiliary action is unported"));
        }
        let description = action.description_text().unwrap_or_default();
        let future_label = if event == "TheFutureOfPotions" {
            let (_, id) = run.observation_future_potion(action.option_id.ok_or(ObsError::InvalidActionSlot)?)
                .ok_or(ObsError::UnsupportedState("Future of Potions typed offer missing"))?;
            Some(format!("Trade {}", sts2_sim::tables::POTION_INFO[id.index()].id))
        } else { None };
        let label=event_option_label(run,action)?;
        let text = [future_label.as_deref().unwrap_or(&label).trim(), description.trim()]
            .into_iter().filter(|s| !s.is_empty()).collect::<Vec<_>>().join(" ");
        let max_hp = integer(&text, 3, false).or_else(|| integer(&text, 4, true))
            .or_else(|| integer(&text, 5, true).map(|v| -v));
        let values = [integer(&text, 1, true), integer(&text, 2, true), max_hp,
            integer(&text, 6, true), integer(&text, 7, true), gain(&text, "card"),
            integer(&text, 8, true), integer(&text, 9, true)];
        let relic = gain(&text, "relic").is_some();
        let potion = gain(&text, "potion").is_some();
        let slot = b.push_candidate()?;
        b.candidate_token_named(slot, 0, "action_kind", "event_choice")?;
        b.candidate_token_named(slot, 1, "event_option_id", action.option_id.ok_or(ObsError::InvalidActionSlot)?)?;
        b.candidate_token_named(slot, 2, "event_id", event)?;
        if action.enabled && (matches!(event, "Tezcatara" | "Orobas" | "Pael" | "Vakuu" | "Nonupeipe" | "Tanx")
            || (event == "DollRoom" && action.option_id.is_some_and(|id| id.starts_with("doll_")))) {
            let id = run.observation_ancient_relic(action.option_id.ok_or(ObsError::InvalidActionSlot)?)
                .ok_or(ObsError::UnsupportedState("ancient typed relic offer missing"))?;
            // structured_encoder.py:926-930: the generated offer, not its label.
            b.candidate_token_named(slot, 3, "relic_id", id.name())?;
            if let Some((starter,upgraded))=run.observation_orobas_starter_swap(
                action.option_id.ok_or(ObsError::InvalidActionSlot)?) {
                b.candidate_token_named(slot,4,"relic_id",starter.name())?;
                b.candidate_token_named(slot,5,"relic_id",upgraded.name())?;
            }
        }
        if event == "FakeMerchant" && action.option_id.is_some_and(|id| id.starts_with("buy_")) {
            // MerchantRelicEntry.Model is public inventory, not a reward roll.
            let id = action.relic_id.ok_or(ObsError::UnsupportedState("Fake Merchant typed stock missing"))?;
            b.candidate_token_named(slot, 3, "relic_id", id.name())?;
        }
        if event=="ColorfulPhilosophers" {
            let character=match run.observation_philosopher_pool(ordinal) {
                Some(sts2_sim::character::Character::Ironclad)=>"Ironclad",Some(sts2_sim::character::Character::Silent)=>"Silent",Some(sts2_sim::character::Character::Defect)=>"Defect",
                Some(sts2_sim::character::Character::Regent)=>"Regent",Some(sts2_sim::character::Character::Necrobinder)=>"Necrobinder",
                None=>return Err(ObsError::UnsupportedState("Philosopher retained character pool missing")),
            };
            b.candidate_token_named(slot,3,"character",character)?;
        }
        for (i, (value, scale)) in values.into_iter()
            .zip([50.0,50.0,25.0,500.0,500.0,5.0,5.0,5.0]).enumerate()
        { b.candidate_scalar(slot, i, value.unwrap_or(0), scale, value.is_some()); }
        for (i, value) in [(10,action.enabled),(11,relic),(12,relic),(13,potion),(14,potion),
            (15,values.iter().any(Option::is_some)||relic||potion)]
        { b.candidate_bool(slot, i, value); }
        b.set_model_eligible(slot, action.enabled);
    }
    Ok(())
}

// Frozen act2.py:1061-1093: both pages use this ordered two-row table.
// Only damage 3, 4 and 5 have independent digest/wire witnesses.
// Later pages must not borrow admission from an earlier table.
fn bridge_table_proven(run:&RunManager, actions: &[RunAction]) -> bool {
    if actions.len() < 2 { return false; }
    let a = &actions[0];
    let b = &actions[1];
    let description = b.description_text().unwrap_or_default();
    let damage = description.strip_prefix("Take ")
        .and_then(|s| s.strip_suffix(" damage, reroll card"))
        .and_then(|s| s.parse::<u32>().ok());
    a.kind == RunActionKind::EventChoice && a.enabled
        && a.option_id == Some("overcome") && a.label == Some("Overcome")
        && a.description_text().as_deref() == Some("Lose a random card")
        && b.kind == RunActionKind::EventChoice && b.enabled
        && b.option_id == Some("hold_on") && b.label == Some("Hold On")
        && damage.is_some_and(|n| (3..=5).contains(&n) && description == format!("Take {n} damage, reroll card"))
        && potion_suffix_proven(run,&actions[2..])
}

// Frozen shared.py:1402-1450, initial and take_them -> fight pages.
// Witnesses replay seed 4100109 before its later accepted combat divergence.
fn punch_table_proven(actions: &[RunAction]) -> bool {
    let rows: Vec<_> = actions.iter().map(|a|
        (a.kind, a.option_id, a.label, a.description_text(), a.enabled)).collect();
    let row = |id, label, description: &str| (RunActionKind::EventChoice,
        Some(id), Some(label), Some(description.to_owned()), true);
    rows == vec![row("nab", "Nab", "Gain Injury curse + relic reward"),
        row("take_them", "I Can Take Them", "Fight for relic + potion")]
        || rows == vec![row("fight", "Fight", "Fight for relic + potion")]
}

// Frozen act2.py:267-316: initial and repeat pages; dish identity is hidden
// and must not become an observation feature. The locked row is omitted from
// legal actions (run_manager.py::_actions_event), leaving only Leave.
fn conveyor_table_proven(run:&RunManager, actions: &[RunAction]) -> bool {
    let count = actions.iter().take_while(|a| a.kind == RunActionKind::EventChoice).count();
    if !potion_suffix_proven(run,&actions[count..]) { return false; }
    let rows: Vec<_> = actions[..count].iter().map(|a|
        (a.kind, a.option_id, a.label, a.description_text(), a.enabled)).collect();
    let row = |id, label, description: &str| (RunActionKind::EventChoice,
        Some(id), Some(label), Some(description.to_owned()), true);
    rows == vec![row("grab", "Grab Something (40g)", "Random dish"),
        row("observe", "Observe Chef", "Upgrade 1 random card (free)")]
        || rows == vec![row("grab", "Grab Another (40g)", "Random dish"),
            row("leave", "Leave", "Done eating")]
        || rows == vec![row("leave", "Leave", "Done eating")]
}

#[cfg(test)]
mod tests {
    // Source-only predicates must not bypass the frozen-wire audit, even when
    // their initial menu is otherwise well-formed.
    macro_rules! pending_frozen_event {
        ($test:ident,$id:literal,$ids:expr) => {
            #[test]
            fn $test() {
                let mut run=RunManager::new(0,Default::default());
                let template=run.available_actions()[0];
                let actions:Vec<_>=$ids.iter().map(|&id| {
                    let mut action=template;
                    action.kind=RunActionKind::EventChoice;
                    action.option_id=Some(id);
                    assert!(sts2_sim::obs::frozen_token_id("event_option_id",id).is_some());
                    action
                }).collect();
                assert!(!metadata_source_table_proven($id,&run,&actions));
            }
        };
    }
    pending_frozen_event!(frozen_crystal_ids_pending,"CRYSTAL_SPHERE",["pay","debt"]);
    pending_frozen_event!(frozen_philosopher_ids_pending,"COLORFUL_PHILOSOPHERS",["pool_1","pool_2","pool_3"]);
    pending_frozen_event!(frozen_lantern_ids_pending,"THE_LANTERN_KEY",["return_key","keep_key"]);
    pending_frozen_event!(frozen_field_ids_pending,"FIELD_OF_MAN_SIZED_HOLES",["resist","enter"]);
    pending_frozen_event!(frozen_automaton_ids_pending,"INFESTED_AUTOMATON",["study","touch_core"]);
    pending_frozen_event!(frozen_grafter_ids_pending,"SPIRIT_GRAFTER",["let_it_in","rejection"]);
    pending_frozen_event!(frozen_doll_ids_pending,"DOLL_ROOM",["random","take_time","examine"]);
    pending_frozen_event!(frozen_choir_ids_pending,"LUMINOUS_CHOIR",["reach","tribute"]);
    pending_frozen_event!(frozen_fake_merchant_ids_pending,"FAKE_MERCHANT",["buy","leave"]);
    pending_frozen_event!(frozen_trader_ids_pending,"RELIC_TRADER",["trade_0","trade_1","trade_2"]);
    pending_frozen_event!(frozen_stone_ids_pending,"STONE_OF_ALL_TIME",["lift","push"]);
    pending_frozen_event!(frozen_wongo_ids_pending,"WELCOME_TO_WONGOS",["bargain_bin","featured","leave"]);
    pending_frozen_event!(frozen_vakuu_ids_pending,"VAKUU",["relic_1","relic_2","relic_3"]);
    pending_frozen_event!(frozen_ranwid_ids_pending,"RANWID_THE_ELDER",["potion","gold","relic"]);
    use super::*;
    #[test]
    fn source_metadata_admission_is_whole_initial_screen_not_an_identity_allowlist() {
        let mut run = RunManager::new(0, Default::default());
        let template = run.available_actions()[0].clone();
        let cases: &[(&str, &[(&str, &str, &str)])] = &[
            ("CRYSTAL_SPHERE", &[("pay","Uncover Future","3 Prophesize picks"),
                ("debt","Payment Plan","6 picks, gain Debt curse")]),
            ("DOLL_ROOM", &[("random","Random","Get a random doll relic"),
                ("take_time","Take Some Time","Take 5 damage, choose from 2 dolls"),
                ("examine","Examine","Take 15 damage, choose from 3 dolls")]),
            ("FIELD_OF_MAN_SIZED_HOLES", &[("resist","Resist","Remove 2 cards, gain Normality curse"),
                ("enter","Enter Your Hole","Enchant a card with Perfect Fit")]),
            ("INFESTED_AUTOMATON", &[("study","Study","Gain a random Power card"),
                ("touch_core","Touch Core","Gain a random 0-cost card")]),
            ("SPIRIT_GRAFTER", &[("let_it_in","Let It In","Heal 25 HP, gain Metamorphosis card"),
                ("rejection","Rejection","Upgrade 1 card, take 10 damage")]),
            ("THE_LANTERN_KEY", &[("return_key","Return the Key","Gain 100 gold"),
                ("keep_key","Keep the Key","Fight for Lantern Key card")]),
        ];
        for &(id, rows) in cases {
            let actions: Vec<_> = rows.iter().map(|&(option,label,text)| {
                let mut a = template.clone();
                a.kind = RunActionKind::EventChoice;
                a.option_id = Some(option); a.label = Some(label); a.description = Some(text);
                a.enabled = true;
                a
            }).collect();
            assert!(metadata_source_rows_proven(id, &run, &actions), "{id}");
            let mut changed = actions.clone();
            changed.reverse();
            assert!(!metadata_source_rows_proven(id, &run, &changed), "order: {id}");
            changed = actions.clone();
            changed[0].description = Some("Gain 999 gold");
            assert!(!metadata_source_rows_proven(id, &run, &changed), "text: {id}");
            changed = actions.clone();
            changed[0].card_id = Some(sts2_sim::tables::CardId::Metamorphosis);
            assert!(!metadata_source_rows_proven(id, &run, &changed), "hidden target: {id}");
            changed = actions.clone();
            changed.push(template.clone());
            assert!(!metadata_source_rows_proven(id, &run, &changed), "extra candidate: {id}");
            changed = actions.clone();
            changed.pop();
            assert!(!metadata_source_rows_proven(id, &run, &changed), "missing candidate: {id}");
        }
    }

    #[test]
    fn generated_metadata_pages_cannot_borrow_initial_source_tables() {
        let mut run = RunManager::new(0, Default::default());
        let mut action = run.available_actions()[0].clone();
        action.option_id = Some("doll_1");
        action.label = Some("Bing Bong");
        action.description = Some("Gain a doll relic");
        action.relic_id = Some(sts2_sim::tables::RelicId::BingBong);
        assert!(!metadata_source_table_proven("DOLL_ROOM", &run, &[action]));
        assert_eq!(identity("COLORFUL_PHILOSOPHERS").unwrap(), "ColorfulPhilosophers");
        assert!(!metadata_source_table_proven("VAKUU",&run,&[action]));
    }

    #[test]
    fn bridge_later_damage_tables_cannot_borrow_first_page_witnesses() {
        let mut run = RunManager::new(0, Default::default());
        let mut actions: Vec<_> = run.available_actions().into_iter().take(2).collect();
        actions[0].option_id = Some("overcome");
        actions[0].label = Some("Overcome");
        actions[0].description = Some("Lose a random card");
        actions[1].option_id = Some("hold_on");
        actions[1].label = Some("Hold On");
        for (description, admitted) in [
            ("Take 3 damage, reroll card", true),
            ("Take 4 damage, reroll card", true),
            ("Take 5 damage, reroll card", true),
            ("Take 6 damage, reroll card", false),
            ("Take 30 damage, reroll card", false),
        ] {
            actions[1].description = Some(description);
            assert_eq!(bridge_table_proven(&run,&actions), admitted);
        }
    }

    #[test]
    fn source_backed_identity_tables_reject_metadata_mutation() {
        let mut run=RunManager::new(0,Default::default());
        let mut actions:Vec<_>=run.available_actions().into_iter().take(2).collect();
        actions[0].option_id=Some("eat");
        actions[0].label=Some("Eat");
        actions[0].description=Some("Heal 9 HP, upgrade 1 card");
        actions[1].option_id=Some("plant");
        actions[1].label=Some("Plant the Seed");
        actions[1].description=Some("Enchant a card with Sown");
        assert!(scoped_source_table_proven("SAPPHIRE_SEED",&run,&actions));
        actions[1].label=Some("Plant");
        assert!(!scoped_source_table_proven("SAPPHIRE_SEED",&run,&actions));
    }

    #[test]
    fn tablet_later_pages_and_changed_option_tables_need_their_own_proof() {
        let mut run=RunManager::new(0,Default::default());
        let mut actions=run.available_actions();
        actions.truncate(2);
        actions[0].option_id=Some("read");
        actions[1].option_id=Some("leave");
        let first=option_table_key("TABLET_OF_TRUTH",&actions);
        let proven=format!("{first}\tfixture.json\n");
        assert!(table_admitted(&first,&proven));
        actions[1].option_id=Some("give_up");
        let later=option_table_key("TABLET_OF_TRUTH",&actions);
        assert!(!table_admitted(&later,&proven));
        assert!(!table_admitted(&later,"event:TABLET_OF_TRUTH\tidentity-only.json\n"));
        actions[1].option_id=Some("leave");
        actions[0].enabled=false;
        assert!(!table_admitted(&option_table_key("TABLET_OF_TRUTH",&actions),&proven));
        actions[0].enabled=true;
        actions.reverse();
        assert!(!table_admitted(&option_table_key("TABLET_OF_TRUTH",&actions),&proven));
    }
    #[test]
    fn public_text_never_borrows_another_units_number_or_conditional_gain() {
        assert_eq!(gain("Gain 57 gold and one random card", "card"), Some(1));
        assert_eq!(gain("Gain 57 gold", "card"), None);
        assert_eq!(gain("Gain a relic if you win", "relic"), None);
        assert_eq!(integer("Take 6 damage, gain 57 gold", 1, true), Some(6));
        assert_eq!(integer("+2 Max HP, take 3 damage", 3, false), Some(2));
    }
    #[test]
    fn registry_aliases_do_not_guess_ordinal_offer_targets() {
        assert_eq!(identity("WATERLOGGED_SCRIPTORIUM").unwrap(), "WaterloggedScriptorium");
        assert_eq!(identity("ROOM_FULL_OF_CHEESE").unwrap(), "RoomFullOfCheese");
        assert_eq!(identity("SUNKEN_TREASURY").unwrap(), "SunkenTreasury");
        assert_eq!(identity("TEZCATARA").unwrap(), "Tezcatara");
        assert_eq!(identity("OROBAS").unwrap(), "Orobas");
        assert_eq!(identity("PAEL").unwrap(), "Pael");
        assert_eq!(identity("TANX").unwrap(), "Tanx");
        assert_eq!(identity("SAPPHIRE_SEED").unwrap(), "SapphireSeed");
        assert_eq!(identity("WHISPERING_HOLLOW").unwrap(), "WhisperingHollow");
        for (id,name) in [("VAKUU","Vakuu"),("NONUPEIPE","Nonupeipe"),("RANWID_THE_ELDER","RanwidTheElder")] {
            assert_eq!(identity(id).unwrap(),name);
            let mut run=RunManager::new(0,Default::default());
            let actions=run.available_actions();
            assert!(!metadata_source_table_proven(id,&run,&actions),"no retained offers");
        }
    }

    fn metadata_action(option: &'static str, label: &'static str, text: &'static str) -> RunAction {
        let mut run = RunManager::new(0, Default::default());
        let mut a = run.available_actions()[0].clone();
        a.option_id = Some(option); a.label = Some(label); a.description = Some(text);
        a
    }

    #[test]
    fn ranwid_requires_all_unlocked_rows_in_order_and_forbids_inventory_actions() {
        let rows=vec![metadata_action("potion","Give a Potion","Lose a potion, gain a relic"),
            metadata_action("gold","Give 100 Gold","Lose 100g, gain a relic"),
            metadata_action("relic","Give a Relic","Lose a relic, gain 2 relics")];
        assert!(ranwid_rows_proven(true,true,&rows));
        assert!(ranwid_rows_proven(false,false,&rows[1..2]));
        let mut changed=rows.clone();changed.swap(0,2);
        assert!(!ranwid_rows_proven(true,true,&changed));
        let mut changed=rows.clone();changed[0].potion_id=Some(sts2_sim::tables::PotionId::Bloodpotion);
        assert!(!ranwid_rows_proven(true,true,&changed),"do not invent candidate targets");
        let mut changed=rows.clone();let mut discard=rows[0];
        discard.kind=RunActionKind::DiscardPotion;changed.push(discard);
        assert!(!ranwid_rows_proven(true,true,&changed));
        assert!(!ranwid_rows_proven(false,true,&rows),"locked row cannot leak");
    }

    #[test]
    fn ancient_metadata_needs_retained_pool_targets_costs_and_complete_order() {
        use sts2_sim::tables::RelicId as R;
        let rows=vec![metadata_action("relic_1","Fiddle","Gain a relic"),
            metadata_action("relic_2","Sere Talon","Gain a relic"),
            metadata_action("relic_3","Music Box","Gain a relic")];
        let offers=[R::Fiddle,R::SereTalon,R::MusicBox];
        assert!(ancient_offer_rows_proven("VAKUU",&offers,false,&rows));
        let mut changed=rows.clone();changed[1].description=Some("Gain an ancient relic");
        assert!(!ancient_offer_rows_proven("VAKUU",&offers,false,&changed),"frozen UI text");
        assert!(!ancient_offer_rows_proven("VAKUU",&[R::MusicBox,R::SereTalon,R::Fiddle],false,&rows));
        assert!(!ancient_offer_rows_proven("VAKUU",&offers,false,&rows[..2]));
        let rows=vec![metadata_action("relic_1","Glitter","Gain an ancient relic"),
            metadata_action("relic_2","Looming Fruit","Gain an ancient relic"),
            metadata_action("relic_3","Beautiful Bracelet","Gain an ancient relic")];
        let offers=[R::Glitter,R::LoomingFruit,R::BeautifulBracelet];
        assert!(ancient_offer_rows_proven("NONUPEIPE",&offers,true,&rows));
        assert!(!ancient_offer_rows_proven("NONUPEIPE",&offers,false,&rows));
        for id in ["relic_1","relic_2","relic_3","potion","gold","relic"] {
            assert!(sts2_sim::obs::frozen_token_id("event_option_id",id).is_some(),
                "every admitted option must be encodable: {id}");
        }
    }

    #[test]
    fn choir_admission_tracks_retained_price_and_omitted_locked_tribute() {
        let reach = metadata_action("reach","Reach Into the Flesh","Remove 2 cards, gain Spore Mind curse");
        let mut tribute = metadata_action("tribute","Offer Tribute","Gain a relic");
        tribute.price = Some(123);
        let rows = vec![reach.clone(), tribute.clone()];
        assert!(choir_rows_proven(123,123,&rows));
        assert!(choir_rows_proven(122,123,&[reach]));
        assert!(!choir_rows_proven(122,123,&rows));
        assert!(!choir_rows_proven(123,124,&rows));
        assert!(!choir_rows_proven(500,99,&rows));
        let mut changed = rows.clone(); changed.swap(0,1);
        assert!(!choir_rows_proven(500,123,&changed));
        tribute.relic_id = Some(sts2_sim::tables::RelicId::Anchor);
        assert!(!choir_rows_proven(500,123,&[rows[0].clone(),tribute]));
    }

    #[test]
    fn trader_admission_requires_complete_generated_offers_and_no_guessed_row_targets() {
        use sts2_sim::tables::RelicId;
        let offers = [(RelicId::Anchor,RelicId::Pear),(RelicId::Lantern,RelicId::Mango),
            (RelicId::PenNib,RelicId::Strawberry)];
        let rows: Vec<_> = [("trade_0","Trade Relic 1"),("trade_1","Trade Relic 2"),("trade_2","Trade Relic 3")].into_iter()
            .map(|(id,label)| metadata_action(id,label,"Swap an owned relic for a new one")).collect();
        assert!(trader_rows_proven(&offers,&rows));
        assert!(!trader_rows_proven(&offers,&rows[..2]));
        let mut changed = rows.clone(); changed.swap(1,2);
        assert!(!trader_rows_proven(&offers,&changed));
        changed = rows.clone(); changed[0].relic_id = Some(RelicId::Mango);
        assert!(!trader_rows_proven(&offers,&changed));
        assert!(trader_rows_proven(&[],&[metadata_action("proceed","Proceed","")]));
        assert!(!trader_rows_proven(&offers,&[metadata_action("proceed","Proceed","")]));
    }

    #[test]
    fn stone_admission_omits_locked_rows_and_rejects_every_inventory_suffix() {
        use sts2_sim::tables::PotionId;
        let lift = metadata_action("lift","Lift","Discard a potion, gain 10 Max HP");
        let push = metadata_action("push","Push","Take 6 damage, enchant a card with Vigorous +8");
        let potion = Some((2,PotionId::Fruitjuice));
        assert!(stone_rows_proven(potion,true,&[lift.clone(),push.clone()]));
        assert!(stone_rows_proven(potion,false,&[lift.clone()]));
        assert!(stone_rows_proven(None,true,&[push.clone()]));
        assert!(!stone_rows_proven(None,true,&[lift.clone(),push.clone()]));
        assert!(!stone_rows_proven(potion,true,&[push.clone(),lift.clone()]));
        let mut extra = push.clone(); extra.kind = RunActionKind::DiscardPotion;
        assert!(!stone_rows_proven(potion,true,&[lift.clone(),push,extra]));
        let mut changed = lift; changed.description = Some("Gain 11 Max HP");
        assert!(!stone_rows_proven(potion,false,&[changed]));
    }

    #[test]
    fn fake_merchant_admission_proves_stock_affordability_prices_and_foul_suffix() {
        use sts2_sim::tables::RelicId;
        let inventory = [
            (RelicId::FakeAnchor,50,true),(RelicId::FakeBloodVial,60,true),
            (RelicId::FakeHappyFlower,50,false),(RelicId::FakeLeesWaffle,50,false),
            (RelicId::FakeMango,50,false),(RelicId::FakeOrichalcum,50,false),
        ];
        let mut buy = metadata_action("buy_0","Fake Anchor","Pay 50 gold for this relic");
        buy.price = Some(50); buy.relic_id = Some(RelicId::FakeAnchor);
        let leave = metadata_action("leave","Leave","");
        let rows = vec![buy.clone(),leave.clone()];
        assert!(fake_merchant_rows_proven(&inventory,50,&rows));
        assert!(!fake_merchant_rows_proven(&inventory,49,&rows));
        assert!(fake_merchant_rows_proven(&inventory,49,&[leave.clone()]));
        assert!(!fake_merchant_rows_proven(&inventory,60,&rows)); // omitted affordable slot
        let mut changed = rows.clone(); changed[0].price = Some(49);
        assert!(!fake_merchant_rows_proven(&inventory,50,&changed));
        changed = rows.clone(); changed[0].relic_id = Some(RelicId::FakeMango);
        assert!(!fake_merchant_rows_proven(&inventory,50,&changed));
        changed = rows; changed.swap(0,1);
        assert!(!fake_merchant_rows_proven(&inventory,50,&changed));
        let potions = [None,Some(sts2_sim::tables::PotionId::Foulpotion)];
        let mut discard = metadata_action("","","");
        discard.kind = RunActionKind::DiscardPotion;
        discard.option_id = None; discard.label = None; discard.description = None;
        discard.potion_id = potions[1]; discard.slot_index = Some(1);
        let mut use_foul = discard.clone(); use_foul.kind = RunActionKind::UsePotion;
        assert!(fake_merchant_suffix_proven(&potions,&[discard.clone(),use_foul.clone()]));
        assert!(!fake_merchant_suffix_proven(&potions,&[use_foul.clone(),discard.clone()]));
        assert!(!fake_merchant_suffix_proven(&potions,&[discard]));
        let mut targeted = use_foul.clone(); targeted.target_index = Some(0);
        let mut discard = use_foul.clone(); discard.kind = RunActionKind::DiscardPotion;
        assert!(!fake_merchant_suffix_proven(&potions,&[discard,targeted]));
        use_foul.slot_index = Some(0);
        assert!(!fake_merchant_suffix_proven(&potions,&[use_foul]));
    }

    #[test]
    fn philosophers_use_retained_pool_targets_not_labels_or_ordinals() {
        let actions = vec![
            metadata_action("pool_1", "Necrobinder Philosopher", "Card rewards from Necrobinder pool"),
            metadata_action("pool_2", "Silent Philosopher", "Card rewards from Silent pool"),
            metadata_action("pool_3", "Defect Philosopher", "Card rewards from Defect pool"),
        ];
        assert!(philosopher_rows_proven(&[sts2_sim::character::Character::Necrobinder,sts2_sim::character::Character::Silent,sts2_sim::character::Character::Defect], &actions));
        assert!(!philosopher_rows_proven(&[sts2_sim::character::Character::Regent,sts2_sim::character::Character::Silent,sts2_sim::character::Character::Defect], &actions));
        assert!(!philosopher_rows_proven(&[sts2_sim::character::Character::Defect,sts2_sim::character::Character::Silent,sts2_sim::character::Character::Necrobinder], &actions));
        assert!(!philosopher_rows_proven(&[sts2_sim::character::Character::Necrobinder,sts2_sim::character::Character::Silent,sts2_sim::character::Character::Silent], &actions));
        assert!(!philosopher_rows_proven(&[sts2_sim::character::Character::Necrobinder,sts2_sim::character::Character::Silent], &actions));
        let mut changed = actions.clone();
        changed[1].option_id = Some("pool_3");
        assert!(!philosopher_rows_proven(&[sts2_sim::character::Character::Necrobinder,sts2_sim::character::Character::Silent,sts2_sim::character::Character::Defect], &changed));
    }

    #[test]
    fn inventory_suffix_preserves_slot_holes_usage_model_and_whole_order() {
        use sts2_sim::tables::{PotionId, POTION_INFO, PotionTargetType};
        let potions = [None, Some(PotionId::Fruitjuice), Some(PotionId::Foulpotion)];
        let mut use_potion = metadata_action("", "", "");
        use_potion.kind = RunActionKind::UsePotion;
        use_potion.option_id = None; use_potion.label = None; use_potion.description = None;
        use_potion.potion_id = Some(PotionId::Fruitjuice); use_potion.slot_index = Some(1);
        use_potion.target_player_id = if POTION_INFO[PotionId::Fruitjuice.index()].target == PotionTargetType::AnyPlayer {
            Some(1)
        } else { None };
        let mut discard = use_potion.clone();
        discard.kind = RunActionKind::DiscardPotion; discard.target_player_id = None;
        let mut foul = discard.clone();
        foul.slot_index = Some(2); foul.potion_id = Some(PotionId::Foulpotion);
        let actions = vec![use_potion, discard, foul];
        assert!(potion_inventory_suffix_proven(&potions, &actions));
        let mut changed = actions.clone(); changed.swap(0, 1);
        assert!(!potion_inventory_suffix_proven(&potions, &changed));
        changed = actions.clone(); changed[0].slot_index = Some(0);
        assert!(!potion_inventory_suffix_proven(&potions, &changed));
        changed = actions.clone(); changed[1].potion_id = Some(PotionId::Fairyinabottle);
        assert!(!potion_inventory_suffix_proven(&potions, &changed));
        changed = actions.clone(); changed[2].kind = RunActionKind::UsePotion;
        assert!(!potion_inventory_suffix_proven(&potions, &changed));
        assert!(!potion_inventory_suffix_proven(&potions, &actions[..2]));
        changed = actions.clone(); changed.push(actions[2].clone());
        assert!(!potion_inventory_suffix_proven(&potions, &changed));
    }

    #[test]
    fn later_doll_and_sphere_pages_need_the_complete_current_public_targets() {
        use sts2_sim::tables::RelicId;
        let dolls = vec![
            metadata_action("doll_1","Bing Bong","Take this doll relic"),
            metadata_action("doll_2","Mr Struggles","Take this doll relic"),
        ];
        assert!(doll_rows_proven(&[RelicId::BingBong,RelicId::MrStruggles], &dolls));
        assert!(!doll_rows_proven(&[RelicId::MrStruggles,RelicId::BingBong], &dolls));
        assert!(!doll_rows_proven(&[RelicId::BingBong,RelicId::BingBong], &dolls));
        assert!(!doll_rows_proven(&[RelicId::BingBong,RelicId::MrStruggles], &dolls[..1]));
        let grid = vec![
            metadata_action("sphere_big_0_0","Big (0, 0)",""),
            metadata_action("sphere_small_0_0","Small (0, 0)",""),
            metadata_action("sphere_big_0_1","Big (0, 1)",""),
            metadata_action("sphere_small_0_1","Small (0, 1)",""),
        ];
        assert!(sphere_rows_proven(&[(0,0),(0,1)], &grid));
        assert!(!sphere_rows_proven(&[(0,0)], &grid));
        assert!(!sphere_rows_proven(&[(0,0),(1,0)], &grid));
        let mut changed = grid.clone(); changed.swap(0,1);
        assert!(!sphere_rows_proven(&[(0,0),(0,1)], &changed));
        changed = grid.clone(); changed[0].relic_id = Some(RelicId::BingBong);
        assert!(!sphere_rows_proven(&[(0,0),(0,1)], &changed));
        let run = RunManager::new(0, Default::default());
        let fight = metadata_action("fight","Fight","Fight for Lantern Key card");
        assert!(metadata_source_rows_proven("THE_LANTERN_KEY", &run, &[fight.clone()]));
        let mut changed = fight; changed.description = Some("Gain Lantern Key card");
        assert!(!metadata_source_table_proven("THE_LANTERN_KEY", &run, &[changed]));
    }
}
