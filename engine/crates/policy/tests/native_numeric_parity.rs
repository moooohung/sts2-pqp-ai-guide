#![cfg(feature="obs-v2")]

use sts2_native_obs::{DecisionId,NativeObsV1};
use sts2_policy::native::{NativeDecision,encoder_for_policy};
use sts2_sim::run::{RunManager,RunOptions,mp::Character};

#[test]
fn seed400_observation_numbers_round_identically_in_v1_and_v2() {
    let game=RunManager::new_for_character(400,RunOptions {
        ascension:10,saved_wongo_points:Some(0),..RunOptions::default()
    },Character::Ironclad);
    for (relational,lane) in [(false,5),(true,1)] {
        let mut obs=Box::new(NativeObsV1::new());
        encoder_for_policy(relational).encode_solo(&game,DecisionId::default(),&mut obs).unwrap();
        let view=obs.view().unwrap();
        assert_eq!(view.rows.integers[2][lane],6);
        let decision=NativeDecision::from_view(view).unwrap();
        let input:serde_json::Value=serde_json::from_str(&decision.trajectory_json(relational).unwrap()).unwrap();
        let number=input["rows"][2]["numbers"][lane].as_f64().unwrap() as f32;
        // ln(7) = 1.9459101490553133..., whose nearest binary32 is 0x3ff91395.
        assert_eq!(number.to_bits(),0x3ff91395,
            "seed 400 relational={relational} row=2 lane={lane}");
    }
}
