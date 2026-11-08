use super::*;

#[derive(Clone,Copy,Debug,Default,serde::Serialize,serde::Deserialize)]
pub(crate) struct PoolTotals {
    pub(crate) legacy:u64,
    pub(crate) owners:[u64;MAX_SEATS+1],
    pub(crate) groups:[u64;4],
}
impl PoolTotals {
    pub(super) fn add(&mut self,mass:u64,legacy:bool,owner:usize,group:usize) {
        if legacy {self.legacy+=mass;self.owners[owner]+=mass;}
        self.groups[group]+=mass;
    }
}
impl NativeDecision {
    #[cfg(feature="obs-v2")]
    pub(super) fn exclude_choice_pile_details(&mut self,v:&ObservationView<'_>) {
        if !v.rows.kind.contains(&RowKind::Choice) {return;}
        let piles=(0..v.state_len).filter(|&row|v.rows.kind[row]==RowKind::PileSummary)
            .map(|row|(v.rows.owner[row],v.rows.detail[row][0])).collect::<Vec<_>>();
        let mut members=vec![false;v.state_len];
        for edge in v.edges {
            if edge.relation==Relation::Member && edge.from.index().is_some_and(|i|v.rows.kind[i]==RowKind::PileSummary) {
                if let Some(row)=edge.to.index() {members[row]=true;}
            }
        }
        let mut excluded=vec![false;v.state_len];
        for row in 0..v.state_len {
            let parent=v.rows.parent[row].index();
            excluded[row]=parent.is_some_and(|i|excluded[i]);
            if v.rows.kind[row]!=RowKind::Card || members[row] {continue;}
            let pile=v.rows.detail[row][3];
            let unordered=pile.namespace==Namespace::PileKind && pile.value.checked_sub(1)
                .and_then(|i|Namespace::PileKind.registry().unwrap().names.get(i as usize))
                .is_some_and(|&name|matches!(name,"Draw"|"Discard"|"Exhaust"|"Play"));
            if unordered && parent.is_some_and(|i|v.rows.kind[i]==RowKind::Choice)
                && piles.contains(&(v.rows.owner[row],pile)) {
                excluded[row]=true;
            }
        }
        if excluded.contains(&true) {
            // These are candidate-reference entities, not additional copies in
            // the public pile multiset. Keep their features and typed edges.
            self.pool_multiplicity=(0..v.state_len).map(|row|
                if excluded[row] {0}else{v.rows.multiplicity[row]}).collect();
        }
    }
    pub(crate) fn row_multiplicity(&self,row:usize)->u64 {
        self.multiplicity.get(row).copied().unwrap_or(1) as u64
    }
    pub(crate) fn pool_mass(&self,row:usize)->u64 {
        self.pool_multiplicity.get(row).copied().map(u64::from).unwrap_or_else(||self.row_multiplicity(row))
    }
    pub(crate) fn message_mass(&self,row:usize)->u64 {
        self.message_multiplicity.get(row).copied().map(u64::from).unwrap_or_else(||self.pool_mass(row))
    }
    pub(crate) fn edge_mass(&self,from:usize,to:usize)->u64 {
        // A grouped child belongs to its matching parent copy, not every
        // parent copy. External Seat/Pile rows have unit multiplicity.
        self.row_multiplicity(from-1).max(self.row_multiplicity(to-1))
    }
    pub(crate) fn fill_degree(&self,out:&mut Vec<u64>) {
        if !self.degree.is_empty() {out.clone_from(&self.degree);return;}
        out.resize(self.rows.len()+1,0);out.fill(0);
        for &[from,to,_] in &self.relations {
            let mass=self.edge_mass(from,to);out[from]+=mass;out[to]+=mass;
        }
    }
    pub(crate) fn pool_totals(&self)->PoolTotals {
        if let Some(totals)=self.totals {return totals;}
        let mut totals=PoolTotals::default();
        for row in 0..self.rows.len() {
            totals.add(self.pool_mass(row),self.legacy_member(row),
                self.owners.get(row).copied().unwrap_or(0) as usize,
                self.groups.get(row).copied().unwrap_or(0) as usize);
        }
        totals
    }
}

pub(crate) fn normalized(mass:u64,total:u64)->f32 {
    if mass==0 {0.}else{(mass as f64/total as f64) as f32}
}
