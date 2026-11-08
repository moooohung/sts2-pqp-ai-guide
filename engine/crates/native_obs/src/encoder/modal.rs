use super::*;
use sts2_sim::run::{EventPageIdentity, EventParameter, NativeRewardContent as R,
    event_preview::{PublicEventParameter, PublicEventTiming}};

pub(super) fn is_modal_page(page: EventPageIdentity) -> bool {
    matches!(page.parameter, EventParameter::AbandonRunConfirmation(_) | EventParameter::SphereReward { .. })
}

impl NativeEncoder {
    pub(super) fn abandon_modal_option(&self, obs: &mut NativeObsV1, parent: RowHandle,
        index: usize, confirm: bool, enabled: bool) -> Result<RowHandle> {
        let owner = self.seat_rows[usize::from(self.decision.actor)];
        let mut row = Row { kind: RowKind::ModalOption,
            model: id(Namespace::PublicModalAction, usize::from(!confirm)), owner, parent, ..Row::default() };
        ints(&mut row.numbers, &[(0, as_i32(index)?)])?;
        row.flags.set(Flag::Locked, !enabled);
        let handle = Self::row(obs, row)?;
        self.event_parameters(obs, handle, owner, &[PublicEventParameter::integer(
            if confirm { "AbandonRun" } else { "Continue" }, 1).at(PublicEventTiming::Confirmation)])?;
        Ok(handle)
    }

    pub(super) fn sphere_rewards(&self, run: &MpRun, obs: &mut NativeObsV1, parent: RowHandle) -> Result<()> {
        let seat = usize::from(self.decision.actor);
        let owner = self.seat_rows[seat];
        let player = &run.roster.players()[seat];
        let options = run.native_event_options(self.net_id, seat).map_err(EncodeError::InvalidState)?
            .ok_or(EncodeError::InvalidState("sphere reward options missing"))?;
        let mut rewards = std::collections::BTreeMap::new();
        let mut enabled_index = 0;
        for (index, option) in options.iter().enumerate() {
            let EventParameter::SphereReward { offer, action } = option.option_id.parameter
                else { return Err(EncodeError::InvalidState("mixed sphere reward options")); };
            let content = run.native_sphere_reward(seat, usize::from(offer))
                .ok_or(EncodeError::InvalidState("visible sphere reward missing"))?;
            if !rewards.contains_key(&offer) {
                let (kind, model) = match content {
                    R::Potion(p) => (RewardKind::Potion, id(Namespace::PotionId, p.index())),
                    R::Relic(r) => (RewardKind::Relic, id(Namespace::RelicId, r.index())),
                    R::Cards { .. } => (RewardKind::Card, ContentId::ABSENT),
                    _ => return Err(EncodeError::InvalidState("unsupported sphere reward")),
                };
                let mut row = Row { kind: RowKind::Reward, model, owner, parent,
                    detail: [kind.into(), ContentId::ABSENT, ContentId::ABSENT, ContentId::ABSENT], ..Row::default() };
                row.flags.set(Flag::Skippable, true);
                ints(&mut row.numbers, &[(1, i32::from(offer))])?;
                let source = Self::row(obs, row)?;
                let mut targets = Vec::new();
                match content {
                    R::Potion(p) => targets.push(self.potion(obs, Some(p), owner, source, 0, player.max_potion_slots)?),
                    R::Relic(r) => targets.push(self.offered_relic(obs, r, source, owner)?),
                    R::Cards { cards, .. } => for (order, card) in cards.iter().enumerate() {
                        targets.push(self.card(obs, card, owner, source, PileKind::None, Some(order), None)?);
                    },
                    _ => unreachable!(),
                }
                rewards.insert(offer, (source, targets));
            }
            let (source, targets) = &rewards[&offer];
            let (choice, target, model, instance, legal, modal_action) = match (action, content) {
                (0, _) => (SphereRewardChoice::Skip, RowHandle::ABSENT, ContentId::ABSENT, None, true, 3),
                (1, R::Potion(p)) => (SphereRewardChoice::Potion, targets[0], id(Namespace::PotionId, p.index()), None,
                    player.potions.iter().any(Option::is_none) || player.relics.iter().any(|r| r.enabled && r.id == RelicId::Sozu), 2),
                (1, R::Relic(r)) => (SphereRewardChoice::Relic, targets[0], id(Namespace::RelicId, r.index()), None, true, 2),
                (5, R::Cards { reroll: true, .. }) => (SphereRewardChoice::Reroll, RowHandle::ABSENT, ContentId::ABSENT, None, true, 4),
                (6, R::Cards { .. }) => (SphereRewardChoice::Sacrifice, RowHandle::ABSENT, ContentId::ABSENT, None,
                    player.relics.iter().any(|r| r.enabled && r.id == RelicId::PaelsWing), 5),
                (a @ 1..=4, R::Cards { cards, .. }) => {
                    let card = cards.get(usize::from(a - 1)).ok_or(EncodeError::InvalidState("sphere card index missing"))?;
                    (SphereRewardChoice::Card, targets[usize::from(a - 1)], id(Namespace::CardId, card.id.index()), Some(card.instance_id), true, 2)
                }
                _ => return Err(EncodeError::InvalidState("invalid visible sphere action")),
            };
            self.action(obs, Candidate { source: *source, target, model,
                detail: [id(Namespace::PublicModalAction, modal_action), ContentId::ABSENT, ContentId::ABSENT, ContentId::ABSENT],
                legal: legal && option.enabled, ..Candidate::default() },
                Command::MpSphereReward { option: enabled_index, choice }, instance, None, Some(index as u64))?;
            enabled_index += u32::from(option.enabled);
        }
        Ok(())
    }
}
