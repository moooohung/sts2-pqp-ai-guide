//! Two contiguous host fields per rollout; upload once, slice/shuffle on device.
use burn::{prelude::*, tensor::TensorData};
use sts2_native_obs::{Flag, NUMERIC_LANES, REGISTRIES, MAX_SEATS};
use crate::native::{NativeDecision, Features};

const RELATION_TYPES:usize=2*sts2_native_obs::Relation::COUNT+1;
fn relation_index_width(r:usize,e:usize)->usize {2*(r+2)+RELATION_TYPES+1+3*e}

pub const SLOTS: usize = sts2_native_obs::ID_LANES + 2;
pub const INT_ROW: usize = SLOTS + 2 + Flag::COUNT;
pub const NUMBERS: usize = NUMERIC_LANES * 4 + Flag::COUNT * 2;

pub fn table_rows() -> usize { 1 + REGISTRIES.iter().map(|r| r.count).sum::<usize>() }
pub fn embedding_width() -> usize { REGISTRIES.iter().map(|r| r.embedding_width).max().unwrap() }

pub struct NativeHostBatch {
    ints: Vec<i32>,
    floats: Vec<f32>,
    pub b: usize,
    pub r: usize,
    pub k: usize,
    pub e: usize,
    team: bool,
    projection:Option<Box<NativeHostBatch>>,
    stream_rows:Vec<NativeHostBatch>,
    stream_edges:Vec<HostStreamBatch>,
}
struct HostStreamBatch {input:NativeHostBatch,main:Vec<i32>}
#[derive(Clone)]
pub(crate) struct StreamBatch {pub(crate) input:NativeBatch,pub(crate) main:Tensor<2,Int>}
impl NativeHostBatch {
    pub fn pack(decisions: &[&NativeDecision]) -> Self {
        assert!(!decisions.is_empty());
        let r = decisions.iter().map(|d| d.rows.len()).max().unwrap().next_power_of_two();
        let k = decisions.iter().map(|d| d.slots.len()).max().unwrap().next_power_of_two();
        Self::pack_with_layout(decisions,r,k)
    }
    pub fn pack_with_layout(decisions: &[&NativeDecision], r: usize, k: usize) -> Self {
        // Any layout at least as large as every decision is exact: padded rows carry a zero
        // context weight and padded candidates are masked, so callers may pick tight sizes.
        assert!(!decisions.is_empty() && r > 0 && k > 0);
        assert!(decisions.iter().all(|d|d.rows.len()<=r && d.slots.len()<=k));
        let projection=decisions.iter().any(|d|d.projection.is_some()).then(|| {
            let old=decisions.iter().map(|d|d.projection.as_deref().unwrap_or(d)).collect::<Vec<_>>();
            let rows=old.iter().map(|d|d.rows.len()).max().unwrap().next_power_of_two();
            Box::new(Self::pack_with_layout(&old,rows,k))
        });
        let mut stream_rows=Vec::new();
        let mut stream_edges=Vec::new();
        let row_chunks=decisions.iter().map(|d|d.streamed.as_ref().map_or(0,|s|s.rows.len())).max().unwrap();
        let edge_chunks=decisions.iter().map(|d|d.streamed.as_ref().map_or(0,|s|s.edges.len())).max().unwrap();
        let missing=if row_chunks>0 || edge_chunks>0 {
            decisions.iter().map(|d|d.empty_stream_chunk()).collect::<Vec<_>>()
        }else{Vec::new()};
        for index in 0..row_chunks {
            let inputs=decisions.iter().enumerate().map(|(lane,d)|
                d.streamed.as_ref().and_then(|s|s.rows.get(index)).unwrap_or(&missing[lane])).collect::<Vec<_>>();
            stream_rows.push(Self::pack(&inputs));
        }
        for index in 0..edge_chunks {
            let inputs=decisions.iter().enumerate().map(|(lane,d)|
                d.streamed.as_ref().and_then(|s|s.edges.get(index)).map(|s|&s.input)
                    .unwrap_or(&missing[lane])).collect::<Vec<_>>();
            let input=Self::pack(&inputs);
            let mut main=vec![0;decisions.len()*(input.r+1)];
            for (lane,d) in decisions.iter().enumerate() {
                if let Some(edge)=d.streamed.as_ref().and_then(|s|s.edges.get(index)) {
                    for (row,&handle) in edge.main.iter().enumerate() {main[lane*(input.r+1)+row+1]=handle as i32;}
                }
            }
            stream_edges.push(HostStreamBatch {input,main});
        }
        let b = decisions.len();
        let n = r + k;
        let team = decisions.iter().any(|d| !d.actors.is_empty() || !d.owners.is_empty());
        let e = decisions.iter().map(|d|d.relations.len()*2).max().unwrap().max(1).next_power_of_two();
        let edge_i = n * INT_ROW + k * if team {4} else {3};
        let edge_f = n * NUMERIC_LANES + r + if team {MAX_SEATS*r} else {0};
        let direct_i = edge_i + 3*e;
        let ratio_f = edge_f + e;
        let csr_i = direct_i + 2 + k;
        let iw = csr_i + relation_index_width(r,e);
        let pool_f = ratio_f + (r+1)*4;
        let fw = pool_f + 3*r;
        let mut ints = vec![0; b * iw];
        let mut floats = vec![0.; b * fw];
        let mut offsets = vec![];
        let mut offset = 0;
        for registry in REGISTRIES { offsets.push(offset); offset += registry.count; }
        for (lane, d) in decisions.iter().enumerate() {
            let totals=d.pool_totals();
            ints[lane*iw+direct_i] = d.direct.global as i32;
            ints[lane*iw+direct_i+1] = d.direct.value_seat as i32;
            for (row,ratios) in d.direct.ratios.iter().enumerate() {
                floats[lane*fw+ratio_f+row*4..][..4].copy_from_slice(ratios);
            }
            for (row, features) in d.rows.iter().enumerate() {
                Self::put(features, &offsets, &mut ints[lane*iw+row*INT_ROW..][..INT_ROW],
                    &mut floats[lane*fw+row*NUMERIC_LANES..][..NUMERIC_LANES]);
                if d.legacy_member(row) {floats[lane*fw+n*NUMERIC_LANES+row] =
                    crate::native::weights::normalized(d.pool_mass(row),totals.legacy);}
                let group=d.groups.get(row).copied().unwrap_or(0) as usize;
                if group>0 {floats[lane*fw+pool_f+(group-1)*r+row] =
                    crate::native::weights::normalized(d.message_mass(row),totals.groups[group]);}
            }
            for (i, features) in d.candidates.iter().enumerate() {
                Self::put(features, &offsets, &mut ints[lane*iw+(r+i)*INT_ROW..][..INT_ROW],
                    &mut floats[lane*fw+(r+i)*NUMERIC_LANES..][..NUMERIC_LANES]);
                ints[lane*iw+n*INT_ROW+i] = d.sources[i] as i32;
                ints[lane*iw+n*INT_ROW+k+i] = d.targets[i] as i32;
                ints[lane*iw+n*INT_ROW+2*k+i] = 1;
                ints[lane*iw+direct_i+2+i] = d.direct.seats.get(i).copied().unwrap_or(0) as i32;
                if team { ints[lane*iw+n*INT_ROW+3*k+i] = d.actors.get(i).copied().unwrap_or(0) as i32; }
            }
            if team {
                for (row, &owner) in d.owners.iter().enumerate() {
                    if owner > 0 && d.legacy_member(row) {
                        floats[lane*fw+n*NUMERIC_LANES+r+(owner as usize-1)*r+row] =
                            crate::native::weights::normalized(d.pool_mass(row),totals.owners[owner as usize]);
                    }
                }
            }
            let mut degree=Vec::new();d.fill_degree(&mut degree);
            for (j,&[from,to,kind]) in d.relations.iter().enumerate() {
                for (i,source,target,relation) in [(2*j,from,to,kind),(2*j+1,to,from,kind+sts2_native_obs::Relation::COUNT)] {
                    ints[lane*iw+edge_i+i] = source as i32;
                    ints[lane*iw+edge_i+e+i] = target as i32;
                    ints[lane*iw+edge_i+2*e+i] = relation as i32;
                    floats[lane*fw+edge_f+i] =
                        crate::native::weights::normalized(d.edge_mass(from,to),degree[target]);
                }
            }
            // Stable adjacency preserves the original message-summation order.
            // Index all three reductions once on the host, not per GPU update.
            let mut pointer=csr_i;
            let mut indices=csr_i+2*(r+2)+RELATION_TYPES+1;
            for (plane,groups) in [(1,r+1),(0,r+1),(2,RELATION_TYPES)] {
                let mut counts=vec![0usize;groups];
                for edge in 0..2*d.relations.len() {
                    counts[ints[lane*iw+edge_i+plane*e+edge] as usize]+=1;
                }
                let mut total=0;
                for (group,count) in counts.iter().enumerate() {
                    ints[lane*iw+pointer+group]=total as i32;
                    total+=count;
                }
                ints[lane*iw+pointer+groups]=total as i32;
                let mut positions=ints[lane*iw+pointer..lane*iw+pointer+groups].to_vec();
                for edge in 0..2*d.relations.len() {
                    let group=ints[lane*iw+edge_i+plane*e+edge] as usize;
                    ints[lane*iw+indices+positions[group] as usize]=edge as i32;
                    positions[group]+=1;
                }
                pointer+=groups+1;
                indices+=e;
            }
        }
        Self { ints, floats, b, r, k, e, team, projection,stream_rows,stream_edges }
    }
    fn put(f: &Features, offsets: &[usize], ints: &mut [i32], floats: &mut [f32]) {
        for (slot, id) in f.ids.iter().enumerate() {
            if id.value > 0 { ints[slot] = (offsets[id.namespace as usize - 1] + id.value as usize) as i32; }
        }
        floats.copy_from_slice(&f.numbers);
        ints[SLOTS] = f.ip as i32;
        ints[SLOTS+1] = f.fp as i32;
        for (i,flag) in Flag::ALL.iter().enumerate() {
            let present = f.present & flag.bit() != 0;
            let value = f.flags & flag.bit() != 0;
            ints[SLOTS+2+i] = if !present { 0 } else if !value { 1 } else { 2 };
        }
    }
    pub fn byte_size(&self)->u64 {
        4*(self.ints.len()+self.floats.len()+NUMERIC_LANES) as u64
            +self.projection.as_ref().map_or(0,|p|p.byte_size())
            +self.stream_rows.iter().map(Self::byte_size).sum::<u64>()
            +self.stream_edges.iter().map(|edge|edge.input.byte_size()+4*edge.main.len() as u64).sum::<u64>()
    }
    pub fn upload(self, device: &Device) -> NativeBatch {
        let iw = (self.r+self.k)*INT_ROW+self.k*if self.team {4} else {3}+3*self.e+2+self.k
            +relation_index_width(self.r,self.e);
        let fw = (self.r+self.k)*NUMERIC_LANES+self.r+if self.team {MAX_SEATS*self.r} else {0}+self.e+(self.r+1)*4+3*self.r;
        NativeBatch {
            ints: Tensor::from_data(TensorData::new(self.ints, [self.b, iw]), device),
            floats: Tensor::from_data(TensorData::new(self.floats, [self.b, fw]), device),
            bits: Tensor::from_data(TensorData::new((0..NUMERIC_LANES).map(|i| 1i32<<i).collect::<Vec<_>>(),
                [1, NUMERIC_LANES]), device),
            b:self.b, r:self.r, k:self.k, e:self.e, team:self.team,
            projection:self.projection.map(|old|Box::new(old.upload(device))),
            stream_rows:self.stream_rows.into_iter().map(|input|input.upload(device)).collect(),
            stream_edges:self.stream_edges.into_iter().map(|edge| {
                let main=Tensor::from_data(TensorData::new(edge.main,[self.b,edge.input.r+1]),device);
                StreamBatch {input:edge.input.upload(device),main}
            }).collect(),
        }
    }
}
#[derive(Clone)]
pub struct NativeBatch {
    pub(crate) ints: Tensor<2, Int>,
    pub(crate) floats: Tensor<2>,
    pub(crate) bits: Tensor<2, Int>,
    pub b: usize,
    pub r: usize,
    pub k: usize,
    pub e: usize,
    pub(crate) team: bool,
    pub(crate) projection:Option<Box<NativeBatch>>,
    pub(crate) stream_rows:Vec<NativeBatch>,
    pub(crate) stream_edges:Vec<StreamBatch>,
}
impl NativeBatch {
    pub(crate) fn edge_i(&self)->usize {(self.r+self.k)*INT_ROW+self.k*if self.team {4}else{3}}
    pub(crate) fn edge_f(&self)->usize {(self.r+self.k)*NUMERIC_LANES+self.r+if self.team {MAX_SEATS*self.r}else{0}}
    pub(crate) fn direct_i(&self)->usize {self.edge_i()+3*self.e}
    pub(crate) fn relation_i(&self)->usize {self.direct_i()+2+self.k}
    pub(crate) fn ratio_f(&self)->usize {self.edge_f()+self.e}
    pub(crate) fn pool_f(&self)->usize {self.ratio_f()+(self.r+1)*4}
    /// Borrow packed fields for transport validation without re-encoding them.
    pub fn packed_fields(&self)->(&Tensor<2,Int>,&Tensor<2>,&Tensor<2,Int>) {
        (&self.ints,&self.floats,&self.bits)
    }
    /// Bytes allocated by select: bit tensors are shared, every recursive field is copied.
    pub fn selection_byte_size(&self,b:usize)->u64 {
        4*b as u64*(self.ints.dims()[1]+self.floats.dims()[1]) as u64
            +self.projection.as_ref().map_or(0,|p|p.selection_byte_size(b))
            +self.stream_rows.iter().map(|input|input.selection_byte_size(b)).sum::<u64>()
            +self.stream_edges.iter().map(|edge|edge.input.selection_byte_size(b)
                +4*b as u64*edge.main.dims()[1] as u64).sum::<u64>()
    }
    pub fn slice(&self, start: usize, end: usize) -> Self {
        assert!(start < end && end <= self.b);
        Self { ints:self.ints.clone().slice([start..end]),
            floats:self.floats.clone().slice([start..end]), bits:self.bits.clone(),
            b:end-start, r:self.r, k:self.k, e:self.e, team:self.team,
            projection:self.projection.as_ref().map(|old|Box::new(old.slice(start,end))),
            stream_rows:self.stream_rows.iter().map(|input|input.slice(start,end)).collect(),
            stream_edges:self.stream_edges.iter().map(|edge|StreamBatch {input:edge.input.slice(start,end),
                main:edge.main.clone().slice([start..end])}).collect() }
    }
    /// Upload the epoch permutation once, then slice this shuffled rollout.
    pub fn select(&self, permutation: Tensor<1, Int>) -> Self {
        let b = permutation.dims()[0];
        Self { ints:self.ints.clone().select(0, permutation.clone()),
            floats:self.floats.clone().select(0, permutation.clone()), bits:self.bits.clone(), b, r:self.r, k:self.k, e:self.e, team:self.team,
            projection:self.projection.as_ref().map(|old|Box::new(old.select(permutation.clone()))),
            stream_rows:self.stream_rows.iter().map(|input|input.select(permutation.clone())).collect(),
            stream_edges:self.stream_edges.iter().map(|edge|StreamBatch {input:edge.input.select(permutation.clone()),
                main:edge.main.clone().select(0,permutation.clone())}).collect() }
    }
}
