use super::*;

#[cfg(target_arch="wasm32")]
mod heap {
    use std::alloc::{GlobalAlloc,Layout,System};
    use std::sync::atomic::{AtomicUsize,Ordering};
    static LIVE:AtomicUsize=AtomicUsize::new(0);
    static PEAK:AtomicUsize=AtomicUsize::new(0);
    struct Counted;
    fn add(bytes:usize) {
        let live=LIVE.fetch_add(bytes,Ordering::Relaxed)+bytes;
        PEAK.fetch_max(live,Ordering::Relaxed);
    }
    unsafe impl GlobalAlloc for Counted {
        unsafe fn alloc(&self,layout:Layout)->*mut u8 {
            let ptr=unsafe {System.alloc(layout)};
            if !ptr.is_null() {add(layout.size());}
            ptr
        }
        unsafe fn alloc_zeroed(&self,layout:Layout)->*mut u8 {
            let ptr=unsafe {System.alloc_zeroed(layout)};
            if !ptr.is_null() {add(layout.size());}
            ptr
        }
        unsafe fn dealloc(&self,ptr:*mut u8,layout:Layout) {
            unsafe {System.dealloc(ptr,layout);}
            LIVE.fetch_sub(layout.size(),Ordering::Relaxed);
        }
        unsafe fn realloc(&self,ptr:*mut u8,layout:Layout,size:usize)->*mut u8 {
            let next=unsafe {System.realloc(ptr,layout,size)};
            if !next.is_null() {
                if size>=layout.size() {add(size-layout.size());}
                else {LIVE.fetch_sub(layout.size()-size,Ordering::Relaxed);}
            }
            next
        }
    }
    #[global_allocator]
    static ALLOCATOR:Counted=Counted;
    #[wasm_bindgen::prelude::wasm_bindgen(js_name=resetMemoryPeak)]
    pub fn reset() {PEAK.store(LIVE.load(Ordering::Relaxed),Ordering::Relaxed);}
    #[wasm_bindgen::prelude::wasm_bindgen(js_name=allocationStats)]
    pub fn stats()->String {
        serde_json::json!({"live_bytes":LIVE.load(Ordering::Relaxed),
            "peak_bytes":PEAK.load(Ordering::Relaxed)}).to_string()
    }
}

pub fn pile(cards:usize,unique:bool)->Result<NativeDecision,String> {
    use sts2_sim::{combat::Combat,creature::Side,rng::Rng,tables::{CardId,EnchantmentId,RoomType}};
    let mut ctx=RunManager::new(7,RunOptions::default()).run_ctx().clone();
    ctx.deck.clear();ctx.relics.clear();ctx.potions.clear();ctx.max_potion_slots=0;ctx.room_type=RoomType::Monster;
    let mut combat=Combat::new(ctx,7);
    combat.setup_encounter("TUNNELER_WEAK",&mut Rng::new(1));
    combat.players[0].energy=3;combat.side=Side::Player;
    for _ in 0..10 {
        let key=combat.new_card(0,CardId::StrikeIronclad,false).ok_or("hand card")?;
        combat.players[0].hand.push(key);
    }
    for index in 0..cards {
        let key=combat.new_card(0,CardId::StrikeNecrobinder,false).ok_or("pile card")?;
        let amount=if unique {i32::try_from(index+1).map_err(|e|e.to_string())?}else{1};
        if !combat.cards[usize::from(key)].apply_static_enchantment(EnchantmentId::Sharp,amount) {
            return Err("stress card enchantment".into());
        }
        combat.players[0].draw.push(key);
    }
    let mut encoder=NativeEncoder::new();let mut obs=NativeObsV1::new();
    encoder.encode_combat_snapshot(&combat,&[Character::Necrobinder],
        DecisionId{episode:7,generation:0,actor:0},1,&mut obs).map_err(|e|format!("{e:?}"))?;
    NativeDecision::from_view(obs.view().map_err(|e|format!("{e:?}"))?)
}

#[cfg_attr(target_arch="wasm32",wasm_bindgen::prelude::wasm_bindgen)]
impl NativeAiPlay {
    #[cfg_attr(target_arch="wasm32",wasm_bindgen(js_name=inferenceMemory))]
    pub fn inference_memory(&self)->String {
        json!({"input":self.episode.as_ref().and_then(|e|e.current.as_ref())
            .map(|(d,_)|d.allocation_stats()),"workspace":self.workspace.allocation_stats(self.meta.hidden as usize)}).to_string()
    }
    #[cfg_attr(target_arch="wasm32",wasm_bindgen(js_name=memoryPile))]
    pub fn memory_pile(&mut self,cards:u32,unique:bool)->Result<String,String> {
        if !self.relational {return Err("streamed pile stress requires relational heads".into());}
        let input=pile(cards as usize,unique)?;
        let value=self.policy.forward(&input,&mut self.workspace);
        Ok(json!({"input":input.allocation_stats(),"workspace":self.workspace.allocation_stats(self.meta.hidden as usize),
            "observation_sha256":sha256(input.trajectory_json(true).map_err(|e|e.to_string())?.as_bytes()),
            "slots":input.slots,"logits":self.workspace.logits,"logp":self.workspace.logp,"value":value}).to_string())
    }
}
