use super::*;
#[test]
fn tangled_preserves_csharp_entangled_amount_in_the_observation(){
    let run=crate::run::RunManager::new(1,crate::run::RunOptions::default()).run_ctx().clone();
    let mut c=Combat::new(run,1);
    let attack=c.new_card(0,CardId::StrikeIronclad,false).unwrap();
    c.players[0].hand.push(attack);
    c.tangled_afflict_card(0,attack);
    assert_eq!(c.cards[attack as usize].cv.affliction,Some(CardAffliction::Entangled));
    assert_eq!(c.cards[attack as usize].observation_affliction_amount(),1);
    assert!(!c.cards[attack as usize].bound);
}
#[test]
fn smog_afflicts_only_eligible_owner_skills_and_clears_only_its_own_marker(){
    let run=crate::run::RunManager::new(1,crate::run::RunOptions::default()).run_ctx().clone();
    let mut c=Combat::new(run,1);
    c.add_player(50,50,3,0,&[]);
    let skill=c.new_card(0,CardId::DefendIronclad,false).unwrap();
    let other=c.new_card(0,CardId::DefendIronclad,false).unwrap();
    let attack=c.new_card(0,CardId::StrikeIronclad,false).unwrap();
    let foreign=c.new_card(1,CardId::DefendIronclad,false).unwrap();
    c.players[0].hand.extend([skill,other,attack]);
    c.players[1].hand.push(foreign);
    c.cards[other as usize].cv.affliction=Some(CardAffliction::Entangled);
    c.creature_apply_power(0,PowerId::Smoggy,1,None,None,false,false);
    c.hist.play_start_cards.push(skill);
    c.fire(Hook::AfterCardPlayed,Ev::Card(skill),None);
    assert_eq!(c.cards[skill as usize].cv.affliction,Some(CardAffliction::Smog));
    assert_eq!(c.cards[skill as usize].cv.affliction_amount,1);
    assert!(!c.cards[skill as usize].bound);
    assert_eq!(c.cards[other as usize].cv.affliction,Some(CardAffliction::Entangled));
    assert_eq!(c.cards[attack as usize].cv.affliction,None);
    assert_eq!(c.cards[foreign as usize].cv.affliction,None);
    assert!(!c.powers_allow_card_play(skill));
    assert!(c.powers_allow_card_play(other),"another affliction prevents Smog, not play");
    let generated=c.new_card(0,CardId::DefendIronclad,false).unwrap();
    c.players[0].draw.push(generated);
    c.fire(Hook::AfterCardEnteredCombat,Ev::Card(generated),None);
    assert_eq!(c.cards[generated as usize].cv.affliction,Some(CardAffliction::Smog));
    c.fire(Hook::AfterTurnEnd,Ev::Side(Side::Player),None);
    assert_eq!(c.cards[skill as usize].cv.affliction,None);
    assert_eq!(c.cards[skill as usize].cv.affliction_amount,0);
    assert_eq!(c.cards[other as usize].cv.affliction,Some(CardAffliction::Entangled));
    assert_eq!(c.cards[generated as usize].cv.affliction,None);
    assert!(c.powers_allow_card_play(skill));
}
