use crate::{combat::Combat, card::CId, identity::EncounterId, rng::Rng, tables::*};

pub(crate) fn setup(c: &mut Combat, id: EncounterId, _rng: &mut Rng) -> bool {
    let kind = match id {
        EncounterId::BattlewornDummyEventV1Encounter => MonsterKind::BattleFriendV1,
        EncounterId::BattlewornDummyEventV2Encounter => MonsterKind::BattleFriendV2,
        EncounterId::BattlewornDummyEventV3Encounter => MonsterKind::BattleFriendV3,
        _ => return false,
    };
    let (cr, ai) = c.create_monster(kind, 0);
    c.add_enemy(cr, ai);
    true
}

pub(crate) fn move_effect(_c: &mut Combat, _e: CId, kind: MonsterKind, state: &str) -> bool {
    matches!(kind, MonsterKind::BattleFriendV1 | MonsterKind::BattleFriendV2 | MonsterKind::BattleFriendV3)
        && state == "NOTHING_MOVE"
}

