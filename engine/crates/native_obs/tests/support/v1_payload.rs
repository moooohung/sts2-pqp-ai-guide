use serde_json::{Value, json};
use sts2_native_obs::*;

fn content(id: ContentId) -> Value {
    json!([id.namespace.registry().map_or("Absent", |r| r.name), id.value])
}

// Namespace discriminants belong to the lineage's schema. Registry identity
// aligns them across versions; numeric values, float bits and handles do not change.
pub fn fields(rows: RowsView<'_>, candidates: CandidatesView<'_>, edges: &[Edge],
    status: DecisionStatus, scope: DecisionScope) -> Value {
    let rows: Vec<_> = (0..rows.kind.len()).map(|i| json!([
        format!("{:?}", rows.kind[i]), content(rows.model[i]),
        rows.detail[i].map(content), rows.owner[i].0, rows.parent[i].0,
        rows.flags[i], rows.flags_present[i], rows.integers[i],
        rows.floats[i].map(f32::to_bits), rows.integer_present[i], rows.float_present[i]
    ])).collect();
    let actions: Vec<_> = (0..candidates.action.len()).map(|i| json!([
        format!("{:?}", candidates.action[i]), content(candidates.model[i]),
        candidates.detail[i].map(content), candidates.source[i].0, candidates.target[i].0,
        candidates.actor[i], candidates.flags[i], candidates.flags_present[i],
        candidates.integers[i], candidates.floats[i].map(f32::to_bits),
        candidates.integer_present[i], candidates.float_present[i],
        candidates.candidate_present[i / 64] & (1 << (i % 64)) != 0,
        candidates.legal[i / 64] & (1 << (i % 64)) != 0
    ])).collect();
    let edges: Vec<_> = edges.iter().map(|e| json!([e.from.0, e.to.0, format!("{:?}", e.relation)])).collect();
    json!([format!("{status:?}"), format!("{scope:?}"), rows, actions, edges])
}

pub fn observation(obs: &NativeObsV1, net_id: u64) -> Value {
    let view = obs.view().unwrap();
    let bindings: Vec<_> = (0..view.candidate_len).map(|slot|
        obs.binding(slot, view.decision, net_id).ok().map(|b| format!("{b:?}"))).collect();
    json!([fields(view.rows, view.candidates, view.edges, view.status, view.scope),
        [view.decision.episode, view.decision.generation, u64::from(view.decision.actor)], bindings])
}
