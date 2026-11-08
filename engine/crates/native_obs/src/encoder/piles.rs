use super::*;
use crate::pile_stream::PileStream;

pub(super) trait RowWriter {
    fn push_row(&mut self, row: Row) -> Result<RowHandle>;
    fn push_edge(&mut self, edge: Edge) -> Result<()>;
}
impl RowWriter for NativeObsV2 {
    fn push_row(&mut self, row: Row) -> Result<RowHandle> { Ok(NativeObsV2::push_row(self, row)?) }
    fn push_edge(&mut self, edge: Edge) -> Result<()> { Ok(NativeObsV2::push_edge(self, edge)?) }
}

struct GroupWriter<'a> {
    stream: &'a mut PileStream,
    row_start: usize,
    multiplicity: u32,
}
impl RowWriter for GroupWriter<'_> {
    fn push_row(&mut self, mut row: Row) -> Result<RowHandle> {
        row.numbers.validate(ROW_FIELDS[row.kind as usize])?;
        row.flags.validate()?;
        if !row.model.is_valid() || row.detail.iter().any(|id| !id.is_valid()) {
            return Err(ContractError::InvalidContentId.into());
        }
        row.multiplicity = self.multiplicity;
        if matches!(row.kind, RowKind::CardPreview | RowKind::EffectParameter) {
            row.legacy_role = LegacyRowRole::V2Only;
        }
        self.stream.rows.push(row);
        let local = self.stream.rows.len() - self.row_start;
        Ok(RowHandle(u16::try_from(usize::from(PILE_LOCAL_BASE) + local)
            .map_err(|_| ContractError::InvalidHandle)?))
    }
    fn push_edge(&mut self, edge: Edge) -> Result<()> { self.stream.edges.push(edge); Ok(()) }
}

impl NativeEncoder {
    pub(super) fn deferred_card(&self, stream: &mut PileStream, card: &Card, row: Row,
        preview: &[sts2_sim::card_preview::CardPreviewVar], x: (Option<i32>, Option<i32>),
        summary: RowHandle, multiplicity: u32) -> Result<()> {
        let row_start = stream.rows.len(); let edge_start = stream.edges.len();
        let mut writer = GroupWriter { stream, row_start, multiplicity };
        let root = self.card_from_row(&mut writer, card, row)?;
        self.display_variables(&mut writer, card.id, root, row.owner, None, preview, x)?;
        writer.push_edge(Edge { from: summary, to: root, relation: Relation::Member })?;
        let row_len = writer.stream.rows.len() - row_start;
        let edge_len = writer.stream.edges.len() - edge_start;
        writer.stream.groups.push(PileGroupSpan { pile: summary, row_start, row_len,
            edge_start, edge_len, multiplicity });
        Ok(())
    }

    pub(super) fn finish(&mut self, obs: &mut NativeObsV1, status: DecisionStatus) -> Result<()> {
        if !obs.checking_binding() {
            for group in &self.deferred_piles.groups {
                let (rows, _, edges, _) = obs.lengths();
                if !self.team && rows + group.row_len <= STATE_CAPACITY && edges + group.edge_len <= EDGE_CAPACITY {
                    let route = |handle: RowHandle| if handle.0 > PILE_LOCAL_BASE {
                        RowHandle((rows + usize::from(handle.0 - PILE_LOCAL_BASE)) as u16)
                    } else { handle };
                    for source in &self.deferred_piles.rows[group.row_start..group.row_start + group.row_len] {
                        let mut row = *source; row.owner = route(row.owner); row.parent = route(row.parent);
                        obs.push_row(row)?;
                    }
                    for edge in &self.deferred_piles.edges[group.edge_start..group.edge_start + group.edge_len] {
                        obs.push_edge(Edge { from: route(edge.from), to: route(edge.to), relation: edge.relation })?;
                    }
                } else {
                    obs.streamed_piles.append(StreamedPilesView {
                        groups: core::slice::from_ref(group),
                        rows: &self.deferred_piles.rows,
                        edges: &self.deferred_piles.edges,
                    });
                }
            }
        }
        obs.finish(status)?;
        Ok(())
    }

    pub(super) fn publish_legacy(obs: &mut NativeObsV1, old: Box<NativeObsV1>, result: Result<()>) -> Result<()> {
        match result {
            Ok(()) => obs.attach_legacy(old)?,
            Err(EncodeError::Contract(ContractError::Capacity { buffer: BufferKind::Rows | BufferKind::Edges, .. })) => {
                // There was no V1 observation for this state. Retain its scratch
                // allocation, not a fabricated or truncated old-path snapshot.
                obs.legacy = Some(old);
            }
            Err(error) => return Err(error),
        }
        Ok(())
    }
}
