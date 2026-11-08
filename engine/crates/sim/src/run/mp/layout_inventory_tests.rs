use crate::identity::EventId;
use super::*;
use crate::run::mp::Character;

#[test]
fn dormant_punch_off_layout_does_not_block_owner_inventory_discard() {
    let specs=[PlayerSpec{net_id:100,character:Character::Silent},
        PlayerSpec{net_id:101,character:Character::Necrobinder}];
    let mut run=MpRun::new("7300000804",&specs,4).unwrap();
    run.phase=MpPhase::Event;
    run.world.current_room=Some("PUNCH_OFF");
    run.roster.player_mut(0).unwrap().potions[0]=Some(PotionId::Firepotion);
    run.roster.player_mut(1).unwrap().potions[0]=Some(PotionId::Blockpotion);
    run.begin_generic_event(EventId::PunchOff).unwrap();
    assert!(run.combat.is_some());
    let layout=format!("{:?}",run.combat);
    assert!(run.discard_potion(101,0,0).is_err());
    run.discard_potion(100,0,0).unwrap();
    assert!(run.roster.player(0).unwrap().potions[0].is_none());
    assert_eq!(run.roster.player(1).unwrap().potions[0],Some(PotionId::Blockpotion));
    assert_eq!(format!("{:?}",run.combat),layout);
}
