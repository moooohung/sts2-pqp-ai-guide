//! Shared identity generator: the simulator owns the types; NativeObsV1 reexports them.
//! Run from the simulator directory, alongside its existing content-table generator.
use serde_json::Value;
use std::{collections::{BTreeMap, BTreeSet}, fmt::Write, fs, path::Path};

fn source(path: &str) -> syn::File {
    println!("cargo:rerun-if-changed={path}");
    syn::parse_file(&fs::read_to_string(path).unwrap()).unwrap()
}

fn variants(path: &str, name: &str) -> Vec<String> {
    source(path).items.into_iter().find_map(|item| match item {
        syn::Item::Enum(item) if item.ident == name => Some(item.variants.into_iter()
            .map(|v| v.ident.to_string()).collect()),
        _ => None,
    }).unwrap_or_else(|| panic!("missing native enum {path}::{name}"))
}

fn identity_method(path: &str, name: &str, method: &str, target: &str,
    prefix: &str, renames: &[(&str, &str)]) -> String {
    let item = source(path).items.into_iter().find_map(|item| match item {
        syn::Item::Enum(item) if item.ident == name => Some(item),
        _ => None,
    }).unwrap_or_else(|| panic!("missing native enum {path}::{name}"));
    let mut code = format!("impl {name} {{ pub(crate) const fn {method}(&self) -> crate::identity::{target} {{ match self {{\n");
    for variant in item.variants {
        let member = variant.ident.to_string();
        let pattern = match variant.fields {
            syn::Fields::Unit => "",
            syn::Fields::Unnamed(_) => "(..)",
            syn::Fields::Named(_) => " { .. }",
        };
        let mapped = renames.iter().find_map(|(from, to)| (*from == member).then_some(*to))
            .unwrap_or(&member);
        writeln!(code, "Self::{member}{pattern} => crate::identity::{target}::{prefix}{mapped},").unwrap();
    }
    code.push_str("} } }\n");
    code
}

fn enchantments() -> Vec<String> {
    for item in source("src/card.rs").items {
        if let syn::Item::Const(item) = item && item.ident == "ENCHANTMENT_NAMES" {
            let syn::Expr::Reference(value) = *item.expr else { panic!("enchantment names reference"); };
            let syn::Expr::Array(values) = *value.expr else { panic!("enchantment names array"); };
            return values.elems.into_iter().map(|value| {
                let syn::Expr::Lit(value) = value else { panic!("enchantment name literal"); };
                let syn::Lit::Str(value) = value.lit else { panic!("enchantment name string"); };
                value.value()
            }).collect();
        }
    }
    panic!("missing native enchantment names")
}

pub fn generate(out: &Path) {
    let catalog = "../native_obs/data/original_ids.json";
    println!("cargo:rerun-if-changed={catalog}");
    let original: Value = serde_json::from_slice(&fs::read(catalog).unwrap()).unwrap();
    let mut registries: BTreeMap<&str, Vec<(String, String)>> = BTreeMap::new();
    for (name, field) in [("EventId", "events"), ("EventPageId", "pages"), ("EventOptionId", "options"), ("PublicBossId", "public_bosses")] {
        registries.insert(name, original[field].as_array().unwrap().iter().map(|row|
            (row["name"].as_str().unwrap().into(), row["key"].as_str().unwrap().into())).collect());
    }
    registries.insert("EnchantmentId", enchantments().into_iter().map(|name| (name.clone(), name)).collect());
    let mut rewards: Vec<_> = original["reward_kinds"].as_array().unwrap().iter().map(|name| {
        let name = name.as_str().unwrap().to_owned(); (name.clone(), name)
    }).collect();
    assert!(variants("src/run/manager.rs", "RewardItemKind").iter().any(|name| name == "CardBundles"));
    rewards.push(("CardBundles".into(), "CardBundles".into()));
    registries.insert("RewardKind", rewards);
    for (name, sources) in [
        ("ChoicePurpose", &[("src/run/manager.rs", "RunDeckChoiceKind"),
            ("src/run/events/mod.rs", "RunChoiceEffect"), ("src/run/mp/events.rs", "EventCardAction"),
            ("src/choice.rs", "ChoiceSrc"), ("src/run/mp/native_view.rs", "NativeModalKind")][..]),
        ("EffectId", &[("src/run/events/mod.rs", "EventEffect"),
            ("src/run/events/mod.rs", "RunChoiceEffect")][..]),
    ] {
        registries.insert(name, sources.iter().flat_map(|(path, prefix)| variants(path, prefix)
            .into_iter().map(move |variant| { let name = format!("{prefix}{variant}"); (name.clone(), name) })).collect());
    }
    let mut code = String::from("// @generated from native definitions and the original-DLL identity catalog.\n");
    for (name, members) in registries {
        assert!(!members.is_empty() && members.len() < u16::MAX as usize);
        let mut seen_names = BTreeSet::new();
        let mut seen_keys = BTreeSet::new();
        for (member, key) in &members {
            syn::parse_str::<syn::Ident>(member).expect("Rust identity");
            assert!(seen_names.insert(member) && seen_keys.insert(key), "duplicate identity in {name}");
        }
        writeln!(code, "#[repr(u16)]\n#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]\npub enum {name} {{").unwrap();
        for (index, (member, _)) in members.iter().enumerate() { writeln!(code, "{member} = {index},").unwrap(); }
        writeln!(code, "}}\nimpl {name} {{\npub const COUNT: usize = {};", members.len()).unwrap();
        writeln!(code, "pub const ALL: [Self; Self::COUNT] = [{}];", members.iter()
            .map(|(member, _)| format!("Self::{member}")).collect::<Vec<_>>().join(",")).unwrap();
        writeln!(code, "pub const fn index(self) -> usize {{ self as usize }}\n\
            pub const fn wire_id(self) -> u16 {{ self as u16 + 1 }}\n\
            pub fn from_index(index: usize) -> Option<Self> {{ Self::ALL.get(index).copied() }}").unwrap();
        writeln!(code, "pub const fn name(self) -> &'static str {{ match self {{ {} }} }}", members.iter()
            .map(|(member, key)| format!("Self::{member} => {key:?},")).collect::<String>()).unwrap();
        writeln!(code, "pub fn from_name(name: &str) -> Option<Self> {{ match name {{ {} _ => None }} }}\n}}", members.iter()
            .map(|(member, key)| format!("{key:?} => Some(Self::{member}),")).collect::<String>()).unwrap();
        // Preserve legacy snapshot text at the serialization boundary. Numeric consumers
        // use index()/wire_id(); they never stringify an identity to find its ordinal.
        writeln!(code, "impl core::fmt::Debug for {name} {{ fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {{ core::fmt::Debug::fmt(self.name(), f) }} }}").unwrap();
        writeln!(code, "impl core::fmt::Display for {name} {{ fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {{ f.write_str(self.name()) }} }}").unwrap();
        writeln!(code, "impl TryFrom<&str> for {name} {{ type Error = (); fn try_from(name: &str) -> Result<Self, ()> {{ Self::from_name(name).ok_or(()) }} }}").unwrap();
    }
    for (name, field) in [("EventPageId", "pages"), ("EventOptionId", "options")] {
        writeln!(code, "impl {name} {{ pub const fn event(self) -> EventId {{ match self {{").unwrap();
        for row in original[field].as_array().unwrap() {
            let key = row["key"].as_str().unwrap();
            let event = original["events"].as_array().unwrap().iter().find(|event|
                key.strip_prefix(event["key"].as_str().unwrap()).is_some_and(|tail| tail.starts_with('.')))
                .expect("every page and option belongs to an original event");
            writeln!(code, "Self::{} => EventId::{},", row["name"].as_str().unwrap(),
                event["name"].as_str().unwrap()).unwrap();
        }
        code.push_str("} } }\n");
    }
    code.push_str("impl EventId { pub const fn initial_page(self) -> EventPageId { match self {\n");
    for event in original["events"].as_array().unwrap() {
        let key = format!("{}.INITIAL", event["key"].as_str().unwrap());
        let page = original["pages"].as_array().unwrap().iter().find(|page| page["key"] == key)
            .expect("original EventModel supplies every initial page");
        writeln!(code, "Self::{} => EventPageId::{},", event["name"].as_str().unwrap(),
            page["name"].as_str().unwrap()).unwrap();
    }
    code.push_str("} } }\n");
    // Resolve the existing native state-table ordinals at build time. Runtime
    // intent projection never converts state names into a second vocabulary.
    println!("cargo:rerun-if-changed=data/monsters.json");
    let monsters: Value = serde_json::from_slice(&fs::read("data/monsters.json").unwrap()).unwrap();
    for (name, factory, state) in [
        ("NATIVE_FORGOTTEN_DREAD", "create_the_forgotten", "DREAD"),
        ("NATIVE_WATERFALL_PRESSURE", "create_waterfall_giant", "PRESSURE_GUN_MOVE"),
        ("NATIVE_WATERFALL_ERUPTION", "create_waterfall_giant", "EXPLODE_MOVE"),
        ("NATIVE_TEST_SUBJECT_CLAW", "create_test_subject", "MULTI_CLAW_MOVE"),
    ] {
        let indices = ["a0", "a10", "a8"].map(|band| {
            let states = monsters["monsters"][factory][band]["states"].as_object().unwrap();
            assert_eq!(states[state]["kind"], "move");
            u8::try_from(states.keys().position(|key| key == state).unwrap()).unwrap()
        });
        writeln!(code, "pub(crate) const {name}: [u8; 3] = {indices:?};").unwrap();
    }
    fs::write(out.join("gen_native_ids.rs"), code).unwrap();
    for (file, methods) in [
        ("gen_event_identity.rs", vec![
            identity_method("src/run/events/mod.rs", "EventEffect", "native_effect_id", "EffectId", "EventEffect", &[]),
            identity_method("src/run/events/mod.rs", "RunChoiceEffect", "native_effect_id", "EffectId", "RunChoiceEffect", &[]),
            identity_method("src/run/events/mod.rs", "RunChoiceEffect", "native_purpose", "ChoicePurpose", "RunChoiceEffect", &[]),
        ]),
        ("gen_manager_identity.rs", vec![
            identity_method("src/run/manager.rs", "RewardItemKind", "native_kind", "RewardKind", "", &[("CardRemoval", "RemoveCard")]),
            identity_method("src/run/manager.rs", "RunDeckChoiceKind", "native_purpose", "ChoicePurpose", "RunDeckChoiceKind", &[]),
        ]),
        ("gen_mp_event_identity.rs", vec![
            identity_method("src/run/mp/events.rs", "EventCardAction", "native_purpose", "ChoicePurpose", "EventCardAction", &[]),
        ]),
    ] {
        fs::write(out.join(file), methods.join("\n")).unwrap();
    }
}
