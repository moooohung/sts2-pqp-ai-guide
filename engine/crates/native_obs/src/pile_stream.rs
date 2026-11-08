use crate::*;

/// Handles above this value are group-local, not main-observation handles.
pub const PILE_LOCAL_BASE: u16 = STATE_CAPACITY as u16;

#[derive(Clone, Copy, Debug)]
pub struct PileGroupSpan {
    pub pile: RowHandle,
    pub row_start: usize,
    pub row_len: usize,
    pub edge_start: usize,
    pub edge_len: usize,
    pub multiplicity: u32,
}

/// Lossless input to bounded-chunk, weighted policy aggregation. No card
/// instances, hidden pile order or simulator references enter this sidecar.
#[derive(Clone, Copy, Debug, Default)]
pub struct StreamedPilesView<'a> {
    pub groups: &'a [PileGroupSpan],
    pub rows: &'a [Row],
    pub edges: &'a [Edge],
}
impl StreamedPilesView<'_> {
    pub fn weighted_rows(&self) -> impl Iterator<Item = (&Row, u32)> {
        self.groups.iter().flat_map(|group|
            self.rows[group.row_start..group.row_start + group.row_len].iter()
                .map(move |row| (row, group.multiplicity)))
    }
}

#[derive(Default)]
pub(crate) struct PileStream {
    pub groups: Vec<PileGroupSpan>,
    pub rows: Vec<Row>,
    pub edges: Vec<Edge>,
}
impl PileStream {
    pub fn clear(&mut self) { self.groups.clear(); self.rows.clear(); self.edges.clear(); }
    pub fn view(&self) -> StreamedPilesView<'_> {
        StreamedPilesView { groups: &self.groups, rows: &self.rows, edges: &self.edges }
    }
    pub fn append(&mut self, view: StreamedPilesView<'_>) {
        for group in view.groups {
            let row_start = self.rows.len(); let edge_start = self.edges.len();
            self.rows.extend_from_slice(&view.rows[group.row_start..group.row_start + group.row_len]);
            self.edges.extend_from_slice(&view.edges[group.edge_start..group.edge_start + group.edge_len]);
            self.groups.push(PileGroupSpan { row_start, edge_start, ..*group });
        }
    }
}
