//! WelcomeToWongos.cs: owner-specific shop bags, profile input and downgrade.
use super::MpRun;
use crate::{identity::EventOptionId, run::events::{EventOption,EventOptionIdentity,EventState}, tables::{RelicId,RELIC_INFO}};

impl MpRun {
    fn pull_wongo_shop_relic(&mut self,seat:usize,rarity:usize)->RelicId {
        self.world.pull_player_relic_from_front_matching(&self.roster,seat,rarity,
            |id|RELIC_INFO[id.index()].shops)
    }

    pub(super) fn initialize_wongo_event(&mut self,seat:usize,event:&mut EventState)
        ->Result<(), &'static str> {
        if !self.saved_wongo_points[seat].is_some_and(|points|points>=0) {
            return Err("Wongo saved progress input missing");
        }
        let featured=self.pull_wongo_shop_relic(seat,2);
        event.relics.push(featured);
        let gold=self.roster.player(seat).unwrap().gold;
        event.options.replace([
            EventOption::new("bargain_bin","Bargain Bin","Common relic"),
            EventOption::new("featured","Featured Item","Rare relic"),
            EventOption::new("mystery","Mystery Box","Mystery ticket relic"),
            EventOption::new("leave","Leave","Downgrade a random upgraded card"),
        ]);
        for (option,cost) in event.options.iter_mut().zip([100,200,300]) {
            option.enabled=gold>=cost;
        }
        Ok(())
    }

    pub(super) fn choose_wongo_option(&mut self,seat:usize,mut event:EventState,option:EventOptionIdentity)
        ->Result<(), &'static str> {
        if option.is(EventOptionId::WelcomeToWongosInitialLeave) {
            let candidates:Vec<_>=self.roster.player(seat).unwrap().deck.iter().enumerate()
                .filter_map(|(i,card)|card.upgraded.then_some(i)).collect();
            if !candidates.is_empty() {
                let index=candidates[event.rng.choice_index(candidates.len())];
                let card=&self.roster.player(seat).unwrap().deck[index];
                let card=super::super::manager::RunManager::downgrade_wongo_template(card)?;
                self.roster.player_mut(seat).unwrap().deck[index]=card;
            }
        } else {
            let (cost,points)=match option.id {
                EventOptionId::WelcomeToWongosInitialBargainBin=>(100,32),
                EventOptionId::WelcomeToWongosInitialFeaturedItem=>(200,16),
                EventOptionId::WelcomeToWongosInitialMysteryBox=>(300,8),
                _=>return Err("invalid Wongo option"),
            };
            let saved=self.saved_wongo_points[seat].filter(|points|*points>=0)
                .ok_or("Wongo saved progress input missing")?;
            self.roster.player_mut(seat).unwrap().gold-=cost;
            let id=match option.id {
                EventOptionId::WelcomeToWongosInitialBargainBin=>self.pull_wongo_shop_relic(seat,0),
                EventOptionId::WelcomeToWongosInitialFeaturedItem=>event.relics[0],
                _=>RelicId::WongosMysteryTicket,
            };
            self.attach_event_relic(seat,id)?;
            self.record_relic_seen(seat,id);
            self.roster.player_mut(seat).unwrap().extra_fields.wongo_points=points;
            if saved%2000+points>=2000 {
                self.attach_event_relic(seat,RelicId::WongoCustomerAppreciationBadge)?;
                self.record_relic_seen(seat,RelicId::WongoCustomerAppreciationBadge);
            }
        }
        self.finish_event_seat(seat);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::run::mp::{Character,MpPhase,PlayerSpec};

    fn event()->MpRun {
        let specs=[PlayerSpec{net_id:100,character:Character::Ironclad},
            PlayerSpec{net_id:101,character:Character::Silent}];
        let mut run=MpRun::new("7100000670",&specs,0).unwrap();
        assert_eq!(run.saved_wongo_points,vec![Some(0),Some(0)]);
        run.saved_wongo_points=vec![Some(0),Some(1990)];
        for seat in 0..2 {run.roster.player_mut(seat).unwrap().gold=300;}
        run.phase=MpPhase::Event;
        run.world.current_room=Some("WELCOME_TO_WONGOS");
        run.begin_generic_event(crate::identity::EventId::WelcomeToWongos).unwrap();
        run
    }

    #[test]
    fn wongo_uses_owner_shop_bags_and_explicit_saved_progress() {
        for (option,cost,points) in [(0,100,32),(1,200,16),(2,300,8)] {
            let mut run=event();
            let other=format!("{:?}",run.roster.player(0).unwrap());
            let featured=run.events[1].as_ref().unwrap().relics[0];
            assert!(RELIC_INFO[featured.index()].shops);
            assert_eq!(run.native_wongo_featured(1),Some(featured));
            run.events[1].as_mut().unwrap().options[1].enabled=false;
            assert_eq!(run.native_wongo_featured(1),None,"locked merchandise must stay private");
            run.events[1].as_mut().unwrap().options[1].enabled=true;
            let rng=format!("{:?}",run.roster.player(1).unwrap().rngs);
            run.choose_event_option(101,1,option).unwrap();
            let p=run.roster.player(1).unwrap();
            assert_eq!(p.gold,300-cost);
            assert_eq!(p.extra_fields.wongo_points,points);
            assert_eq!(p.relics.iter().any(|r|r.id==RelicId::WongoCustomerAppreciationBadge),points>=10);
            assert_eq!(format!("{:?}",p.rngs),rng);
            assert_eq!(format!("{:?}",run.roster.player(0).unwrap()),other);
            assert!(run.event_done[1]);
        }
        let mut run=event();
        run.saved_wongo_points[1]=None;
        let before=format!("{run:?}");
        assert_eq!(run.choose_event_option(101,1,0),Err("Wongo saved progress input missing"));
        assert_eq!(format!("{run:?}"),before);
    }

    #[test]
    fn wongo_leave_downgrades_one_owner_card_without_allocating_identity() {
        let mut run=event();
        let mut ctx=run.rest_context(1);
        ctx.upgrade_deck_card(0);
        run.carry_rest_context(1,ctx);
        let old=run.roster.player(1).unwrap().deck[0].clone();
        let next_id=run.roster.next_card_id;
        run.choose_event_option(101,1,3).unwrap();
        let card=&run.roster.player(1).unwrap().deck[0];
        assert_eq!(card.instance_id,old.instance_id);
        assert!(!card.upgraded);
        assert_eq!(run.roster.next_card_id,next_id);
        assert_eq!(run.roster.player(1).unwrap().extra_fields.wongo_points,0);
    }
}
