use super::*;

impl MpRun {
    pub fn rest_cook_choices(&self, net_id: u64, seat: usize) -> Result<Option<&[u32]>, &'static str> {
        self.roster.actor(net_id, seat)?;
        if self.phase != MpPhase::RestSite { return Err("not at rest site"); }
        Ok(self.rest_sites[seat].cook.as_deref())
    }

    /// CookRestSiteOption.OnSelect awaits a cancelable, manually confirmed pair.
    pub fn select_rest_cook(&mut self, net_id: u64, seat: usize, indices: &[usize])
        -> Result<(), &'static str> {
        let choices = self.rest_cook_choices(net_id, seat)?.ok_or("rest cook selection missing")?;
        if !indices.is_empty() && (indices.len() != 2 || indices[0] == indices[1]
            || indices.iter().any(|i| *i >= choices.len())) {
            return Err("invalid cooked cards");
        }
        let ids: Vec<_> = indices.iter().map(|i| choices[*i]).collect();
        let mut next = self.clone();
        next.rest_sites[seat].cook = None;
        if !ids.is_empty() {
            let player = next.roster.player_mut(seat).unwrap();
            if ids.iter().any(|id| !player.deck.iter().any(|card| card.instance_id == *id)) {
                return Err("cooked card missing");
            }
            player.retain_deck_cards(|card| !ids.contains(&card.instance_id));
            player.max_hp += 5;
            player.hp += 5;
            next.finish_rest_choice(seat, crate::run::rest::RestOptionKind::Cook);
        }
        *self = next;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use crate::run::mp::*;
    use crate::{relic::Relic, tables::{RelicId, CardId}, card::Card};
    #[test]
    fn cook_requires_two_owned_removable_cards_and_cancel_preserves_rest() {
        let specs = [PlayerSpec { net_id: 1, character: Character::Ironclad },
            PlayerSpec { net_id: 2, character: Character::Silent }];
        let mut run = MpRun::new("83003", &specs, 0).unwrap();
        let mut cleaver = Relic::new(RelicId::MeatCleaver); cleaver.owner = 1;
        run.roster.player_mut(1).unwrap().relics.push(cleaver);
        let mut fixed = Card::from_factory(CardId::AscendersBane, false, 900).unwrap(); fixed.owner = 1;
        run.roster.player_mut(1).unwrap().deck.push(fixed);
        run.enter_rest_site().unwrap();
        assert!(!run.rest_options(1, 0).unwrap().iter().any(|o| o.0 == "COOK"));
        let index = run.rest_options(2, 1).unwrap().iter().position(|o| o.0 == "COOK").unwrap();
        let before = format!("{:?}", run.roster.player(1).unwrap());
        let other = format!("{:?}", run.roster.player(0).unwrap());
        run.choose_rest_option(2, 1, index).unwrap();
        let domain = run.census_menu().unwrap().into_iter().find(|d| d.decision.kind == "cook").unwrap();
        assert_eq!((domain.min, domain.max, domain.skip), (2, 2, true));
        run.census_take(&domain.decision, &[]).unwrap();
        assert_eq!(format!("{:?}", run.roster.player(1).unwrap()), before);
        run.choose_rest_option(2, 1, index).unwrap();
        let domain = run.census_menu().unwrap().into_iter().find(|d| d.decision.kind == "cook").unwrap();
        assert!(run.census_take(&domain.decision, &[0]).is_err());
        assert!(run.census_take(&domain.decision, &[0, 0]).is_err());
        run.census_take(&domain.decision, &[1, 0]).unwrap();
        let player = run.roster.player(1).unwrap();
        assert_eq!(player.max_hp, 75);
        assert_eq!(player.hp, 75);
        assert!(player.deck.iter().any(|c| c.id == CardId::AscendersBane));
        assert_eq!(format!("{:?}", run.roster.player(0).unwrap()), other);
        assert!(run.rest_options(2, 1).unwrap().is_empty());
    }
}
