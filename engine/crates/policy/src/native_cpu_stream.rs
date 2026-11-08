use super::*;
use crate::native::weights::{PoolTotals,normalized};

fn add(target:&mut [f32],source:&[f32],weight:f32) {
    for (x,&y) in target.iter_mut().zip(source) {*x+=y*weight;}
}
pub(super) fn legacy(d:&NativeDecision,row:usize,encoded:&[f32],totals:&PoolTotals,
    context:&mut [f32],owners:&mut [f32]) {
    if !d.legacy_member(row) {return;}
    let mass=d.pool_mass(row);
    if mass==0 {return;}
    add(context,encoded,normalized(mass,totals.legacy));
    let owner=d.owners.get(row).copied().unwrap_or(0) as usize;
    if owner>0 {
        let h=encoded.len();
        add(&mut owners[owner*h..][..h],encoded,normalized(mass,totals.owners[owner]));
    }
}
impl CpuPolicy {
    pub(super) fn stream_forward(&self,d:&NativeDecision,w:&mut CpuWorkspace,v2:Option<&CpuRelationalPolicy>) {
        let Some(stream)=&d.streamed else {return;};
        let h=self.h;
        w.stream_row.resize(h,0.);
        for input in &stream.rows {
            let totals=input.pool_totals();
            for (row,feature) in input.rows.iter().enumerate() {
                self.encode_cached(feature,&mut w.input,&mut w.stream_row,&mut w.encoded,&mut w.encodings);
                legacy(input,row,&w.stream_row,&totals,&mut w.context,&mut w.owners);
                let group=input.groups[row] as usize;
                if v2.is_some() && group>0 {
                    add(&mut w.v2_pools[(group-1)*h..][..h],&w.stream_row,
                        normalized(input.message_mass(row),totals.groups[group]));
                }
            }
        }
        let Some(v2)=v2 else {return;};
        for edge in &stream.edges {
            let input=&edge.input;
            let totals=input.pool_totals();
            w.stream_state.resize((input.rows.len()+1)*h,0.);w.stream_state[..h].fill(0.);
            for (row,feature) in input.rows.iter().enumerate() {
                self.encode_cached(feature,&mut w.input,
                    &mut w.stream_state[(row+1)*h..][..h],&mut w.encoded,&mut w.encodings);
            }
            v2.children.forward(input,&w.stream_state,&mut w.stream_messages,
                &mut w.stream_scratch,&mut w.stream_degree);
            for (row,&main) in edge.main.iter().enumerate() {
                let values=&w.stream_messages[(row+1)*h..][..h];
                if main>0 {add(&mut w.v2_state[main*h..][..h],values,1.);}
                else {
                    let group=input.groups[row] as usize;
                    if group>0 {add(&mut w.v2_pools[(group-1)*h..][..h],values,
                        normalized(input.message_mass(row),totals.groups[group]));}
                }
            }
        }
    }
}
