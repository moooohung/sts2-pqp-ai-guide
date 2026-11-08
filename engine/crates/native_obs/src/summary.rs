use crate::*;

/// Independent policy pools. Seat and Global are direct context, not members.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum SummaryGroup { Combat, Deck, Map }

impl ObservationView<'_> {
    pub fn summary_group(&self, row_index: usize) -> Option<SummaryGroup> {
        use SummaryGroup::*;
        let mut index = row_index;
        loop {
            match *self.rows.kind.get(index)? {
                RowKind::Absent | RowKind::Global | RowKind::Seat => return None,
                RowKind::MapNode | RowKind::MapEdge | RowKind::RouteSummary | RowKind::Boss => return Some(Map),
                RowKind::Creature | RowKind::Intent | RowKind::Power | RowKind::Orb | RowKind::Rune => return Some(Combat),
                RowKind::Card => return Some(if self.rows.detail[index][3]
                    == ContentId::from_index(Namespace::PileKind, sts2_sim::combat::PileKind::None as usize).unwrap() { Deck } else { Combat }),
                RowKind::PileSummary => return Some(if self.rows.detail[index][0]
                    == ContentId::from_index(Namespace::PileKind, sts2_sim::combat::PileKind::None as usize).unwrap() { Deck } else { Combat }),
                RowKind::Modifier | RowKind::CardPreview | RowKind::EffectParameter | RowKind::ChoiceOption => {
                    if let Some(parent) = self.rows.parent[index].index() { index = parent; }
                    else { return Some(Deck); }
                }
                RowKind::Choice => {
                    let phase = self.rows.detail[0][0];
                    let combat = phase == ContentId::from_index(Namespace::RunPhase, sts2_sim::run::RunPhase::Combat as usize).unwrap()
                        || phase == ContentId::from_index(Namespace::MpPhase, sts2_sim::run::mp::MpPhase::Combat as usize).unwrap();
                    return Some(if combat { Combat } else { Deck });
                }
                RowKind::UpgradePreview | RowKind::Relic | RowKind::Potion | RowKind::Reward |
                RowKind::Offer | RowKind::Event | RowKind::EventPage | RowKind::EventOption |
                RowKind::RestService | RowKind::Treasure | RowKind::Modal | RowKind::ModalOption => return Some(Deck),
            }
        }
    }
}
