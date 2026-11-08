use super::*;
use crate::native_batch::NativeBatch;
use burn::tensor::IndexingUpdateOp;


/// A sparse typed message, nonlinear in both endpoints. Reverse messages have
/// distinct types, so directed map links cannot become undirected edges.
#[derive(Module, Debug)]
pub(super) struct ChildAggregation {
    pub(super) types: Embedding,
}
impl ChildAggregation {
    pub(super) fn init(hidden:usize,device:&Device)->Self {
        Self {types:EmbeddingConfig::new(2*Relation::COUNT+1,hidden).init(device)}
    }
    /// `state` includes a zero absent-handle row at index zero.
    pub(super) fn forward(&self,p:&NativeBatch,state:Tensor<3>)->Tensor<3> {
        self.forward_reference(p,state)
    }
    fn forward_reference(&self,p:&NativeBatch,state:Tensor<3>)->Tensor<3> {
        let h=state.dims()[2];
        let index=|side|p.ints.clone().slice([0..p.b,p.edge_i()+side*p.e..p.edge_i()+(side+1)*p.e])
            .reshape([p.b,p.e,1]).repeat_dim(2,h);
        let kinds=p.ints.clone().slice([0..p.b,p.edge_i()+2*p.e..p.edge_i()+3*p.e])
            .reshape([p.b*p.e,1]).repeat_dim(1,h);
        let typed=self.types.weight.val().gather(0,kinds).reshape([p.b,p.e,h]);
        let messages=gelu(state.clone().gather(1,index(0))*(typed+1.)+state.clone().gather(1,index(1)));
        let weights=p.floats.clone().slice([0..p.b,p.edge_f()..p.edge_f()+p.e]).reshape([p.b,p.e,1]);
        Tensor::<3>::zeros(state.dims(),&state.device()).scatter(1,index(1),messages*weights,IndexingUpdateOp::Add)
    }
}

pub(super) struct CpuChildAggregation {
    types:Vec<f32>,
    hidden:usize,
}
impl CpuChildAggregation {
    pub(super) fn copy_from(block:&ChildAggregation)->Self {
        Self {types:block.types.weight.val().into_data().try_to_vec().unwrap(),
            hidden:block.types.weight.val().dims()[1]}
    }
    fn message(&self,state:&[f32],source:usize,target:usize,relation:usize,message:&mut [f32]) {
        let h=self.hidden;
        for (j,x) in message.iter_mut().enumerate() {
            *x=state[source*h+j]*(1.+self.types[relation*h+j])+state[target*h+j];
        }
        super::cpu::activate(message);
    }
    pub(super) fn forward(&self,d:&NativeDecision,state:&[f32],out:&mut Vec<f32>,
        message:&mut Vec<f32>,degree:&mut Vec<u64>) {
        let h=self.hidden;
        out.resize(state.len(),0.);out.fill(0.);
        message.resize(h,0.);
        d.fill_degree(degree);
        for &[from,to,kind] in &d.relations {
            for (source,target,relation) in [(from,to,kind),(to,from,kind+Relation::COUNT)] {
                self.message(state,source,target,relation,message);
                let weight=super::weights::normalized(d.edge_mass(from,to),degree[target]);
                for (x,&y) in out[target*h..][..h].iter_mut().zip(message.iter()) {*x+=y*weight;}
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn typed_messages_aggregate_into_their_owner_and_preserve_reverse_direction() {
        analytic_messages(Device::flex().autodiff());
    }
    fn analytic_messages(device:Device) {
        let mut d=super::super::tests::public_observation(0.);
        d.rows.push(d.rows[1].clone());
        d.relations=vec![[2,1,Relation::Parent.wire_id() as usize],[3,1,Relation::Owner.wire_id() as usize]];
        let p=crate::native_batch::NativeHostBatch::pack(&[&d]).upload(&device);
        let mut block=ChildAggregation::init(1,&device);
        let types=(0..2*Relation::COUNT+1).map(|i|i as f32/10.).collect::<Vec<_>>();
        block.types.weight=block.types.weight.map(|_|Tensor::from_data(
            TensorData::new(types,[2*Relation::COUNT+1,1]),&device).require_grad());
        let mut state=vec![0.;p.r+1];state[1..4].copy_from_slice(&[1.,2.,3.]);
        let input=Tensor::from_data(TensorData::new(state.clone(),[1,p.r+1,1]),&device).require_grad();
        let result=block.forward(&p,input.clone());
        let values=result.clone().into_data().try_to_vec::<f32>().unwrap();
        let phi=|x:f32|0.5*x*(1.+libm::erff(x/std::f32::consts::SQRT_2));
        assert!((values[1]-(phi(3.2)+phi(4.6))*0.5).abs()<2e-6);
        assert!((values[2]-phi(3.8)).abs()<2e-6);
        assert!((values[3]-phi(4.9)).abs()<2e-6);
        assert_eq!(values[0],0.);
        let mut cpu=Vec::new();
        CpuChildAggregation::copy_from(&block).forward(&d,&state,&mut cpu,&mut Vec::new(),&mut Vec::new());
        assert!(values.iter().zip(cpu).all(|(a,b)|(a-b).abs()<2e-6));
        let grads=result.sum().backward();
        let grad=block.types.weight.grad(&grads).unwrap().into_data().try_to_vec::<f32>().unwrap();
        let derivative=|x:f32|0.5*(1.+libm::erff(x/std::f32::consts::SQRT_2))
            +x*(-0.5*x*x).exp()/(2.*std::f32::consts::PI).sqrt();
        for (index,expected) in [(1,derivative(3.2)),(2,1.5*derivative(4.6)),
            (8,derivative(3.8)),(9,derivative(4.9))] {assert!((grad[index]-expected).abs()<3e-6);}
        assert_eq!(grad[0],0.);
        let input_grad=input.grad(&grads).unwrap().into_data().try_to_vec::<f32>().unwrap();
        for (i,expected) in [
            (1,0.5*derivative(3.2)+0.5*derivative(4.6)+1.8*derivative(3.8)+1.9*derivative(4.9)),
            (2,0.55*derivative(3.2)+derivative(3.8)),
            (3,0.6*derivative(4.6)+derivative(4.9)),
        ] {assert!((input_grad[i]-expected).abs()<4e-6);}
    }
    #[test]
    fn ragged_relation_transport_survives_batch_permutation_and_slicing() {
        let device=Device::flex();
        let mut a=super::super::tests::public_observation(0.);
        a.rows.push(a.rows[1].clone());
        a.relations=vec![[2,1,Relation::Parent.wire_id() as usize],[3,1,Relation::Owner.wire_id() as usize]];
        let mut b=a.clone();b.relations=vec![[3,2,Relation::Target.wire_id() as usize]];
        let block=ChildAggregation::init(1,&device);
        let cpu=CpuChildAggregation::copy_from(&block);
        let p=crate::native_batch::NativeHostBatch::pack(&[&a,&b]).upload(&device);
        let order=Tensor::<1,Int>::from_data(TensorData::new(vec![1,0],[2]),&device);
        let shuffled=p.select(order);
        for (lane,d) in [&b,&a].into_iter().enumerate() {
            let slice=shuffled.slice(lane,lane+1);
            let mut state=vec![0.;slice.r+1];state[1..4].copy_from_slice(&[1.,2.,3.]);
            let actual=block.forward(&slice,Tensor::from_data(TensorData::new(state.clone(),[1,slice.r+1,1]),&device))
                .into_data().try_to_vec::<f32>().unwrap();
            let mut expected=Vec::new();
            cpu.forward(d,&state,&mut expected,&mut Vec::new(),&mut Vec::new());
            assert!(actual.iter().zip(expected).all(|(a,b)|(a-b).abs()<3e-6));
        }
    }
}
