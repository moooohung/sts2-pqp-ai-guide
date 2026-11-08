use super::*;

impl NativePolicy {
    pub fn into_relational_warm_start(mut self,device:&Device)->Result<Self,String> {
        if self.is_v2() {return Err("policy is already relational; refusing to discard learned residuals".into());}
        let width=crate::native_batch::embedding_width();
        let source=self.embedding.weight.val().into_data().try_to_vec::<f32>().map_err(|e|e.to_string())?;
        let mut target=vec![0.;crate::native_batch::table_rows()*width];
        target[..width].copy_from_slice(&source[..width]);
        for (row,&old) in manifest::v1_lookup().iter().enumerate().skip(1) {
            if old!=0 {target[row*width..][..width].copy_from_slice(&source[old as usize*width..][..width]);}
        }
        self.embedding.weight=self.embedding.weight.map(|_|Tensor::from_data(
            TensorData::new(target,[crate::native_batch::table_rows(),width]),device).set_require_grad(device.is_autodiff()));
        let mut next=super::v2::RelationalPolicy::init(self.hidden,device);
        next.actor=LinearConfig::new(self.hidden,1).with_initializer(burn::nn::Initializer::Zeros).init(device);
        next.critic=LinearConfig::new(self.hidden,1).with_initializer(burn::nn::Initializer::Zeros).init(device);
        self.v2=Some(next);
        Ok(self)
    }
    /// Preserve the actor; the new owner block starts inert and the team critic is refit separately.
    pub fn into_joint_warm_start(self, device:&Device)->Self {
        let joint=self.is_joint();
        let mut next=Self::init_joint(self.hidden,device);
        next.embedding=self.embedding;
        next.encoder=self.encoder;
        next.actor=self.actor;
        if joint {
            next.actor_hidden=self.actor_hidden;
        }else{
            let hidden=self.hidden;
            next.actor_hidden.weight=self.actor_hidden.weight.map(|weight|
                Tensor::cat(vec![weight,Tensor::<2>::zeros([hidden,hidden],device)],0));
            next.actor_hidden.bias=self.actor_hidden.bias;
        }
        next.critic=self.critic;
        next.v2=self.v2;
        next
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn warm_start_preserves_the_old_actor_and_adds_no_owner_preference() {
        let _rng_guard=crate::RNG_TEST_LOCK.lock().unwrap();
        let device=Device::flex();
        let old=NativePolicy::init(8,&device);
        let embedding=old.embedding.weight.val().into_data();
        let input=crate::native::tests::public_observation(0.25);
        let mut before=CpuWorkspace::default();
        CpuPolicy::copy_from(&old).forward(&input,&mut before);
        let next=old.into_joint_warm_start(&device);
        assert!(next.is_joint());
        assert_eq!(embedding,next.embedding.weight.val().into_data());
        let hidden=next.hidden;
        let actor_hidden=next.actor_hidden.weight.val().into_data().try_to_vec::<f32>().unwrap();
        assert!(actor_hidden[actor_hidden.len()-hidden*hidden..].iter().all(|&value|value==0.));
        let mut after=CpuWorkspace::default();
        CpuPolicy::copy_from(&next).forward(&input,&mut after);
        for(a,b)in before.logp.iter().zip(&after.logp){assert_eq!(a.to_bits(),b.to_bits());}

        let joint=NativePolicy::init_joint(8,&device);
        let joint_actor=joint.actor_hidden.weight.val().into_data();
        let joint_critic=joint.critic.weight.val().into_data();
        let migrated=joint.into_joint_warm_start(&device);
        assert_eq!(joint_actor,migrated.actor_hidden.weight.val().into_data());
        assert_eq!(joint_critic,migrated.critic.weight.val().into_data());
    }
    #[test]
    fn relational_transfer_preserves_raw_logits_values_and_learned_baseline() {
        let _guard=crate::RNG_TEST_LOCK.lock().unwrap();
        let device=Device::flex().autodiff();
        let obs=super::super::v2_tests::observation();
        let input=NativeDecision::from_view(obs.view().unwrap()).unwrap();
        let packed=crate::native_batch::NativeHostBatch::pack(&[&input]).upload(&device);
        for joint in [false,true] {
            let old=if joint {NativePolicy::init_joint(8,&device)}else{NativePolicy::init(8,&device)};
            let before=old.logits_packed(&packed);
            let next=old.into_relational_warm_start(&device).unwrap();
            assert!(next.is_v2());
            let after=next.logits_packed(&packed);
            assert_eq!(before.0.into_data(),after.0.clone().into_data());
            assert_eq!(before.1.into_data(),after.1.into_data());
            let grads=after.0.sum().backward();
            let grad=next.v2.as_ref().unwrap().actor.weight.grad(&grads).unwrap().into_data().try_to_vec::<f32>().unwrap();
            assert!(grad.iter().any(|x|x.abs()>1e-7));
            assert!(next.into_relational_warm_start(&device).is_err());
        }
    }
}
