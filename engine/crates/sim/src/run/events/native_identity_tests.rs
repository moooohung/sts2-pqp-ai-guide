//! Expectations follow original event localization keys and custom C# symbols.
use super::*;
use crate::identity::{ChoicePurpose, EffectId, EventId, EventOptionId as O, EventPageId as P};
use super::native_identity::{EventOptionIdentity, EventParameter as A};

#[test]
fn merged_tanx_and_tezcatara_reward_callbacks_keep_native_identities() {
    for (event_id, keys) in [
        (crate::identity::EventId::Tanx, &["relic_1", "relic_2", "relic_3"][..]),
        (crate::identity::EventId::Tezcatara, &["wax_0", "wax_1", "wax_2", "wax_3", "wax_4", "skip_wax"][..]),
    ] {
        let mut event = EventState::new(event_id, Rng::new(7));
        for (index, key) in keys.iter().enumerate() {
            event.options.push(EventOption::new(key, "", ""));
            if *key != "skip_wax" { event.relics.push(if index % 2 == 0 { RelicId::Girya } else { RelicId::HappyFlower }); }
        }
        for (index, option) in event.options.iter().enumerate() {
            assert_eq!(option.option_id.legacy_id(), keys[index]);
            let expected = event.relics.get(index).copied().map(super::native_offer::NativeEventOffer::Relic);
            assert_eq!(event.native_offer(option.option_id, &[]), expected);
        }
        if event_id == crate::identity::EventId::Tezcatara {
            event.relics.remove(0); event.options.remove(0);
            assert_eq!(event.native_offer(event.options[0].option_id, &[]),
                Some(super::native_offer::NativeEventOffer::Relic(event.relics[0])));
        }
    }
}

#[test]
fn implemented_initial_menus_retain_only_owned_numeric_identities() {
    for id in EventId::ALL {
        if let Some(event) = initialize(id, 19, 1000, 24, 70, 80) {
            assert_eq!(event.options.page.id.event(), id);
            for option in &event.options {
                assert_eq!(option.native_id().event(), id);
                assert_ne!(option.native_id().wire_id(), 0);
            }
        }
    }
}

#[test]
fn flower_pages_and_option_parameters_follow_original_source_transitions() {
    let mut event = initialize(EventId::ColossalFlower, 19, 0, 0, 70, 80).unwrap();
    assert_eq!(event.options.page.id, P::ColossalFlowerInitial);
    assert_eq!(event.options[0].native_id(), O::ColossalFlowerInitialExtractCurrentPrizeNumberOfDigs1);
    assert_eq!(event.options[0].option_id.parameter, A::Counter(1));
    let next = choose(&mut event, "reach_deeper").unwrap();
    event.options.replace(next.next_options);
    assert_eq!(event.options.page.id, P::ColossalFlowerReachDeeperNumberOfDigs);
    assert_eq!(event.options.page.parameter, A::Counter(1));
    assert_eq!(event.options[0].native_id(), O::ColossalFlowerReachDeeperNumberOfDigsExtractCurrentPrizeNumberOfDigs1);
    assert_eq!(event.options[0].option_id.parameter, A::Counter(2));
    let next = choose(&mut event, "reach_deeper").unwrap();
    event.options.replace(next.next_options);
    assert_eq!(event.options.page.id, P::ColossalFlowerReachDeeper2);
    assert_eq!(event.options[0].native_id(), O::ColossalFlowerReachDeeper2ExtractInstead);
    assert_eq!(event.options[1].native_id(), O::ColossalFlowerReachDeeper2PollinousCore);
}

#[test]
fn bridge_template_keeps_page_and_option_suffixes_separate_including_loop() {
    let mut options = EventOptions::new(EventId::SlipperyBridge);
    options.set_page(P::SlipperyBridgeHoldOnHoldOnSuffix, A::Bridge(Some(0)));
    options.push(EventOption::new("hold_on", "Hold On", ""));
    assert_eq!(options.page.parameter, A::Bridge(Some(0)));
    assert_eq!(options[0].option_id.parameter, A::Bridge(Some(1)));
    options.clear();
    options.set_page(P::SlipperyBridgeHoldOnHoldOnSuffix, A::Bridge(Some(6)));
    options.push(EventOption::new("hold_on", "Hold On", ""));
    assert_eq!(options[0].option_id.parameter, A::Bridge(None));
    options.clear();
    options.set_page(P::SlipperyBridgeHoldOnHoldOnSuffix, A::Bridge(None));
    options.push(EventOption::new("hold_on", "Hold On", ""));
    assert_eq!(options[0].option_id.parameter, A::Bridge(None));
}

#[test]
fn crystal_actions_keep_one_source_identity_and_exact_tool_and_coordinates() {
    let sphere = SphereState::new(&mut Rng::new(7), 3);
    let options = sphere.options();
    assert_eq!(options.len(), sphere.hidden.iter().filter(|&&hidden| hidden).count() * 2);
    for pair in options.chunks_exact(2) {
        let A::Sphere { big, x, y } = pair[0].option_id.parameter else { panic!("typed grid action") };
        assert!(big && sphere.hidden[SphereState::cell(x as usize, y as usize)]);
        assert_eq!(pair[0].native_id(), O::CrystalSphereCrystalSphereMinigameCellClicked);
        assert_eq!(pair[1].option_id.parameter, A::Sphere { big: false, x, y });
    }
    assert_eq!(NativeEventOption::sphere(true, 2, 4).option_id.legacy_id(), "sphere_big_2_4");
    assert_eq!(NativeEventOption::sphere(false, 2, 4).option_id.legacy_id(), "sphere_small_2_4");
}

#[test]
fn retained_offers_are_distinct_without_expanding_parameterized_vocabulary() {
    let mut event = EventState::new(EventId::FakeMerchant, Rng::new(7));
    event.options.push(EventOption::new("buy_0", "Relic", ""));
    event.options.push(EventOption::new("buy_1", "Relic", ""));
    assert_eq!(event.options.page.id, P::FakeMerchantNFakeMerchant);
    assert_eq!(event.options[0].native_id(), O::FakeMerchantNFakeMerchantMerchantRelicEntry);
    assert_eq!(event.options[1].native_id(), event.options[0].native_id());
    assert_eq!(event.options[0].option_id.parameter, A::Offer(0));
    assert_eq!(event.options[1].option_id.parameter, A::Offer(1));
    assert_eq!(EventOptionIdentity::neow(2).id, O::NeowPageNameRelicIdEntry);
    assert_eq!(EventOptionIdentity::neow(2).parameter, A::Offer(2));
}

#[test]
fn locked_option_id_tracks_current_eligibility() {
    let mut event = initialize(EventId::SelfHelpBook, 19, 0, 0, 70, 80).unwrap();
    assert_eq!(event.options[0].native_id(), O::SelfHelpBookInitialReadTheBack);
    event.options[0].enabled = false;
    assert_eq!(event.options[0].native_id(), O::SelfHelpBookInitialReadTheBackLocked);
    event.options[0].enabled = true;
    assert_eq!(event.options[0].native_id(), O::SelfHelpBookInitialReadTheBack);
}

#[test]
fn effect_and_choice_metadata_come_from_retained_native_variants() {
    assert_eq!(EventEffect::LoseGold(125).native_effect_id(), EffectId::EventEffectLoseGold);
    let effect = RunChoiceEffect::Enchant { name: crate::identity::EnchantmentId::Swift, amount: 2 };
    assert_eq!(effect.native_effect_id(), EffectId::RunChoiceEffectEnchant);
    assert_eq!(effect.native_purpose(), ChoicePurpose::RunChoiceEffectEnchant);
}

#[test]
#[should_panic(expected = "event option lacks an original-source identity")]
fn unknown_option_fails_before_any_native_row_can_be_published() {
    EventOptions::new(EventId::AbyssalBaths).push(EventOption::new("unknown", "", ""));
}

