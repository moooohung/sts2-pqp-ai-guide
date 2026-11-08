//! Authentic frozen r7 prompt literals, keyed by the actual resolver source,
//! never inferred from pile names or target creature indices.
use sts2_sim::{choice::{ChoiceSrc, PendingChoice}, combat::Combat};

fn prompt(choice: &PendingChoice) -> Result<&'static str, &'static str> {
    match choice.resolver.src {
        // cards/ironclad.py:257,427,686,852,1766.
        ChoiceSrc::Card(id) => match id.name() {
            "ARMAMENTS" => Ok("Choose a hand card to upgrade"),
            "HEADBUTT" => Ok("Choose a discard card to put on top of draw pile"),
            "TRUE_GRIT" | "BURNING_PACT" | "BRAND" => Ok("Choose a hand card to exhaust"),
            // Frozen r7 cards/silent.py:88-100,217-240,462-495.
            "ACROBATICS" | "HIDDEN_DAGGERS" | "PREPARED" => Ok("Choose a hand card to discard"),
            // Frozen r7 cards/regent.py:114-135,365-385,204-215,430-445.
            "BEGONE" => Ok("Choose a hand card to transform"),
            "CHARGE" => Ok("Choose card(s) to transform into Minion Strike"),
            // C# EntropyPower.AfterPlayerTurnStart: TransformSelectionPrompt.
            // r7 combat.py:3219 interpolates the count; wire purpose is transform.
            "ENTROPY" => Ok("Choose card(s) to transform"),
            "COSMIC_INDIFFERENCE" => Ok("Choose a discard card to put on draw pile"),
            "GLIMMER" => Ok("Choose a hand card to put back on draw pile"),
            // Frozen r7 cards/status.py:744-763 and cards/regent.py:297-310.
            "DUAL_WIELD" => Ok("Choose an Attack or Power to copy"),
            "PHOTON_CUT" => Ok("Choose a hand card to put back on top of draw pile"),
            // Frozen r7 cards/colorless.py:494-510.
            "PURITY" => Ok("Choose any number of hand cards to exhaust"),
            // Frozen r7 cards/necrobinder.py:dredge; C# Dredge.OnPlay
            // supplies this exact CardSelectorPrefs prompt.
            "DREDGE" => Ok("Choose discard cards to move to hand"),
            "GRAVEBLAST" => Ok("Choose a discard card to return to hand"),
            // C# Hologram/Wish.OnPlay select from discard/draw respectively.
            "HOLOGRAM" => Ok("Choose a discard card to return to hand"),
            "WISH" => Ok("Choose a card from draw pile"),
            // C# Abundance/Discovery use FromChooseACardScreen.
            "ABUNDANCE" => Ok("Choose one upgraded Power"),
            "DISCOVERY" => Ok("Choose one of three cards"),
            // C# StratagemPower.AfterShuffle; r7 search_draw_pile_to_hand.
            "STRATAGEM" => Ok("Choose card(s) to move to hand"),
            // C# SculptingStrike.OnPlay selects a hand card for Ethereal.
            "SCULPTING_STRIKE" => Ok("Choose a hand card to make Ethereal"),
            // Frozen r7 cards/silent.py:155 and cards/colorless.py:530,625.
            "DAGGER_THROW" => Ok("Choose a hand card to discard"),
            // Frozen r7 cards/silent.py hand_trick and C# HandTrick.OnPlay:
            // select a Skill in hand for the single-turn Sly modifier.
            "HAND_TRICK" => Ok("Choose a Skill to make Sly this turn"),
            "SEEKER_STRIKE" => Ok("Choose one of the revealed cards"),
            "THINKING_AHEAD" => Ok("Choose a hand card to put back on draw pile"),
            // cards/colorless.py:906,922.
            "SECRET_TECHNIQUE" => Ok("Choose a Skill from draw pile"),
            "SECRET_WEAPON" => Ok("Choose an Attack from draw pile"),
            // Frozen r7 cards/colorless.py:250-276,560-607 and
            // core/combat.py:2999-3021 + powers/remaining_c.py:1110-1135.
            "DISCOVERY" => Ok("Choose one of three cards"),
            "SPLASH" => Ok("Choose one upgraded free attack"),
            "STRATAGEM" => Ok("Choose card(s) to move to hand"),
            _ => Err("combat card choice prompt source is unported"),
        },
        // potions/effects.py:115,155,231,261,319,401,509,576,648.
        ChoiceSrc::Potion(id) => match id.name() {
            "AttackPotion" => Ok("Choose an Attack card"),
            "ColorlessPotion" => Ok("Choose a Colorless card"),
            "PowerPotion" => Ok("Choose a Power card"),
            "SkillPotion" => Ok("Choose a Skill card"),
            "Ashwater" => Ok("Choose any number of hand cards to exhaust"),
            "GamblersBrew" => Ok("Choose any number of hand cards to discard"),
            "TouchOfInsanity" => Ok("Choose a hand card to set to 0 cost"),
            "DropletOfPrecognition" => Ok("Choose a draw pile card"),
            "LiquidMemories" => Ok("Choose a discard pile card"),
            _ => Err("combat potion choice prompt source is unported"),
        },
        ChoiceSrc::Relic(id) => match id.name() {
            // C# ChoicesParadox.AfterPlayerTurnStart: retained generated card.
            "CHOICES_PARADOX" => Ok("Choose a retained card to add to your hand."),
            // Frozen r7 relics/rare.py:310-347.
            "GAMBLING_CHIP" => Ok("Choose cards to discard"),
            // Frozen r7 relics/shop_event.py:693-729.
            "TOOLBOX" => Ok("Choose a colorless card to add to your hand."),
            // C# ToastyMittens.AfterPlayerTurnStart uses ExhaustSelectionPrompt;
            // frozen r7 shop_event.py supplies this exact prompt.
            "TOASTY_MITTENS" => Ok("Choose a card to Exhaust."),
            _ => Err("combat listener choice prompt source is unported"),
        },
        ChoiceSrc::Monster(id) => match id.name() {
            // C# KnowledgeDemon.ChooseCurse / frozen act2.py's named prompt.
            "KNOWLEDGE_DEMON" => Ok("Curse of Knowledge"),
            _ => Err("combat listener choice prompt source is unported"),
        },
    }
}

fn source_key(choice: &PendingChoice) -> String {
    match choice.resolver.src {
        ChoiceSrc::Card(id) => format!("card:{}", id.name()),
        ChoiceSrc::Potion(id) => format!("potion:{}", id.name()),
        ChoiceSrc::Relic(id) => format!("relic:{}", id.name()),
        ChoiceSrc::Monster(id) => format!("monster:{}", id.name()),
    }
}

fn runtime_prompt(choice: &PendingChoice) -> Result<&'static str, &'static str> {
    // No legacy screen: Dredge is mandatory, Fury permits zero selections.
    // Keep the prompt unavailable until original-DLL v2 evidence qualifies.
    if choice.resolver.src == ChoiceSrc::Card(sts2_sim::tables::CardId::NeowsFury) {
        return if sts2_sim::obs::rule_admitted("combat_choice_prompt", "card:NEOWS_FURY") {
            Ok("Choose optional discard cards to move to hand")
        } else {
            Err("combat card choice prompt source is unported")
        };
    }
    match prompt(choice) {
        Ok(value) => Ok(value),
        Err(_reason) if sts2_sim::obs::rule_admitted("combat_choice_prompt", &source_key(choice)) => Ok(""),
        Err(reason) => Err(reason),
    }
}

pub fn combat_choice_prompt(combat: &Combat) -> Result<&'static str, &'static str> {
    if let Some(choice) = combat.pending_choice_for_player(0) { return runtime_prompt(choice); }
    if combat.pending_choice.is_some() {
        return Err("run lane cannot schedule another player's selector");
    }
    Ok("")
}

#[cfg(feature = "selector-proof")]
pub fn combat_choice_source(combat: &Combat) -> Option<String> {
    let choice = combat.pending_choice_for_player(0)?;
    Some(source_key(choice))
}

#[cfg(feature = "selector-proof")]
pub fn combat_choice_prompt_proof(combat: &Combat) -> &'static str {
    combat.pending_choice_for_player(0)
        .and_then(|choice| prompt(choice).ok())
        .unwrap_or("")
}

#[cfg(feature="selector-proof")]
pub fn combat_choice_prompt_proof_for_player(combat:&Combat,actor:sts2_sim::card::CId)
    -> Result<&'static str,&'static str> {
    combat.pending_choice_for_player(actor).map_or(Ok(""),prompt)
}

#[cfg(test)]
mod tests {
    use super::*;
    use sts2_sim::{choice::Resolver, combat::PileKind, tables::{CardId, MonsterKind, PotionId, RelicId}};
    #[test]
    fn hand_draw_discard_and_potion_prompts_use_source_not_target() {
        for (id, pile, text) in [
            ("ARMAMENTS", PileKind::Hand, "Choose a hand card to upgrade"),
            ("ACROBATICS", PileKind::Hand, "Choose a hand card to discard"),
            ("HAND_TRICK", PileKind::Hand, "Choose a Skill to make Sly this turn"),
            ("BEGONE", PileKind::Hand, "Choose a hand card to transform"),
            ("ENTROPY", PileKind::Hand, "Choose card(s) to transform"),
            ("DUAL_WIELD", PileKind::Hand, "Choose an Attack or Power to copy"),
            ("PURITY", PileKind::Hand, "Choose any number of hand cards to exhaust"),
            ("DREDGE", PileKind::Discard, "Choose discard cards to move to hand"),
            ("GRAVEBLAST", PileKind::Discard, "Choose a discard card to return to hand"),
            ("HOLOGRAM", PileKind::Discard, "Choose a discard card to return to hand"),
            ("WISH", PileKind::Draw, "Choose a card from draw pile"),
            ("ABUNDANCE", PileKind::None, "Choose one upgraded Power"),
            ("DISCOVERY", PileKind::None, "Choose one of three cards"),
            ("STRATAGEM", PileKind::Draw, "Choose card(s) to move to hand"),
            ("SCULPTING_STRIKE", PileKind::Hand, "Choose a hand card to make Ethereal"),
            ("THINKING_AHEAD", PileKind::Hand, "Choose a hand card to put back on draw pile"),
            ("SECRET_WEAPON", PileKind::Draw, "Choose an Attack from draw pile"),
            ("SPLASH", PileKind::None, "Choose one upgraded free attack"),
            ("HEADBUTT", PileKind::Discard, "Choose a discard card to put on top of draw pile"),
            ("DISCOVERY", PileKind::None, "Choose one of three cards"),
            ("SPLASH", PileKind::None, "Choose one upgraded free attack"),
            ("STRATAGEM", PileKind::Draw, "Choose card(s) to move to hand"),
        ] {
            let choice = PendingChoice { player: 0, resolver: Resolver {
                src: ChoiceSrc::Card(CardId::from_name(id).unwrap()), k: 0,
                target: Some(7), step: 0, arg: 0,
            }, source: pile, allow_skip: false, min: 1, max: 1, single: true };
            assert_eq!(prompt(&choice), Ok(text));
        }
        let choice = PendingChoice { player: 0, resolver: Resolver {
            src: ChoiceSrc::Potion(PotionId::from_name("LiquidMemories").unwrap()),
            k: 0, target: None, step: 0, arg: 0,
        }, source: PileKind::Discard, allow_skip: false, min: 1, max: 1, single: true };
        assert_eq!(prompt(&choice), Ok("Choose a discard pile card"));

        let relic_choice = PendingChoice { player: 0, resolver: Resolver {
            src: ChoiceSrc::Relic(RelicId::from_name("GAMBLING_CHIP").unwrap()),
            k: 0, target: None, step: 0, arg: 0,
        }, source: PileKind::Hand, allow_skip: true, min: 0, max: 10, single: false };
        assert_eq!(prompt(&relic_choice), Ok("Choose cards to discard"));
        let mut mittens_choice=relic_choice.clone();
        let mut paradox_choice=relic_choice.clone();
        paradox_choice.resolver.src=ChoiceSrc::Relic(RelicId::ChoicesParadox);
        assert_eq!(prompt(&paradox_choice),Ok("Choose a retained card to add to your hand."));
        mittens_choice.resolver.src=ChoiceSrc::Relic(RelicId::ToastyMittens);
        assert_eq!(prompt(&mittens_choice),Ok("Choose a card to Exhaust."));
        let mut demon_choice=mittens_choice;
        demon_choice.resolver.src=ChoiceSrc::Monster(
            sts2_sim::tables::MonsterKind::KnowledgeDemon);
        assert_eq!(prompt(&demon_choice),Ok("Curse of Knowledge"));

        let toolbox_choice = PendingChoice { player: 0, resolver: Resolver {
            src: ChoiceSrc::Relic(RelicId::from_name("TOOLBOX").unwrap()),
            k: 0, target: None, step: 0, arg: 0,
        }, source: PileKind::None, allow_skip: false, min: 1, max: 1, single: true };
        assert_eq!(prompt(&toolbox_choice), Ok("Choose a colorless card to add to your hand."));

        let knowledge_demon_choice = PendingChoice { player: 0, resolver: Resolver {
            src: ChoiceSrc::Monster(MonsterKind::KnowledgeDemon),
            k: 0, target: Some(0), step: 0, arg: 0,
        }, source: PileKind::None, allow_skip: false, min: 1, max: 1, single: true };
        assert_eq!(prompt(&knowledge_demon_choice), Ok("Curse of Knowledge"));

        let unknown = PendingChoice { player: 0, resolver: Resolver {
            src: ChoiceSrc::Card(CardId::from_name("STRIKE_IRONCLAD").unwrap()),
            k: 0, target: None, step: 0, arg: 0,
        }, source: PileKind::Hand, allow_skip: false, min: 1, max: 1, single: true };
        assert_eq!(source_key(&unknown), "card:STRIKE_IRONCLAD");
        assert_eq!(runtime_prompt(&unknown), Err("combat card choice prompt source is unported"));
        let mut fury = unknown;
        fury.resolver.src = ChoiceSrc::Card(CardId::NeowsFury);
        fury.source = PileKind::Discard;
        fury.min = 0;
        fury.max = 2;
        fury.allow_skip = true;
        fury.single = false;
        assert_eq!(runtime_prompt(&fury), Ok("Choose optional discard cards to move to hand"));
    }
}
