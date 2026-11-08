//! Local advice uses the identical CPU weights, observation packing and log-softmax.
use super::*;
use sts2_human_replay::{live::LiveMirror,party::Failure};

pub fn initialization_refusal(reason:&str)->String {
    json!({"ok":false,"error":{"code":"INITIALIZATION_REFUSED","path":reason}}).to_string()
}

#[derive(Clone)]
pub struct SeatAdvice {
    pub decision:DecisionId,
    pub slots:Vec<usize>,
    pub options:Vec<(Binding,f32)>,
}

/// Longest one seat's search may run before it is abandoned and the plain advice stands.
/// The node budget bounds normal work (2 samples/64 nodes: p99 about 250 ms per combat
/// decision under load); only a stuck simulator step comes near this. Well under the
/// mod's 5 s AI fallback (AiFallback.cs), so the advice arrives while it still applies.
pub const SEARCH_STALL_LIMIT:std::time::Duration=std::time::Duration::from_secs(2);
/// Set by the first search that stalls. Its thread may never return and every run makes
/// a new advisor, so no search thread starts again in this process: abandoned threads
/// stay bounded by the seats that were searching at that moment.
static SEARCH_STALLED_IN_PROCESS:std::sync::atomic::AtomicBool=std::sync::atomic::AtomicBool::new(false);

pub struct Advisor {
    mirror:Option<LiveMirror>,
    replay_clean:bool,
    // Shared with search threads, which may outlive a request when abandoned.
    policy:std::sync::Arc<CpuPolicy>,
    workspace:CpuWorkspace,
    relational:bool,
    pub meta:WebPolicyMeta,
    // The mirror replays the whole run from its seed; one replay serves every request
    // until the next consumed input (the C# side sends one request per seat).
    replayed:Option<Vec<sts2_human_replay::live::Pending>>,
    // The replayed run holds the hidden future; only search reads it, through determinization.
    replayed_run:Option<sts2_human_replay::live::LiveRun>,
    advised:Option<Vec<SeatAdvice>>,
    search_ws:Option<super::search::Workspace>,
    searched:Option<(super::search::SearchConfig,Vec<(u8,SeatSearch)>)>,
    // SEARCH_STALLED_IN_PROCESS; a test gives a group of advisors its own process.
    search_stalled:&'static std::sync::atomic::AtomicBool,
    // `search::search`; the advisor's and the bridge's tests substitute a faulty one.
    search:super::search::SearchFn,
}
/// One seat's search: its outcome, or why it could not run.
pub type SeatSearch=Result<super::search::Outcome,String>;
impl Advisor {
    pub fn request(&mut self,request:&str)->String {
        let result=(||->Result<Value,Value>{
            let error=|e:Failure|json!({"code":e.code,"seq":e.seq,"path":e.path});
            let request:Value=serde_json::from_str(request)
                .map_err(|e|json!({"code":"INVALID_REQUEST","path":e.to_string()}))?;
            match request["op"].as_str() {
                Some("meta")=>Ok(serde_json::to_value(&self.meta).unwrap()),
                Some("consume")=>{self.consume(request["row"].clone()).map_err(error)?;Ok(Value::Null)},
                Some("checked_pending")=>{
                    // Parsed once: the parity gate and the live fallback share it.
                    let live=match request.get("live_state") {
                        Some(value)=>match live_snapshot(value) {
                            Ok(snapshot)=>Some(snapshot),
                            Err(e)=>{self.replay_clean=false;return Err(error(e));},
                        },
                        None=>None,
                    };
                    let search=search_config(&request).map_err(error)?;
                    let replay=if request["prefer_live"]==true {Err(fail_live("LIVE_REQUESTED"))}
                        else {self.checked_pending(&request,search.as_ref())};
                    match replay {
                        Ok(value) if value.as_array().is_some_and(|v|!v.is_empty()) &&
                            live.as_ref().is_none_or(|snapshot|self.live_parity(snapshot,None)
                                .is_ok_and(|p|p["identical"]==true))=>Ok(value),
                        other=>{
                            if other.is_err() || live.is_some() {
                                self.replay_clean=false;
                            }
                            if let Some(snapshot)=&live {
                                self.live_pending(snapshot).map_err(error)
                            }else{other.map_err(error)}
                        },
                    }
                },
                Some("live_parity")=>{
                    let snapshot=live_snapshot(&request["live_state"]).map_err(error)?;
                    self.live_parity(&snapshot,Some(&request["live_state"])).map_err(error)
                },
                Some("pending")=>{
                    let search=search_config(&request).map_err(error)?;
                    let pending=self.pending().map_err(error)?;
                    let searched=match &search {Some(config)=>Some(self.search(config,None).map_err(error)?),None=>None};
                    Ok(Value::Array(pending.into_iter().map(|s|{
                        let mut seat=json!({"seat":s.decision.actor,"generation":s.decision.generation.to_string(),
                            "options":s.options.iter().map(|(binding,probability)|
                                json!({"binding":format!("{binding:?}"),"probability":probability})).collect::<Vec<_>>()});
                        if let Some(searched)=&searched {
                            seat["search"]=search_json(searched,s.decision.actor,|slot|s.slots.iter().position(|x|*x==slot)
                                .map(|i|json!(format!("{:?}",s.options[i].0))));
                        }
                        seat
                    }).collect()))
                },
                _=>Err(json!({"code":"UNKNOWN_OPERATION"})),
            }
        })();
        match result {Ok(value)=>json!({"ok":true,"value":value,"replay_clean":self.replay_clean}),
            Err(error)=>json!({"ok":false,"error":error,"replay_clean":self.replay_clean})}.to_string()
    }
    pub fn new(weights:&[u8],start:&[u8])->Result<Self,String>{
        if policy_is_joint(weights)? {return Err("advisor requires sts2-policy-web-v2".into());}
        let (policy,meta)=load(weights)?;
        let mirror=match LiveMirror::new(start) {
            Ok(mirror)=>Some(mirror),
            Err(e)=>if serde_json::from_slice::<Value>(start).ok().is_some_and(|v|v["live_advice"]==true) {None}
                else {return Err(format!("{}: {}",e.code,e.path));}
        };
        Ok(Self{replay_clean:mirror.is_some(),mirror,policy:std::sync::Arc::new(policy),meta,workspace:CpuWorkspace::default(),
            relational:observation_mode(&policy_fields(weights)?)?,replayed:None,replayed_run:None,advised:None,
            search_ws:None,searched:None,search_stalled:&SEARCH_STALLED_IN_PROCESS,search:super::search::search})
    }
    fn replayed(&mut self)->Result<&[sts2_human_replay::live::Pending],Failure>{
        if self.replayed.is_none() {
            let mirror=self.mirror.as_ref().ok_or_else(||fail_live("REPLAY_UNAVAILABLE"))?;
            let (pending,run)=mirror.pending_with_run(sts2_policy::native::encoder_for_policy(self.relational))?;
            self.replayed=Some(pending);self.replayed_run=Some(run);
        }
        Ok(self.replayed.as_deref().unwrap())
    }
    /// Decision-time search of the replayed seats in `seats` (every seat when None) with a
    /// combat choice; off unless requested. Teammates follow the policy inside each line;
    /// the plain advice is unchanged. A seat whose search cannot run (weights exported
    /// without the training discount, a refused root, a fault, a panic or a stall) carries
    /// the reason instead: advice never depends on search.
    pub fn search(&mut self,config:&super::search::SearchConfig,seats:Option<&[u8]>)->Result<Vec<(u8,SeatSearch)>,Failure>{
        use super::search::{Root,SimRun};
        if self.searched.as_ref().is_some_and(|(cached,_)|cached!=config) {self.searched=None;}
        self.replayed()?;
        let (pending,run)=(self.replayed.as_ref().unwrap(),self.replayed_run.as_ref().unwrap());
        let wanted=|actor:&u8|seats.is_none_or(|seats|seats.contains(actor));
        let cache=&self.searched.get_or_insert_with(||(*config,Vec::new())).1;
        let roots:Vec<Root>=pending.iter().filter(|seat|seat.legal.len()>=2 && wanted(&seat.decision.actor)
            && !cache.iter().any(|(s,_)|*s==seat.decision.actor)).filter_map(|seat|{
            let run=match run {
                sts2_human_replay::live::LiveRun::Solo(g)=>SimRun::Solo(g.clone()),
                sts2_human_replay::live::LiveRun::Party(g)=>SimRun::Party(g.clone()),
            };
            run.in_combat().then(||Root{run,decision:seat.decision,seats:pending.len(),net_base:1})
        }).collect();
        for root in roots {
            let actor=root.decision.actor;
            let result=self.search_isolated(root,config);
            self.searched.as_mut().unwrap().1.push((actor,result));
        }
        Ok(self.searched.as_ref().unwrap().1.iter().filter(|(actor,_)|wanted(actor)).cloned().collect())
    }
    /// One seat's search on its own thread, so neither a panic nor a stuck simulator step
    /// reaches the advice: a search with no result within `SEARCH_STALL_LIMIT` is
    /// abandoned to its thread and no further search starts in this process. The thread
    /// owns everything it reads, so destroying the advisor never waits for it.
    fn search_isolated(&mut self,root:super::search::Root,config:&super::search::SearchConfig)->SeatSearch {
        use std::sync::{atomic::Ordering,mpsc::RecvTimeoutError};
        if self.search_stalled.load(Ordering::Acquire) {
            return Err("search is off for this process: an earlier search stalled".into());
        }
        let (policy,reward,gamma,config,search)=(self.policy.clone(),self.meta.reward,self.meta.gamma,*config,self.search);
        let mut ws=self.search_ws.take().unwrap_or_else(||super::search::Workspace::new(self.relational));
        let (send,receive)=std::sync::mpsc::channel();
        // The same stack as the mod's advisor thread that ran searches before.
        let spawned=std::thread::Builder::new().name("sts2 decision search".into()).stack_size(16<<20).spawn(move||{
            let result=std::panic::catch_unwind(std::panic::AssertUnwindSafe(||
                search(&policy,&reward,gamma,&root,&config,&mut ws)));
            // A workspace a panic unwound through is not reused.
            let _=send.send(match result {Ok(result)=>(result,Some(ws)),Err(panic)=>(Err(format!("search panicked: {}",
                panic.downcast_ref::<&str>().copied().or(panic.downcast_ref::<String>().map(String::as_str)).unwrap_or("?"))),None)});
        });
        if let Err(error)=spawned {return Err(format!("search thread unavailable: {error}"));}
        match receive.recv_timeout(SEARCH_STALL_LIMIT) {
            Ok((result,ws))=>{self.search_ws=ws;result},
            Err(RecvTimeoutError::Timeout)=>{
                self.search_stalled.store(true,Ordering::Release);
                Err(format!("search stalled: no result within the {} s stall limit; abandoned, the plain advice stands and search is off for this process",
                    SEARCH_STALL_LIMIT.as_secs_f32()))
            },
            Err(RecvTimeoutError::Disconnected)=>Err("search ended without a result".into()),
        }
    }
    pub fn consume(&mut self,input:Value)->Result<(),Failure>{
        self.replayed=None;
        self.replayed_run=None;
        self.searched=None;
        self.advised=None;
        if let Some(mirror)=&mut self.mirror && let Err(error)=mirror.consume(input){
            self.replay_clean=false;
            self.mirror=None;
            return Err(error);
        }
        Ok(())
    }
    fn live_pending(&mut self,snapshot:&sts2_native_obs::live::Snapshot)->Result<Value,Failure>{
        let obs=snapshot.encode().map_err(|path|Failure{code:"LIVE_ENCODING_FAILED",seq:None,path})?;
        let view=obs.view().map_err(|_|fail_live("LIVE_OBSERVATION_FAILED"))?;
        if !snapshot.options.iter().any(|option|option.legal){return Ok(json!([]));}
        let decision=NativeDecision::from_view(view).map_err(|_|fail_live("LIVE_POLICY_OBSERVATION_FAILED"))?;
        let value=self.policy.forward(&decision,&mut self.workspace);
        let probabilities:Vec<f32>=self.workspace.logp.iter().map(|p|p.exp()).collect();
        if !value.is_finite() || probabilities.len()!=decision.slots.len() ||
            probabilities.iter().any(|p|!p.is_finite()||*p<0.||*p>1.) ||
            (probabilities.iter().map(|p|f64::from(*p)).sum::<f64>()-1.).abs()>1e-5 {
            return Err(fail_live("INVALID_POLICY_DISTRIBUTION"));
        }
        let mut mapped=vec![0.;snapshot.options.len()];
        for (&slot,probability)in decision.slots.iter().zip(probabilities){
            let index=usize::try_from(slot).map_err(|_|fail_live("INVALID_LIVE_POLICY_SLOT"))?;
            *mapped.get_mut(index).ok_or_else(||fail_live("INVALID_LIVE_POLICY_SLOT"))?=probability;
        }
        Ok(json!([{"seat":snapshot.seat,"path":"live","options":snapshot.options.iter().zip(mapped)
            .filter(|(option,_)|option.legal).map(|(option,probability)|json!({
                "binding":option.binding,"probability":probability})).collect::<Vec<_>>()}]))
    }
    /// `report` (the request's raw live state) asks for the full diagnostic report; the
    /// advice gate passes None and reads only `identical`.
    fn live_parity(&mut self,snapshot:&sts2_native_obs::live::Snapshot,report:Option<&Value>)->Result<Value,Failure>{
        let live=snapshot.encode().map_err(|path|Failure{code:"LIVE_ENCODING_FAILED",seq:None,path})?;
        let relational=self.relational;
        let pending=self.replayed()?;
        let replay=pending.iter().find(|p|p.decision.actor==snapshot.seat)
            .ok_or_else(||fail_live("REPLAY_SEAT_UNAVAILABLE"))?;
        let live_view=live.view().map_err(|_|fail_live("LIVE_OBSERVATION_FAILED"))?;
        let replay_view=replay.observation.view().map_err(|_|fail_live("NATIVE_OBSERVATION_FAILED"))?;
        let counts=(live_view.state_len,replay_view.state_len,live_view.candidate_len,replay_view.candidate_len);
        let a=NativeDecision::from_original_view(live_view).map_err(|_|fail_live("LIVE_POLICY_OBSERVATION_FAILED"))?;
        let b=NativeDecision::from_original_view(replay_view).map_err(|_|fail_live("POLICY_OBSERVATION_FAILED"))?;
        let a_json=a.trajectory_json(relational).unwrap();
        let b_json=b.trajectory_json(relational).unwrap();
        let Some(live_state)=report else {return Ok(json!({"identical":a_json==b_json}));};
        let feature_rows=|text:&str|{
            let value:Value=serde_json::from_str(text).unwrap();
            let mut counts=std::collections::BTreeMap::<String,u64>::new();
            for (index,row) in value["rows"].as_array().unwrap().iter().enumerate(){
                let weight=value["multiplicity"].get(index).and_then(Value::as_u64).unwrap_or(1);
                *counts.entry(serde_json::to_string(row).unwrap()).or_default()+=weight;
            }
            counts
        };
        let a_features=feature_rows(&a_json);let b_features=feature_rows(&b_json);
        let matching:u64=a_features.iter().map(|(row,count)|(*count).min(*b_features.get(row).unwrap_or(&0))).sum();
        let mut report=json!({"identical":a_json==b_json,
            "live_rows":counts.0,"replay_rows":counts.1,"live_options":counts.2,"replay_options":counts.3,
            "matching_row_features":matching,"live_feature_rows":a_features.values().sum::<u64>(),
            "replay_feature_rows":b_features.values().sum::<u64>()});
        report["encoder_input"]=json!({"live":serde_json::from_str::<Value>(&a_json).unwrap(),
            "reference":serde_json::from_str::<Value>(&b_json).unwrap()});
        report["raw_input"]=json!({"live":parity_rows(&live.view().unwrap()),
            "reference":parity_rows(&replay.observation.view().unwrap())});
        report["live_snapshot"]=live_state.clone();
        Ok(report)
    }
    fn checked_pending(&mut self,request:&Value,search:Option<&super::search::SearchConfig>)->Result<Value,Failure>{
        let fail=|code|Failure{code,seq:None,path:String::new()};
        let live=request["decisions"].as_array().ok_or_else(||fail("MISSING_LIVE_DECISIONS"))?;
        if live.len()>4 {return Err(fail("INVALID_LIVE_SEATS"));}
        let requested:Vec<u8>=live.iter().filter_map(|d|d["seat"].as_u64()).filter(|n|*n<4).map(|n|n as u8).collect();
        let pending=self.pending()?;
        let searched=match search {Some(config)=>Some(self.search(config,Some(&requested))?),None=>None};
        let mut results=Vec::new();
        let mut seats=Vec::new();
        for decision in live {
            let actor=decision["seat"].as_u64().filter(|n|*n<4).ok_or_else(||fail("INVALID_LIVE_SEAT"))? as u8;
            if seats.contains(&actor){return Err(fail("DUPLICATE_LIVE_SEAT"));}
            seats.push(actor);
            let options=decision["options"].as_array().ok_or_else(||fail("MISSING_LIVE_OPTIONS"))?;
            let Some(native)=pending.iter().find(|s|s.decision.actor==actor) else {continue};
            if options.len()!=native.options.len(){return Err(fail("LIVE_LEGAL_SET_MISMATCH"));}
            let parent=if decision["parent_seq"].is_null(){None}else{
                Some(decision["parent_seq"].as_u64().and_then(|n|usize::try_from(n).ok())
                    .ok_or_else(||fail("INVALID_PARENT_SEQUENCE"))?)
            };
            let mut positions=Vec::new();
            let mut checked=Vec::new();
            let mut originals=Vec::with_capacity(options.len());
            for option in options {
                let binding=option.as_str().ok_or_else(||fail("INVALID_LIVE_OPTION"))?;
                let action:Value=serde_json::from_str(binding).map_err(|_|fail("INVALID_LIVE_OPTION"))?;
                if action["player"].as_u64()!=Some(actor as u64){return Err(fail("LIVE_OPTION_SEAT_MISMATCH"));}
                originals.push(action);
            }
            let resolved=self.mirror.as_ref().ok_or_else(||fail("REPLAY_UNAVAILABLE"))?
                .resolve_many(&originals,decision["before"].clone(),parent,
                ||sts2_policy::native::encoder_for_policy(self.relational))?;
            for (option,chosen) in options.iter().zip(resolved) {
                let binding=option.as_str().unwrap();
                let position=native.slots.iter().position(|slot|*slot==chosen.candidate_slot)
                    .ok_or_else(||fail("LIVE_OPTION_NOT_LEGAL"))?;
                if chosen.actor!=actor || native.options[position].0.command!=chosen.command ||
                    positions.contains(&position) {return Err(fail("LIVE_BINDING_MISMATCH"));}
                positions.push(position);
                checked.push(json!({"binding":binding,"probability":native.options[position].1}));
            }
            let mut seat=json!({"seat":actor,"path":"replay","options":checked});
            if let Some(searched)=&searched {
                seat["search"]=search_json(searched,actor,|slot|native.slots.iter().position(|x|*x==slot)
                    .and_then(|native|positions.iter().position(|p|*p==native)).map(|i|options[i].clone()));
            }
            results.push(seat);
        }
        Ok(Value::Array(results))
    }
    pub fn pending(&mut self)->Result<Vec<SeatAdvice>,Failure>{
        if let Some(advice)=&self.advised {return Ok(advice.clone());}
        self.replayed()?;
        let pending=self.replayed.take().unwrap();
        let advice=self.advise(&pending);
        self.replayed=Some(pending);
        let advice=advice?;
        self.advised=Some(advice.clone());
        Ok(advice)
    }
    fn advise(&mut self,pending:&[sts2_human_replay::live::Pending])->Result<Vec<SeatAdvice>,Failure>{
        let fail=|code|Failure{code,seq:None,path:String::new()};
        let mut advice=Vec::new();
        for seat in pending {
            if seat.legal.len()<2 {continue;}
            let view=seat.observation.view().map_err(|_|fail("NATIVE_OBSERVATION_FAILED"))?;
            let decision=NativeDecision::from_view(view).map_err(|_|fail("POLICY_OBSERVATION_FAILED"))?;
            if decision.slots.len()!=seat.legal.len() || decision.slots.iter().zip(&seat.legal)
                .any(|(slot,(legal,_))|*slot as usize!=*legal) {
                return Err(fail("POLICY_LEGAL_SET_MISMATCH"));
            }
            let value=self.policy.forward(&decision,&mut self.workspace);
            let probabilities:Vec<f32>=self.workspace.logp.iter().map(|p|p.exp()).collect();
            let sum: f64=probabilities.iter().map(|p|*p as f64).sum();
            if !value.is_finite() || probabilities.len()!=seat.legal.len()
                || probabilities.iter().any(|p|!p.is_finite()||*p<0.||*p>1.)
                || (sum-1.).abs()>1e-5 {return Err(fail("INVALID_POLICY_DISTRIBUTION"));}
            advice.push(SeatAdvice{decision:seat.decision,
                slots:seat.legal.iter().map(|(slot,_)|*slot).collect(),
                options:seat.legal.iter().zip(probabilities).map(|((_,b),p)|(*b,p)).collect()});
        }
        Ok(advice)
    }
}

pub(super) fn parity_rows(view:&sts2_native_obs::ObservationView<'_>)->Value {
    use sts2_native_obs::{CANDIDATE_FIELDS,ROW_FIELDS,ContentId};
    let content=|id:ContentId|json!({"namespace":format!("{:?}",id.namespace),"id":id.value,
        "name":id.namespace.registry().and_then(|r|id.value.checked_sub(1)
            .and_then(|index|r.names.get(usize::from(index))))});
    let rows=(0..view.state_len).map(|index|{
        let rows=&view.rows;
        let mut numbers=serde_json::Map::new();
        for (lane,field) in ROW_FIELDS[rows.kind[index] as usize].iter().enumerate(){
            if rows.integer_present[index]&(1<<lane)!=0 {
                numbers.insert(field.name.into(),json!(rows.integers[index][lane]));
            }
            if rows.float_present[index]&(1<<lane)!=0 {
                numbers.insert(field.name.into(),json!(rows.floats[index][lane]));
            }
        }
        json!({"kind":format!("{:?}",rows.kind[index]),"model":content(rows.model[index]),
            "detail":rows.detail[index].map(content),"parent":rows.parent[index].0,
            "owner":rows.owner[index].0,"numbers":numbers,
            "flags":rows.flags[index],"flags_present":rows.flags_present[index]})
    }).collect::<Vec<_>>();
    let candidates=(0..view.candidate_len).map(|index|{
        let c=&view.candidates;
        let mut numbers=serde_json::Map::new();
        for (lane,field) in CANDIDATE_FIELDS.iter().enumerate(){
            if c.integer_present[index]&(1<<lane)!=0 {
                numbers.insert(field.name.into(),json!(c.integers[index][lane]));
            }
            if c.float_present[index]&(1<<lane)!=0 {
                numbers.insert(field.name.into(),json!(c.floats[index][lane]));
            }
        }
        json!({"action":format!("{:?}",c.action[index]),"model":content(c.model[index]),
            "source":c.source[index].0,"target":c.target[index].0,"numbers":numbers,
            "actor":c.actor[index],"detail":c.detail[index].map(content),
            "flags":c.flags[index],"flags_present":c.flags_present[index],
            "present":c.candidate_present[index/64]&(1<<(index%64))!=0,
            "legal":c.legal[index/64]&(1<<(index%64))!=0})
    }).collect::<Vec<_>>();
    json!({"rows":rows,"candidates":candidates,
        "edges":view.edges.iter().map(|edge|json!([edge.from.0,edge.to.0,
            format!("{:?}",edge.relation)])).collect::<Vec<_>>()})
}

/// Re-encode captured live facts while retaining the independently frozen reference.
pub fn encode_live_diagnostic(value:&Value,relational:bool)->Result<Value,String>{
    let snapshot:sts2_native_obs::live::Snapshot=serde_json::from_value(value.clone()).map_err(|e|e.to_string())?;
    let obs=snapshot.encode()?;
    let view=obs.view().map_err(|e|format!("{e:?}"))?;
    let raw=parity_rows(&view);
    let counts=(view.state_len,view.candidate_len);
    let decision=NativeDecision::from_original_view(view)?;
    let input:Value=serde_json::from_str(&decision.trajectory_json(relational).map_err(|e|e.to_string())?)
        .map_err(|e|e.to_string())?;
    Ok(json!({"encoder_input":input,"raw_input":raw,"rows":counts.0,"options":counts.1}))
}

fn fail_live(code:&'static str)->Failure {Failure{code,seq:None,path:String::new()}}
/// Search runs only when a request carries a `search` object; absent or null keeps it off.
fn search_config(request:&Value)->Result<Option<super::search::SearchConfig>,Failure>{
    let fail=|path|Failure{code:"INVALID_SEARCH_CONFIG",seq:None,path};
    match request.get("search") {
        None|Some(Value::Null)=>Ok(None),
        Some(value)=>{
            let config:super::search::SearchConfig=serde::Deserialize::deserialize(value).map_err(|e|fail(e.to_string()))?;
            config.validate().map_err(fail)?;
            Ok(Some(config))
        },
    }
}
/// A seat's search result with its options in the response's binding format.
/// Seats outside combat (or with one option) carry `{"reason":"not_searched"}`; a search
/// that could not run carries `{"reason":"unavailable","error":..}`.
fn search_json(searched:&[(u8,SeatSearch)],seat:u8,binding:impl Fn(usize)->Option<Value>)->Value {
    let outcome=match searched.iter().find(|(s,_)|*s==seat) {
        None=>return json!({"reason":"not_searched"}),
        Some((_,Err(error)))=>return json!({"reason":"unavailable","error":error}),
        Some((_,Ok(outcome)))=>outcome,
    };
    let at=|slot:i32|usize::try_from(slot).ok().and_then(&binding);
    json!({"reason":outcome.reason,"binding":at(outcome.slot),
        "policy_binding":outcome.candidates.iter().find(|c|c.position==outcome.policy_position).and_then(|c|at(c.slot)),
        "samples":outcome.samples,"nodes":outcome.nodes,"incomplete":outcome.incomplete,
        "candidates":outcome.candidates.iter().map(|c|json!({"binding":at(c.slot),"prior":c.prior,"q":c.q})).collect::<Vec<_>>()})
}
// Deserializes in place; the live state is the largest part of each request.
fn live_snapshot(value:&Value)->Result<sts2_native_obs::live::Snapshot,Failure>{
    serde::Deserialize::deserialize(value).map_err(|_|fail_live("INVALID_LIVE_SNAPSHOT"))
}
