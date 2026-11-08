use sts2_sim::{
    combat::Combat, run::{RunManager, RunOptions}, tables::{CardId, PotionId},
};

#[test]
fn distilled_chaos_uses_owner_or_explicit_recipient_for_all_four_seats() {
    for owner in 0..4 {
        for target in [None, Some((owner + 1) % 4)] {
            let run = RunManager::new(1, RunOptions::default()).run_ctx().clone();
            let mut c = Combat::new(run, 1);
            for _ in 1..4 { c.add_player(50, 50, 3, 0, &[]); }
            for p in 0..4 {
                c.players[p].hand.clear();
                c.players[p].draw.clear();
            }
            let recipient = target.unwrap_or(owner);
            let other_owner = (recipient + 1) % 4;
            c.setup_encounter("SLUDGE_SPINNER_WEAK", &mut sts2_sim::rng::Rng::new(1));
            let other = c.new_card(other_owner, CardId::DefendIronclad, false).unwrap();
            c.players[other_owner].draw.push(other);
            for id in [CardId::AscendersBane, CardId::DefendIronclad, CardId::DefendIronclad] {
                let q = c.new_card(recipient, id, false).unwrap();
                c.players[recipient].draw.push(q);
            }
            c.potion_effect_for_player(owner, PotionId::Distilledchaos, target);
            assert_eq!(c.players[other_owner].draw.as_slice(), &[other]);
            assert_eq!(c.cr(other_owner).block, 0);
            assert_eq!(c.cr(recipient).block, 10);
            assert!(c.players[recipient].play.is_empty());
            assert_eq!(c.players[recipient].discard.len(), 3);
            assert_eq!(c.fault, None);
        }
    }
}
