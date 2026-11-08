use sts2_native_obs::*;
use sts2_sim::{combat::Combat, rng::Rng, run::{Coord, RunManager, RunOptions,
    mp::{Character, MpPhase, MpRun, PlayerSpec}}, tables::*};
use sha2::{Digest, Sha256};
#[path = "v1_payload.rs"]
mod v1_payload;

pub fn fingerprint(rows: RowsView<'_>, candidates: CandidatesView<'_>, edges: &[Edge],
    status: DecisionStatus, scope: DecisionScope) -> String {
    let bytes = serde_json::to_vec(&v1_payload::fields(rows, candidates, edges, status, scope)).unwrap();
    Sha256::digest(bytes).iter().map(|b| format!("{b:02x}")).collect()
}

pub fn v2_fingerprint(view: &ObservationView<'_>) -> String {
    let original = fingerprint(view.rows, view.candidates, view.edges, view.status, view.scope);
    let payload = format!("{original}/{:?}/{:?}/{:?}",
        view.rows.legacy_role, view.rows.multiplicity, view.streamed_piles);
    Sha256::digest(payload.as_bytes()).iter().map(|b| format!("{b:02x}")).collect()
}

pub fn real_states(legacy: bool, mut visit: impl FnMut(&str, &NativeObsV1)) {
    let make_encoder = || {
        let encoder = NativeEncoder::new();
        if legacy { encoder.with_legacy_projection() } else { encoder }
    };
    real_states_with(make_encoder, &mut visit);
}

pub fn real_states_with(make_encoder: impl Fn() -> NativeEncoder, mut visit: impl FnMut(&str, &NativeObsV1)) {
    for character in [Character::Ironclad, Character::Silent, Character::Defect, Character::Necrobinder, Character::Regent] {
        let mut run = RunManager::new_for_character(202610290000, RunOptions::default(), character);
        let mut encoder = make_encoder(); let mut obs = NativeObsV1::new();
        let mut decision = DecisionId { episode: 7, generation: 0, actor: 0 }; let mut last = None;
        for _ in 0..32 {
            encoder.encode_solo(&run, decision, &mut obs).unwrap();
            if last != Some(run.phase()) {
                last = Some(run.phase()); visit(&format!("solo/{character:?}/{:?}", run.phase()), &obs);
            }
            if run.combat().is_some() { break; }
            let v = obs.view().unwrap();
            let action = (0..v.candidate_len).find(|&i| v.candidates.sampleable(i, v.candidate_len)).unwrap();
            encoder.dispatch_solo(&mut run, &mut decision, &mut obs, action).unwrap();
        }
    }
    for seats in [2, 4] { for kind in [MapPointType::Shop, MapPointType::RestSite] {
        let specs: Vec<_> = (0..seats).map(|seat| PlayerSpec { net_id: 10 + seat as u64,
            character: [Character::Ironclad, Character::Silent, Character::Defect, Character::Regent][seat] }).collect();
        let mut run = MpRun::new("LEGACY-PROJECTION-REAL-ROOMS", &specs, 10).unwrap();
        for seat in 0..seats {
            let player = run.roster.player_mut(seat).unwrap(); player.gold = 500;
            player.hp = 77; player.max_hp = 80;
            let mut relic = sts2_sim::relic::Relic::new(RelicId::RegalPillow); relic.owner = seat as u8;
            player.relics.push(relic);
        }
        run.phase = MpPhase::Map; let (x, y) = run.world.run.coord.unwrap();
        let child = run.world.map.point(Coord::new(x, y)).unwrap().children[0] as usize;
        run.world.map.points[child].point_type = kind; let coord = run.world.map.points[child].coord;
        for seat in 0..seats { run.vote(10 + seat as u64, seat, coord).unwrap(); }
        let mut encoder = make_encoder(); let mut obs = NativeObsV1::new();
        for seat in 0..seats {
            encoder.encode_multiplayer(&run, DecisionId { episode: 7, generation: 0, actor: seat as u8 },
                10 + seat as u64, &mut obs).unwrap();
            visit(&format!("mp{seats}/{kind:?}/seat{seat}"), &obs);
        }
    } }
    let mut ctx = RunManager::new(7, RunOptions::default()).run_ctx().clone();
    ctx.deck.clear(); ctx.relics.clear(); ctx.potions.clear(); ctx.max_potion_slots = 0;
    let mut c = Combat::new(ctx, 7); c.setup_encounter("TUNNELER_WEAK", &mut Rng::new(1));
    c.players[0].hand.clear(); c.players[0].draw.clear(); c.players[0].discard.clear();
    let card = c.new_card(0, CardId::BodySlam, false).unwrap(); c.players[0].hand.push(card);
    c.cr_mut(0).block = 17;
    let mut encoder = make_encoder(); let mut obs = NativeObsV1::new();
    encoder.encode_combat_snapshot(&c, &[Character::Ironclad], DecisionId { episode: 7, generation: 0, actor: 0 }, 1, &mut obs).unwrap();
    visit("combat/body-slam-dynamic", &obs);
}
