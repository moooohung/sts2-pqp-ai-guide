//! Opt-in run-transition public event sink. No policy data or game decisions live here.
use std::{cell::RefCell, sync::{Arc, Mutex}};
use crate::{card::{CId,KRef,Card}, combat::{Combat, Props,PileKind}, creature::{Creature, Side, PRef}, tables::power_meta};

pub const MAX_JSON_INTEGER: u64 = (1 << 53) - 1;

#[derive(Clone, Debug)]
pub struct CreatureRef {
    pub kind: &'static str,
    pub owner: Option<CId>,
    pub creature_id: CId,
    pub combat_instance: u64,
}

#[derive(Clone, Debug)]
pub enum Source {
    Creature(CreatureRef),
    Model { kind: &'static str, owner: CId, model: &'static str },
}

#[derive(Clone, Copy, Debug)]
pub struct PublicState {
    pub model: &'static str, pub hp: i32, pub max_hp: i32, pub block: i32, pub alive: bool,
    pub in_combat: bool, pub escaped: bool,
}
impl PublicState {
    pub fn of(c: &Creature) -> Self {
        Self { model:c.monster_id, hp:c.hp, max_hp:c.max_hp, block:c.block,
            alive:c.is_alive(), in_combat:c.in_combat, escaped:c.escaped }
    }
}

#[derive(Clone, Debug)]
pub struct PublicCombatPlayer {
    pub target:CreatureRef, pub state:PublicState, pub powers:Vec<(&'static str,i32)>,
    pub hand_count:usize, pub piles:[usize;3],
}

#[derive(Clone, Debug)]
pub enum Details {
    Lifecycle,
    Enemy { powers:Vec<(&'static str,i32)>, roster:Vec<CId>, slot:Option<&'static str> },
    Boundary,
    Block,
    Power { model:&'static str, amount_before:Option<i32>, amount_after:Option<i32>,
        powers:Vec<(&'static str,i32)>, flash:bool },
    Card { card:Option<Box<Card>>, from:PileKind, to:PileKind, hand_count:usize,
        piles:[usize;3], count:usize },
    Hp { requested:i32, hp_gained:i32 },
    Combat { won:bool, players:Vec<PublicCombatPlayer> },
    Damage { requested:i32, blocked:i32, hp_lost:i32, overkill:i32, props:Props,
        intended_target:CreatureRef, block_owner:CreatureRef, block_before:i32, block_after:i32 },
}

#[derive(Clone, Debug)]
pub struct Event {
    pub seq: u64, pub kind: &'static str, pub op: &'static str,
    pub combat_instance: u64, pub round: i32, pub side: &'static str, pub stage: &'static str,
    pub owner: Option<CId>, pub source: Option<Source>, pub target: Option<CreatureRef>,
    pub span: u64, pub queue_seq: u64,
    pub before: PublicState, pub after: PublicState,
    pub details: Details,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OrbActivation {
    pub combat_id: u64,
    pub action_id: u64,
    pub sequence: u64,
    pub activation_id: u64,
    pub parent_activation_id: Option<u64>,
    pub owner_id: CId,
    pub orb_id: u64,
    pub orb_kind: &'static str,
    pub phase: &'static str,
    pub activation_index: u64,
    pub targets: Vec<CId>,
    pub display_slots: Vec<(CId, usize)>,
    pub dequeued: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct OrbContext {
    combat_id: u64,
    action_id: u64,
    activation_id: u64,
    parent_span_id: Option<u64>,
}

#[derive(Clone, Debug)]
pub struct OrbTrace {
    pub combat_id: u64,
    pub action_id: u64,
    pub sequence: u64,
    pub activation_id: u64,
    pub span_id: u64,
    pub parent_span_id: Option<u64>,
    pub operation: &'static str,
    pub transition: &'static str,
    pub target_id: Option<CId>,
}

#[derive(Clone, Debug)]
pub struct OrbEntity {
    pub combat_id: u64,
    pub id: CId,
    pub kind: &'static str,
    pub owner_id: Option<CId>,
    pub model: &'static str,
    pub display_slot: Option<usize>,
    pub slot: Option<&'static str>,
}

#[derive(Clone, Debug)]
struct OrbSpan {
    context: OrbContext,
    span_id: u64,
    parent_span_id: Option<u64>,
    operation: &'static str,
    target_id: Option<CId>,
}

#[derive(Debug, Default)]
struct Log {
    next_seq: u64, next_combat: u64, edge: u64,
    events: Vec<Event>, stage: &'static str, enemy: Option<CId>, source: Option<Source>,
    shuffling:Vec<(u64,KRef)>,
    orb_events: Vec<OrbActivation>,
    orb_stack: Vec<OrbContext>,
    next_orb_sequence: u64,
    orb_counts: Vec<(u64, u64, &'static str, u64)>,
    orb_traces: Vec<OrbTrace>,
    orb_trace_stack: Vec<u64>,
    orb_pending: Vec<(u64, usize)>,
    orb_deferred_spans: Vec<OrbSpan>,
    orb_canceled: Vec<u64>,
    orb_entities: Vec<OrbEntity>,
    fault: Option<&'static str>,
}
type Sink = Arc<Mutex<Log>>;
#[cfg(test)]
mod tests;
thread_local! { static ACTIVE: RefCell<Option<Sink>> = const { RefCell::new(None) }; }

#[derive(Clone, Debug, Default)]
pub struct Recorder(Sink);
pub struct Capture { previous: Option<Sink> }
impl Drop for Capture {
    fn drop(&mut self) { ACTIVE.with(|slot| *slot.borrow_mut() = self.previous.take()); }
}
impl Recorder {
    /// The recorder belongs to the run consumer; this guard spans dispatch AND automatic advancement.
    pub fn begin(&self, edge: u64) -> Capture {
        let mut log = self.0.lock().unwrap();
        log.events.clear(); log.orb_events.clear(); log.orb_stack.clear(); log.orb_counts.clear();
        log.orb_traces.clear(); log.orb_trace_stack.clear();
        log.orb_entities.clear();
        log.edge=edge; log.stage="action"; log.enemy=None; log.source=None;log.shuffling.clear();
        log.fault=(edge>MAX_JSON_INTEGER).then_some("edge-overflow");
        drop(log);
        Capture { previous:ACTIVE.with(|slot| slot.replace(Some(self.0.clone()))) }
    }
    /// Opt in an existing combat before its first replay frame is projected.
    pub fn record_combat(&self, combat: &mut Combat) {
        if combat.orb_recording { return; }
        if combat.between_instance == 0 { combat.between_instance = next_combat(&self.0); }
        combat.orb_recording = true;
        for player in &mut combat.players.0 {
            for orb in &mut player.orbs.orbs {
                combat.next_orb_instance += 1;
                orb.instance_id = combat.next_orb_instance;
            }
        }
    }
    pub fn snapshot(&self) -> (Vec<Event>, Option<&'static str>, Option<u64>) {
        let log=self.0.lock().unwrap();
        (log.events.clone(),log.fault,log.next_seq.checked_sub(1))
    }
    pub fn orb_activations(&self) -> Vec<OrbActivation> {
        self.0.lock().unwrap().orb_events.clone()
    }
    pub fn orb_traces(&self) -> Vec<OrbTrace> {
        self.0.lock().unwrap().orb_traces.clone()
    }
    pub fn orb_entities(&self) -> Vec<OrbEntity> {
        self.0.lock().unwrap().orb_entities.clone()
    }
}

fn next_combat(sink: &Sink) -> u64 {
    let mut log=sink.lock().unwrap();
    log.next_combat+=1;
    if log.next_combat>MAX_JSON_INTEGER {log.fault=Some("combat-instance-overflow");}
    log.next_combat
}

pub fn combat_created() -> u64 {
    ACTIVE.with(|slot| {
        let Some(sink)=slot.borrow().clone() else {return 0};
        next_combat(&sink)
    })
}

pub(crate) fn stage(stage: &'static str, enemy: Option<CId>) {
    ACTIVE.with(|slot| {
        if let Some(sink)=slot.borrow().as_ref() {
            let mut log=sink.lock().unwrap(); log.stage=stage; log.enemy=enemy;
        }
    });
}

pub(crate) struct SourceGuard { sink: Option<Sink>, previous: Option<Source> }
impl Drop for SourceGuard {
    fn drop(&mut self) {
        if let Some(sink)=&self.sink {sink.lock().unwrap().source=self.previous.take();}
    }
}
pub(crate) fn relic_source(owner:CId,model:&'static str)->SourceGuard {
    let sink=ACTIVE.with(|slot|slot.borrow().clone());
    let previous=sink.as_ref().and_then(|sink|sink.lock().unwrap().source.replace(
        Source::Model {kind:"relic",owner,model}));
    SourceGuard {sink,previous}
}

pub(crate) struct ShuffleCapture {sink:Option<Sink>,previous:Vec<(u64,KRef)>}
impl Drop for ShuffleCapture {
    fn drop(&mut self) {
        if let Some(sink)=&self.sink {sink.lock().unwrap().shuffling=std::mem::take(&mut self.previous);}
    }
}

pub(crate) struct OrbCapture { sink: Option<Sink>, activation_id: u64 }
impl OrbCapture {
    pub(crate) fn target(&self, target: CId) {
        if let Some(sink) = &self.sink {
            let mut log = sink.lock().unwrap();
            if let Some(event) = log.orb_events.iter_mut().find(|e| e.activation_id == self.activation_id) {
                event.targets.push(target);
            }
        }
    }
    pub(crate) fn targets(&self, targets: &[CId]) {
        for &target in targets { self.target(target); }
    }
}
impl Drop for OrbCapture {
    fn drop(&mut self) {
        if let Some(sink) = &self.sink {
            let mut log = sink.lock().unwrap();
            let popped = log.orb_stack.pop().map(|c| c.activation_id);
            debug_assert_eq!(popped, Some(self.activation_id));
        }
    }
}

fn push_orb_trace(log: &mut Log, span: &OrbSpan, transition: &'static str) {
    let sequence = log.next_orb_sequence;
    log.next_orb_sequence += 1;
    log.orb_traces.push(OrbTrace {
        combat_id: span.context.combat_id, action_id: span.context.action_id,
        sequence, activation_id: span.context.activation_id, span_id: span.span_id,
        parent_span_id: span.parent_span_id, operation: span.operation, transition,
        target_id: span.target_id,
    });
}

fn orb_pending(log: &Log, span_id: u64) -> usize {
    log.orb_pending.iter().find(|(id, _)| *id == span_id).map_or(0, |(_, n)| *n)
}

pub(crate) struct OrbTraceCapture { sink: Option<Sink>, span: Option<OrbSpan> }
impl Drop for OrbTraceCapture {
    fn drop(&mut self) {
        if let (Some(sink), Some(span)) = (&self.sink, self.span.take()) {
            let mut log = sink.lock().unwrap();
            let popped = log.orb_trace_stack.pop();
            debug_assert_eq!(popped, Some(span.span_id));
            if orb_pending(&log, span.span_id) > 0 {
                push_orb_trace(&mut log, &span, "suspend");
                log.orb_deferred_spans.push(span);
            } else {
                let transition = if let Some(i) = log.orb_canceled.iter().position(|&id| id == span.span_id) {
                    log.orb_canceled.remove(i);
                    "cancel"
                } else { "end" };
                push_orb_trace(&mut log, &span, transition);
            }
        }
    }
}

pub(crate) struct OrbContextCapture { sink: Option<Sink>, context: Option<OrbContext>, span_count: usize }
impl Drop for OrbContextCapture {
    fn drop(&mut self) {
        if let (Some(sink), Some(context)) = (&self.sink, self.context) {
            let mut log = sink.lock().unwrap();
            let popped = log.orb_stack.pop();
            debug_assert_eq!(popped, Some(context));
            for _ in 0..self.span_count { log.orb_trace_stack.pop(); }
        }
    }
}

impl Combat {
    pub(crate) fn record_orb_activation(&mut self, owner: CId, orb: crate::orbs::Orb,
        phase: &'static str, dequeued: bool) -> OrbCapture {
        if !self.orb_recording {
            return OrbCapture { sink: None, activation_id: 0 };
        }
        let Some(sink) = self.between_sink() else {
            return OrbCapture { sink: None, activation_id: 0 };
        };
        self.record_replay_entities();
        let mut log = sink.lock().unwrap();
        let combat_id = self.between_instance;
        let index = match log.orb_counts.iter_mut().find(|(combat, id, p, _)|
            *combat == combat_id && *id == orb.instance_id && *p == phase) {
            Some((_, _, _, count)) => { let index = *count; *count += 1; index }
            None => { log.orb_counts.push((combat_id, orb.instance_id, phase, 1)); 0 }
        };
        let sequence = log.next_orb_sequence;
        log.next_orb_sequence += 1;
        let event = OrbActivation {
            combat_id, action_id: log.edge, sequence, activation_id: sequence,
            parent_activation_id: log.orb_stack.last().map(|c| c.activation_id), owner_id: owner,
            orb_id: orb.instance_id, orb_kind: orb.kind.name(), phase,
            activation_index: index, targets: Vec::new(),
            display_slots: self.enemies.iter().enumerate().map(|(slot,&id)| (id,slot)).collect(),
            dequeued,
        };
        log.orb_events.push(event);
        let context = OrbContext { combat_id, action_id: log.edge, activation_id: sequence,
            parent_span_id: log.orb_trace_stack.last().copied() };
        log.orb_stack.push(context);
        drop(log);
        OrbCapture { sink: Some(sink), activation_id: sequence }
    }

    #[inline]
    pub(crate) fn current_orb_context(&mut self) -> Option<OrbContext> {
        if !self.orb_recording { return None; }
        let sink = self.between_sink()?;
        let log = sink.lock().unwrap();
        log.orb_stack.last().copied().map(|mut context| {
            context.parent_span_id = log.orb_trace_stack.last().copied();
            context
        })
    }

    pub(crate) fn restore_orb_context(&mut self, context: Option<OrbContext>, spans: &[u64]) -> OrbContextCapture {
        let Some(context) = context else { return OrbContextCapture { sink: None, context: None, span_count: 0 }; };
        let Some(sink) = self.between_sink() else { return OrbContextCapture { sink: None, context: None, span_count: 0 }; };
        let mut log = sink.lock().unwrap();
        log.orb_stack.push(context);
        log.orb_trace_stack.extend_from_slice(spans);
        drop(log);
        OrbContextCapture { sink: Some(sink), context: Some(context), span_count: spans.len() }
    }

    pub(crate) fn suspend_orb_context(&mut self, context: Option<OrbContext>) -> Vec<u64> {
        let Some(_) = context else { return Vec::new(); };
        let Some(sink) = self.between_sink() else { return Vec::new(); };
        let mut log = sink.lock().unwrap();
        let mut spans = Vec::new();
        for &id in &log.orb_trace_stack {
            if !spans.contains(&id) { spans.push(id); }
        }
        for &id in &spans {
            match log.orb_pending.iter_mut().find(|(span, _)| *span == id) {
                Some((_, count)) => *count += 1,
                None => log.orb_pending.push((id, 1)),
            }
        }
        spans
    }

    pub(crate) fn finish_orb_context(&mut self, spans: &[u64], transition: &'static str) {
        if spans.is_empty() { return; }
        let Some(sink) = self.between_sink() else { return; };
        let mut log = sink.lock().unwrap();
        for &id in spans.iter().rev() {
            if transition == "cancel" && !log.orb_canceled.contains(&id) {
                log.orb_canceled.push(id);
            }
            let pending = log.orb_pending.iter().position(|(span, _)| *span == id)
                .expect("recorded orb continuation span");
            log.orb_pending[pending].1 -= 1;
            if log.orb_pending[pending].1 == 0 {
                log.orb_pending.swap_remove(pending);
                if let Some(index) = log.orb_deferred_spans.iter().position(|span| span.span_id == id) {
                    let span = log.orb_deferred_spans.remove(index);
                    let transition = if let Some(i) = log.orb_canceled.iter().position(|&canceled| canceled == id) {
                        log.orb_canceled.remove(i);
                        "cancel"
                    } else { "end" };
                    push_orb_trace(&mut log, &span, transition);
                }
            }
        }
    }

    #[inline]
    pub(crate) fn orb_trace(&mut self, operation: &'static str, target_id: Option<CId>) -> OrbTraceCapture {
        if !self.orb_recording { return OrbTraceCapture { sink: None, span: None }; }
        let Some(sink) = self.between_sink() else { return OrbTraceCapture { sink: None, span: None }; };
        let mut log = sink.lock().unwrap();
        let Some(context) = log.orb_stack.last().copied() else {
            return OrbTraceCapture { sink: None, span: None };
        };
        let span = OrbSpan { context, span_id: log.next_orb_sequence,
            parent_span_id: log.orb_trace_stack.last().copied(), operation, target_id };
        push_orb_trace(&mut log, &span, "begin");
        log.orb_trace_stack.push(span.span_id);
        drop(log);
        OrbTraceCapture { sink: Some(sink), span: Some(span) }
    }

    fn record_replay_entities(&mut self) {
        if !self.orb_recording { return; }
        let Some(sink) = self.between_sink() else { return; };
        let mut log = sink.lock().unwrap();
        let combat_id = self.between_instance;
        for (id, creature) in self.creatures.iter().enumerate() {
            let id = id as CId;
            let kind = if creature.is_player { "player" } else if creature.is_pet { "pet" }
                else if creature.side == Side::Enemy { "enemy" } else { "ally" };
            let display_slot = if creature.is_player { Some(id as usize) }
                else if creature.side == Side::Enemy { self.enemies.iter().position(|&e| e == id) }
                else { self.allies.iter().position(|&e| e == id) };
            let entity = OrbEntity { combat_id, id, kind,
                owner_id: if creature.is_player { Some(id) } else { creature.pet_owner },
                model: creature.monster_id, display_slot, slot: creature.slot };
            if let Some(existing) = log.orb_entities.iter_mut().find(|e|
                e.combat_id == combat_id && e.id == id) {
                if entity.display_slot.is_some() { *existing = entity; }
            } else { log.orb_entities.push(entity); }
        }
    }

    fn between_sink(&mut self)->Option<Sink> {
        let sink=ACTIVE.with(|slot|slot.borrow().clone())?;
        if self.between_instance==0 {self.between_instance=combat_created();}
        Some(sink)
    }
    fn between_ref(&self,id:CId)->CreatureRef {
        let cr=self.cr(id);
        CreatureRef {kind:if cr.is_player {"player"}else if cr.is_pet {"pet"}else if cr.side==Side::Enemy {"enemy"}else{"ally"},
            owner:if cr.is_player {Some(id)}else{cr.pet_owner},creature_id:id,combat_instance:self.between_instance}
    }
    fn record_between(&mut self,kind:&'static str,op:&'static str,id:Option<CId>,before:PublicState,
        details:Details,source:Option<Source>) {
        let Some(sink)=self.between_sink() else {return};
        let mut log=sink.lock().unwrap();
        if log.next_seq>MAX_JSON_INTEGER {log.fault=Some("sequence-overflow");return;}
        let source=source.or_else(||log.source.clone()).or_else(||self.active_card.map(|k|Source::Model {
            kind:"card",owner:self.cards[k as usize].owner,model:self.cards[k as usize].id.name(),
        })).or_else(||log.enemy.map(|enemy|Source::Creature(CreatureRef {
            kind:"enemy",owner:None,creature_id:enemy,combat_instance:self.between_instance,
        })));
        let target=id.map(|id|self.between_ref(id));
        let event=Event {seq:log.next_seq,kind,op,combat_instance:self.between_instance,
            round:self.round,side:match log.stage {
                "enemy_start"|"enemy_action"|"enemy_end"=>"enemy",
                "player_start"|"turn_end"=>"player",
                _=>if self.side==Side::Enemy {"enemy"}else{"player"},
            },
            stage:log.stage,owner:target.as_ref().and_then(|t|t.owner),source,target,
            span:log.edge,queue_seq:self.action_queue.next_sequence()+self.action_queue.run_sequence_offset,
            before,after:PublicState::of(self.cr(id.unwrap_or(0))),details};
        log.next_seq+=1;
        log.events.push(event);
    }
    pub(crate) fn record_creature_lifecycle(&mut self,id:CId,op:&'static str,before:PublicState) {
        self.record_replay_entities();
        if self.cr(id).monster_id=="OSTY" && self.cr(id).pet_owner.is_some() {
            self.record_between("lifecycle",op,Some(id),before,Details::Lifecycle,None);
        } else if self.cr(id).side==Side::Enemy && self.between_sink().is_some() {
            let cr=self.cr(id);
            let powers=cr.power_list().into_iter().filter_map(|p|{
                let power=&self.powers[p as usize];
                power_meta(power.id).visible.then_some((power.id.name(),power.amount))
            }).collect();
            let details=Details::Enemy {powers,roster:self.enemies.to_vec(),slot:cr.slot};
            self.record_between("lifecycle",op,Some(id),before,details,None);
        }
    }
    pub(crate) fn record_public_heal(&mut self,id:CId,op:&'static str,before:PublicState,requested:i32) {
        if self.between_sink().is_none(){return;}
        let hp_gained=(self.cr(id).hp-before.hp).max(0);
        self.record_between("hp",op,Some(id),before,Details::Hp {requested,hp_gained},None);
    }
    pub(crate) fn record_combat_result(&mut self) {
        if self.between_sink().is_none(){return;}
        self.record_replay_entities();
        let players=(0..self.players.len() as CId).map(|owner|{
            let p=&self.players[owner];
            let powers=self.cr(owner).power_list().into_iter().filter_map(|p|{
                let power=&self.powers[p as usize];
                power_meta(power.id).visible.then_some((power.id.name(),power.amount))
            }).collect();
            PublicCombatPlayer {target:self.between_ref(owner),state:PublicState::of(self.cr(owner)),powers,
                hand_count:p.hand.len(),piles:[p.draw.len(),p.discard.len(),p.exhaust.len()]}
        }).collect();
        self.record_between("combat","end",None,PublicState::of(self.cr(0)),
            Details::Combat {won:self.won,players},None);
    }
    pub(crate) fn between_stage(&mut self,name:&'static str,enemy:Option<CId>) {
        stage(name,enemy);
        self.record_between("boundary",name,enemy,PublicState::of(self.cr(enemy.unwrap_or(0))),Details::Boundary,None);
    }
    pub(crate) fn gain_recorded_block(&mut self,id:CId,amount:i32) {
        let _block = self.orb_trace("block", Some(id));
        let before=PublicState::of(self.cr(id));
        self.cr_mut(id).gain_block(amount);
        self.record_block_change(id,"gain",before);
    }
    pub(crate) fn record_block_change(&mut self,id:CId,op:&'static str,before:PublicState) {
        if before.block!=self.cr(id).block {
            self.record_between("block",op,Some(id),before,Details::Block,None);
        }
    }
    pub(crate) fn record_damage_hit(&mut self,id:CId,intended:CId,block_owner:CId,
        before:PublicState,block_before:i32,requested:i32,blocked:i32,hp_lost:i32,overkill:i32,
        props:Props,dealer:Option<CId>) {
        if self.between_sink().is_none() {return;}
        let details=Details::Damage {requested,blocked,hp_lost,overkill,props,
            intended_target:self.between_ref(intended),block_owner:self.between_ref(block_owner),
            block_before,block_after:self.cr(block_owner).block};
        self.record_between("damage","hit",Some(id),before,details,
            dealer.map(|id|Source::Creature(self.between_ref(id))));
    }
    pub(crate) fn record_public_power(&mut self,p:PRef,op:&'static str,before:Option<i32>,present:bool,flash:bool) {
        if self.between_sink().is_none() {return;}
        let power=&self.powers[p as usize];
        if !power_meta(power.id).visible {return;}
        let (owner,model,amount)=(power.owner,power.id.name(),power.amount);
        if present&&!self.cr(owner).power_list().contains(&p) {return;}
        let powers=self.cr(owner).power_list().into_iter().filter_map(|p|{
            let power=&self.powers[p as usize];
            power_meta(power.id).visible.then_some((power.id.name(),power.amount))
        }).collect();
        let details=Details::Power {model,amount_before:before,amount_after:present.then_some(amount),powers,flash};
        let source=(op=="trigger").then_some(Source::Model {kind:"power",owner,model});
        self.record_between("power",op,Some(owner),PublicState::of(self.cr(owner)),details,source);
    }
    pub(crate) fn set_recorded_power_amount(&mut self,p:PRef,amount:i32) {
        let before=self.powers[p as usize].amount;
        self.powers[p as usize].amount=amount;
        if before!=amount {self.record_public_power(p,"change",Some(before),true,false);}
    }
    fn card_event_details(&self,owner:CId,card:Option<Box<Card>>,from:PileKind,to:PileKind,count:usize)->Details {
        let p=&self.players[owner];
        Details::Card {card,from,to,hand_count:p.hand.len(),
            piles:[p.draw.len(),p.discard.len(),p.exhaust.len()],count}
    }
    pub(crate) fn record_card_move(&mut self,k:KRef,from:PileKind) {
        let Some(sink)=self.between_sink() else{return};
        let to=self.card_pile(k);
        if from==to{return;}
        if from==PileKind::Discard&&to==PileKind::Draw&&
            sink.lock().unwrap().shuffling.contains(&(self.between_instance,k)){return;}
        let owner=self.cards[k as usize].owner;
        let details=self.card_event_details(owner,Some(Box::new(self.cards[k as usize].clone())),from,to,1);
        self.record_between("card","move",Some(owner),PublicState::of(self.cr(owner)),details,None);
    }
    pub(crate) fn record_card_play(&mut self,k:KRef) {
        if self.between_sink().is_none(){return;}
        let owner=self.cards[k as usize].owner;
        let details=self.card_event_details(owner,Some(Box::new(self.cards[k as usize].clone())),PileKind::Play,PileKind::Play,1);
        self.record_between("card","play",Some(owner),PublicState::of(self.cr(owner)),details,None);
    }
    pub(crate) fn record_shuffle(&mut self,owner:CId,cards:&[KRef])->ShuffleCapture {
        let sink=self.between_sink();
        let previous=sink.as_ref().map(|sink|std::mem::replace(&mut sink.lock().unwrap().shuffling,
            cards.iter().map(|&k|(self.between_instance,k)).collect())).unwrap_or_default();
        if sink.is_some(){
            let details=self.card_event_details(owner,None,PileKind::Discard,PileKind::Draw,cards.len());
            self.record_between("card","shuffle",Some(owner),PublicState::of(self.cr(owner)),details,None);
        }
        ShuffleCapture {sink,previous}
    }
}
