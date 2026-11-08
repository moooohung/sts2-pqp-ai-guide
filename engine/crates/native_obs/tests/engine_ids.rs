use sts2_native_obs::{ContentId, EnchantmentId, EventId, EventOptionId, EventPageId, Namespace};
use sts2_sim::{card::Card, run::{RunManager, RunOptions}, tables::CardId};

#[test]
fn engine_and_observation_share_the_same_types_and_ordinals() {
    let enchantment: sts2_sim::identity::EnchantmentId = EnchantmentId::Sharp;
    let event: sts2_sim::identity::EventId = EventId::Neow;
    let page: sts2_sim::identity::EventPageId = EventPageId::NeowInitial;
    let option: sts2_sim::identity::EventOptionId = EventOptionId::NeowPageNameRelicIdEntry;
    for (id, namespace, ordinal) in [
        (ContentId::from(enchantment), Namespace::EnchantmentId, enchantment.index()),
        (ContentId::from(event), Namespace::EventId, event.index()),
        (ContentId::from(page), Namespace::EventPageId, page.index()),
        (ContentId::from(option), Namespace::EventOptionId, option.index()),
    ] {
        assert_eq!(id, ContentId::from_index(namespace, ordinal).unwrap());
        assert_eq!(usize::from(id.value), ordinal + 1);
    }
}

#[test]
fn enchantment_ingestion_is_typed_and_rejects_unknown_names_without_mutation() {
    let mut card = Card::from_factory(CardId::StrikeIronclad, false, 0).unwrap();
    assert!(!card.apply_static_enchantment("not-an-enchantment", 99));
    assert!(card.enchantments.is_empty());
    assert!(card.apply_static_enchantment(EnchantmentId::Sharp, 2));
    assert_eq!(card.enchantments.as_slice(), &[(EnchantmentId::Sharp, 2)]);
    assert_eq!(format!("{:?}", card.enchantments.as_slice()), "[(\"Sharp\", 2)]");
    assert!(!card.apply_static_enchantment("not-an-enchantment", 99));
    assert_eq!(card.enchantments.as_slice(), &[(EnchantmentId::Sharp, 2)]);
}

#[test]
fn retained_event_identity_agrees_with_the_legacy_snapshot_boundary() {
    let run = RunManager::new(1, RunOptions::default());
    assert_eq!(run.native_event_id().map(EventId::name), run.current_event_id());
    for event in EventId::ALL {
        assert_eq!(EventId::from_name(event.name()), Some(event));
    }
    for enchantment in EnchantmentId::ALL {
        assert_eq!(EnchantmentId::from_name(enchantment.name()), Some(enchantment));
    }
}
