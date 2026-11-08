use super::*;
use crate::run::events::trial::{self, Outcome};

impl RunManager {
    pub(super) fn choose_trial(&mut self, mut event: EventState, option: EventOptionIdentity) -> Result<(), &'static str> {
        let outcome = trial::choose(&mut event, option)?;
        self.phase = RunPhase::MapChoice;
        match outcome {
            Outcome::Continue => {
                self.current_event = Some(event);
                self.phase = RunPhase::Event;
            }
            Outcome::Abandon => {
                self.run_ctx_mut().hp = 0;
                self.phase = RunPhase::RunOver;
            }
            Outcome::MerchantGuilty => {
                self.add_card_to_deck(CardId::Regret)?;
                for _ in 0..2 {
                    let relic = self.pull_player_relic(None, false);
                    self.obtain_event_relic(relic)?;
                }
            }
            Outcome::MerchantInnocent | Outcome::NondescriptInnocent => {
                let upgrade = matches!(outcome, Outcome::MerchantInnocent);
                self.add_card_to_deck(if upgrade { CardId::Shame } else { CardId::Doubt })?;
                let candidates = if upgrade { self.upgradable_deck_indices() } else { self.transformable_deck_indices() };
                let count = candidates.len().min(2);
                if count > 0 {
                    self.current_event = Some(event);
                    self.begin_run_card_choice("event:TRIAL",
                        if upgrade { "Choose 2 cards to upgrade" } else { "Choose 2 cards to transform" },
                        &candidates, count, count, false,
                        if upgrade { RunChoiceEffect::Upgrade } else { RunChoiceEffect::TransformEventRng },
                        ChoiceResume::MapChoice);
                    if candidates.len() <= 2 {
                        let choice = self.pending_run_choice.take().unwrap();
                        self.apply_run_choice(choice, &(0..count).collect::<Vec<_>>())?;
                    }
                }
            }
            Outcome::NobleGuilty => self.apply_event_effect(EventEffect::Heal(10))?,
            Outcome::NobleInnocent => {
                self.add_card_to_deck(CardId::Regret)?;
                self.apply_event_effect(EventEffect::GainGold(300))?;
            }
            Outcome::NondescriptGuilty => {
                self.add_card_to_deck(CardId::Doubt)?;
                self.reward = RewardState::default();
                self.reward.stay_in_container = true;
                for _ in 0..2 {
                    let reward = self.make_noncombat_card_reward(3, self.character.into(), None, false, false, false)?;
                    self.reward.items.push(reward);
                }
                self.advance_rewards();
            }
        }
        Ok(())
    }
}
