use super::{EventOption, EventState};
use crate::tables::RelicId;

pub(crate) fn random(event: &mut EventState) -> RelicId {
    let dolls = [RelicId::DaughterOfTheWind, RelicId::MrStruggles, RelicId::BingBong];
    dolls[event.rng.choice_index(dolls.len())]
}

pub(crate) fn offer(event: &mut EventState, count: usize) {
    let mut dolls = [RelicId::BingBong, RelicId::DaughterOfTheWind, RelicId::MrStruggles];
    event.rng.shuffle(&mut dolls);
    event.relics.clear();
    event.options.clear();
    for (i, relic) in dolls.into_iter().take(count).enumerate() {
        event.relics.push(relic);
        let label = match relic {
            RelicId::BingBong => "Bing Bong",
            RelicId::DaughterOfTheWind => "Daughter of the Wind",
            _ => "Mr Struggles",
        };
        event.options.push(EventOption::new(["doll_1", "doll_2", "doll_3"][i], label, "Take this doll relic"));
    }
}
