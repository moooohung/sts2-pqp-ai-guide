use super::*;

#[test]
fn darv_pickup_options_round_trip_without_aliasing_initial_offers() {
    let page=EventOptions::new(EventId::Darv).page;
    let keys=["relic_1","relic_2","relic_3","bell_0","bell_1","bell_2","skip_bell"];
    let identities:Vec<_>=keys.iter().map(|key|
        EventOptionIdentity::from_legacy(EventId::Darv,page,key)).collect();
    for (index,(identity,key)) in identities.iter().zip(keys).enumerate() {
        assert_eq!(identity.id,EventOptionId::DarvPageNameRelicIdEntry);
        assert_eq!(identity.parameter,EventParameter::Offer(index as u16));
        assert_eq!(identity.legacy_id(),key);
        assert!(!identities[..index].contains(identity));
    }
}

#[test]
fn calling_bell_rejects_out_of_range_or_wrong_parent_options() {
    let page=EventOptions::new(EventId::Darv).page;
    for key in ["bell_3","bell_99","bell_-1","bell_00","relic_0","relic_4"] {
        assert!(std::panic::catch_unwind(||
            EventOptionIdentity::from_legacy(EventId::Darv,page,key)).is_err(),"{key}");
    }
    let page=EventOptions::new(EventId::Orobas).page;
    assert!(std::panic::catch_unwind(||
        EventOptionIdentity::from_legacy(EventId::Orobas,page,"bell_0")).is_err());
}
