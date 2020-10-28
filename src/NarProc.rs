use rand::Rng;
use std::cell::RefCell;
use std::rc::Rc;
use std::sync::{Arc, Mutex};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{sync_channel, SyncSender};
use std::thread;
use std::thread::JoinHandle;
use parking_lot::RwLock;

use crate::NarStamp::*;
use crate::NarSentence::*;
use crate::Term::*;
use crate::TermApi::*;
use crate::NarGoalSystem;
use crate::NarMem;

/// contains all necessary variables of a procedural NAR
pub struct ProcNar {
    /// base for the exponential intervals
    pub cfgIntervalExpBase:f64,
    /// maximal interval time
    pub cfgIntervalMax:i64,

    /// perception window for current events
    pub cfgPerceptWindow:i64,
    /// decision threshold for decision making
    pub cfgDescnThreshold:f64,

    /// maximal number of evidence
    pub cfgNMaxEvidence:i64,

    /// how ofter should event-FIFO get sampled for perception in cycle?
    pub cfgPerceptionSamplesPerStep:i64,
    
    /// enable motor babbling?
    pub cfgEnBabbling:bool,

    /// how many ops can a impl seq maximally contain, values above 1 are considered as EXPERIMENTAL
    pub cfg__nOpsMax:i64,
    /// how high is the proability to select multiple ops for seq impl candidates
    pub cfg__multiOpProbability:f64,


    /// how many concepts does it store at max (soft limit)
    pub cfg__nConcepts:i64,


    /// how verbose is the reasoner, mainly used for debugging
    pub cfgVerbosity:i64,

    /// memory with the (procedural) evidence
    pub evidenceMem: Arc<RwLock<NarMem::Mem>>,

    /// trace of some past events under AIKR
    pub trace: Vec<Rc<SimpleSentence>>,
    /// all anticipated events "in flight"
    pub anticipatedEvents: Vec<AnticipationEvent>,

    /// all registered ops
    pub ops: Vec<Box<dyn Op>>,

    /// NAR time
    pub t:i64,


    pub rng: rand::rngs::ThreadRng,

    /// table with exponential intervals
    pub expIntervalsTable:Vec<i64>,

    /// "goal system" - manages goals of the procedural reasoner
    pub goalSystem: NarGoalSystem::GoalSystem,



    // internals which are still public

    /// array of workers
    pub storeWorkers: Vec<JoinHandle<()>>,
    /// sender to worker
    pub storeWorkersTx: Vec<SyncSender<SentenceDummy>>,
}

/// init and set to default values
pub fn narInit() -> ProcNar {
    let mut nar = ProcNar {
        cfgIntervalExpBase: 1.3,
        cfgIntervalMax: 20,
        cfgPerceptWindow: 2,
        cfgDescnThreshold: 0.58,
        cfgNMaxEvidence: 5000,
        cfgPerceptionSamplesPerStep:4,
        cfgEnBabbling: true,
        cfg__nOpsMax: 1,
        cfg__multiOpProbability: 0.2,


        cfg__nConcepts: 1000,
        
        cfgVerbosity: 0, // be silent

        //evidence: Vec::new(),
        evidenceMem: Arc::new(RwLock::new(NarMem::make())),

        trace: Vec::new(),
        anticipatedEvents: Vec::new(),
        ops: Vec::new(),
        t: 0,

        rng: rand::thread_rng(),

        expIntervalsTable: Vec::new(),

        goalSystem: NarGoalSystem::makeGoalSystem(20, 8),

        storeWorkers: vec![],
        storeWorkersTx: vec![],
    };


    // create worker which stores evidence with revision
    for _iWorker in 0..1 {
        let (tx, rx) = sync_channel(4); // create channel with fixed size, reason is that we want to limit backlog!
        nar.storeWorkersTx.push(tx);

        let evidenceMem = Arc::clone(&nar.evidenceMem);
        nar.storeWorkers.push(thread::spawn(move|| {
            loop {
                let msgRes = rx.recv();
                if !msgRes.is_ok() {
                    break; // other side has hung up, terminate this worker
                }
                let evidenceSentence = msgRes.unwrap(); // receive message
                println!("[STORAGE WORKER] received MSG!");

                /////////
                // STORE
                /////////
                let addEvidenceFlag: Arc<AtomicBool> = Arc::new(AtomicBool::new(true)); // do we need to add new evidence?
                
                { // scope for guard
                    let evidenceMemGuard = evidenceMem.read();
                    for iEEArc in &NarMem::ret_beliefs_by_terms_nonunique(&evidenceMemGuard, &[retSeqCond(&evidenceSentence.term).clone(), retPred(&evidenceSentence.term).clone()]) { // iterate over evidence where seqCond and/or pred appear
                        let mut iEE = iEEArc.lock().unwrap();
                        
                        if !checkOverlap(&iEE.stamp, &evidenceSentence.stamp) { // evidence must no overlap!
                            if
                                iEE.expDt.unwrap() >= evidenceSentence.expDt.unwrap() && // check for greater because we want to count evidence for longer intervals too, because longer ones are "included"
                                
                                // does impl seq match?
                                checkEqTerm(&iEE.term, &evidenceSentence.term)
                            {
                                iEE.stamp = merge(&iEE.stamp, &evidenceSentence.stamp);
                                match iEE.evi.as_ref().unwrap() {
                                    Evidence::CNT{pos,cnt} => {
                                        iEE.evi = Some(Evidence::CNT{pos:pos+1,cnt:cnt+1}); // bump positive counter
                                    },
                                    _ => {panic!("expected CNT!");}
                                }
                                
                                if false {println!("dbg - REV")};
                                
                                addEvidenceFlag.store(false, Ordering::Relaxed); // because we revised
                            }                                
                        }
                    }    
                }
                
                if addEvidenceFlag.load(Ordering::Relaxed) {
                    // add evidence
                    mem_add_evidence(Arc::clone(&evidenceMem), &evidenceSentence);
                }
            }
        }));
    }



    // build table with exponential intervals
    {
        let mut lastExpInterval:i64 = 0;
        let mut i:i64 = 0;
        loop {
            let thisInterval = nar.cfgIntervalExpBase.powf(i as f64) as i64;
            if thisInterval > nar.cfgIntervalMax {
                break; // we collected all intervals
            }
            if thisInterval > lastExpInterval {
                lastExpInterval = thisInterval;
                nar.expIntervalsTable.push(thisInterval); // store
            }
            
            i+=1;
        }
    }
    
    nar
}

/// add procedural evidence to memory
pub fn mem_add_evidence(evidenceMem: Arc<RwLock<NarMem::Mem>>, evidenceSentence: &SentenceDummy) {
    // enumerate subterms to decide concept names where we store the belief
    let subterms = {
        let mut subterms = vec![];
        // enumerate subterms from first seq element of predimpl and predicate of predimpl
        subterms.extend(retSubterms(&retSeqCond(&evidenceSentence.term)));
        subterms.extend(retSubterms(&retPred(&evidenceSentence.term)));
        subterms
    };

    NarMem::storeInConcepts2(&mut evidenceMem.write(), &evidenceSentence, &subterms);
}

/// returns all evidence, can be overlapping!
pub fn mem_ret_evidence_all_nonunique(procNar:&ProcNar) -> Vec<Arc<Mutex<SentenceDummy>>> {
    let mut res = vec![];
    for (ikey, _iConcept) in &procNar.evidenceMem.read().concepts {
        let beliefsOfConcept = NarMem::ret_beliefs_of_concept(&procNar.evidenceMem.read(), &ikey);

        // add to result
        for iBelief in beliefsOfConcept.iter() {
            res.push(Arc::clone(iBelief));
        }
    }
    res
}

/// helper which tries to build a impl seq out of events
/// ex: [a, b, c]  returns (a, b) =/> c
pub fn try_build_implSeq(nar:&ProcNar, events:&[Term]) -> Option<Term> {
    let middle = &events[1..events.len()-1];
    assert!(middle.len() >= 1, ""); // check if the code to select middle is right
    if
        middle.iter().map(|iItem| checkIsCallableOp(&nar, &iItem)).all(|x| x == true) && // middle must be callable ops
        // first and last must not be op
        !checkIsCallableOp(&nar, &events[0])  &&
        !checkIsCallableOp(&nar, &events[events.len()-1]) && 
        !checkEqTerm(&events[0], &events[events.len()-1]) // first and last event must not be the same
    {
        let mut seq2:Vec<Term> = vec![events[0].clone()];
        for i in middle {
            seq2.push(i.clone());
        }

        let implSeq = s(Copula::PREDIMPL, &seq(&seq2), &events[events.len()-1]);
        return Some(implSeq);
    };
    None
}

/// does first work of one reasoner step
pub fn narStep0(nar:&mut ProcNar) {
    if nar.cfgVerbosity > 0 {println!("ae# = {}", nar.anticipatedEvents.len());}; // debug number of anticipated events
    
    // remove confirmed anticipations
    for perceptIdx in 0..nar.cfgPerceptWindow as usize {
        if nar.trace.len() > perceptIdx {
            let curEvent:&Term = &nar.trace[nar.trace.len()-1-perceptIdx].name;
            
            let mut newanticipatedEvents = Vec::new();
            for iDeadline in &nar.anticipatedEvents {
                let evi = iDeadline.evi.lock().unwrap();
                if !checkEqTerm( &retPred(& evi.term), &curEvent) { // is predicted event not current event?
                    newanticipatedEvents.push(iDeadline.clone());
                }
            }
            nar.anticipatedEvents = newanticipatedEvents;
        }
    }

    
    { // neg confirm for anticipated events
        {
            for iDeadlineViolated in nar.anticipatedEvents.iter().filter(|v| v.deadline <= nar.t) {
                let mut mutEviGuard = iDeadlineViolated.evi.lock().unwrap();
                
                // KEYWORD< neg-confirm >
                if false {match mutEviGuard.evi.as_ref().unwrap() {
                    Evidence::CNT{pos,cnt} => {
                        println!("TRACE anticipation: before neg conf   evidence: +/n {}/{}", pos, cnt); // evidence before neg-confirm
                    },
                    _ => {panic!("expected CNT!");}
                }}

                match mutEviGuard.evi.as_ref().unwrap() {
                    Evidence::CNT{pos,cnt} => {
                        mutEviGuard.evi = Some(Evidence::CNT{pos:*pos,cnt:cnt+1}); // add negative evidence
                    },
                    _ => {panic!("expected CNT!");}
                }

                if false {match mutEviGuard.evi.as_ref().unwrap() {
                    Evidence::CNT{pos,cnt} => {
                        println!("TRACE anticipation: after neg conf   evidence: +/n {}/{}", pos, cnt); // evidence before neg-confirm
                    },
                    _ => {panic!("expected CNT!");}
                }}
            }
        }
        
        nar.anticipatedEvents = nar.anticipatedEvents.iter().filter(|&iDeadline| iDeadline.deadline > nar.t).map(|v| v.clone()).collect();
    }

    // neutralize goals which are fullfilled by current event
    if nar.trace.len() > 0 {
        let lastEvent:&SimpleSentence = &*nar.trace[nar.trace.len()-1];
        NarGoalSystem::event_occurred(&mut nar.goalSystem, &lastEvent.name);
    }

    if nar.trace.len() >= 3 { // add evidence
        for _sampleIt in 0..nar.cfgPerceptionSamplesPerStep {
            // filter middle by ops and select random first event before that!
            let idxsOfOps:Vec<i64> = calcIdxsOfOps(&nar, &nar.trace);
            if idxsOfOps.len() > 0 { // there must be at least one op to sample


                let selIdxOfOps: Vec<usize> = {// indices of selected ops
                    
                    let nSelOps = // how many ops do we try to select
                        if nar.cfg__nOpsMax == 1 {1}
                        else if nar.rng.gen_range(0.0, 1.0) < nar.cfg__multiOpProbability {
                            idxsOfOps.len().min(nar.cfg__nOpsMax as usize)
                        }
                        else {1};

                    // select ops
                    let mut selIdxOfOps = vec![];
                    loop {
                        let idx1Idx = nar.rng.gen_range(0, idxsOfOps.len());
                        let selIdx = idxsOfOps[idx1Idx] as usize;
                        if !selIdxOfOps.contains(&selIdx) {
                            selIdxOfOps.push(selIdx);
                        }

                        if selIdxOfOps.len() == nSelOps { // do we have selected the ops?
                            break;
                        }
                    }

                    selIdxOfOps
                };
                
                if selIdxOfOps.len() > 0 && *selIdxOfOps.iter().min().unwrap() > 0 { // is there a valid index for a op which is not the last item in the trace?
                    
                    let selTraceItems: Vec<Rc<SimpleSentence>> = {
                        let idxFirst = nar.rng.gen_range(0, selIdxOfOps.iter().min().unwrap()); // select index of event before first selected op
                        let mut idxLast = nar.trace.len()-1; // last event is last

                        // TODO< rewrite to logic which scans for the first op between idxLast and selIdxOfOps, select random event as idxLast between these!
                        
                        // check if we can select previous event
                        {
                            let sel = nar.trace[nar.trace.len()-1-1].clone();
                            let rng0:i64 = nar.rng.gen_range(0, 2);
                            if rng0 == 1 && nar.trace.len()-1-1 > *selIdxOfOps.iter().max().unwrap() && !checkIsCallableOp(&nar, &sel.name) {
                                idxLast = nar.trace.len()-1-1;
                            }
                        }
                        let idxs = { // compose indices of selected events
                            let mut idxs = vec![idxFirst];
                            idxs.extend(selIdxOfOps);
                            idxs.push(idxLast);
                            idxs.sort();
                            idxs
                        };

                        idxs.iter().map(|idx| Rc::clone(&nar.trace[*idx])).collect() // select trace items
                    };
                    
                    let termsOfSelVecItems:Vec<Term> = selTraceItems.iter().map(|iv| iv.name.clone()).collect();
                    let implSeqOpt: Option<Term> = try_build_implSeq(nar, &termsOfSelVecItems); // try to build impl seq from selected trace items
                    if implSeqOpt.is_some() { // was building of impl seq successful?
                        let candidateTerm:Term = implSeqOpt.unwrap().clone();
                        
                        if nar.cfgVerbosity > 0 {println!("perceive {}", convTermToStr(&candidateTerm));};
                        
                        // compute time between last event and the element before the last event
                        let dt:i64 = selTraceItems[selTraceItems.len()-1].occT - selTraceItems[selTraceItems.len()-2].occT;
                        // compute exponential delta time
                        let expDt:i64 = findMinTableIdx(dt, &nar.expIntervalsTable);
                        


                        
                        let stamp:Stamp = {
                            // compute merged stamp from evidence of all events
                            let stampEvi = selTraceItems.iter().map(|iv| iv.evi).collect();
                            newStamp(&stampEvi)
                        };

                        let evidenceSentence: SentenceDummy = SentenceDummy {
                            punct:EnumPunctation::JUGEMENT,
                            t:None,
                            stamp:stamp,
                            expDt:Some(expDt),
                            term:Arc::new(candidateTerm.clone()), // ex: (e0 &/ e1) =/> e2
                            evi:Some(Evidence::CNT{pos:1,cnt:1})
                        };
                        
                        let workerIdx = nar.rng.gen_range(0, nar.storeWorkersTx.len());
                        nar.storeWorkersTx[workerIdx].send(evidenceSentence).unwrap(); // defer actual storage to worker
                    }
                }
            }
        }
        
    }
    
}

/// does second part of reasoner step
pub fn narStep1(nar:&mut ProcNar) {    
    let mut pickedAction:Option<Term> = None; // complete term of op
    {
        let mut bestEntry:(f64, Option<(Rc<RefCell<NarGoalSystem::Entry>>, Term)>) = (0.0, None); // best entry from goal system to execute

        // * search if we can satisfy goal
        for perceptIdx in 0..nar.cfgPerceptWindow as usize {
            if nar.trace.len() > perceptIdx {

                let checkedState:Term = nar.trace[nar.trace.len()-1-perceptIdx].name.clone();

                // check if current state "leads" to action
                // tuple is (exp, entity)
                let thisEntry: (f64, Option<(Rc<RefCell<NarGoalSystem::Entry>>, Term)>) = NarGoalSystem::selHighestExpGoalByState(&nar.goalSystem, &checkedState);

                if thisEntry.0 > bestEntry.0 { // if exp is higher -> is a better choice
                    bestEntry = thisEntry;
                }
            }
        }



        // * pick action and expected event to anticipations
        if bestEntry.0 > nar.cfgDescnThreshold && bestEntry.1.is_some() {
            let entryAndUnfied:&(Rc<RefCell<NarGoalSystem::Entry>>, Term) = &bestEntry.1.unwrap();
            let entity:&Rc<RefCell<NarGoalSystem::Entry>> = &entryAndUnfied.0;
            let pickedEvidenceOpt: &Option<Arc<Mutex<SentenceDummy>>> = &entity.borrow().evidence;
            
            if pickedEvidenceOpt.is_some() {
                let pickedEvidence: Arc<Mutex<SentenceDummy>> = Arc::clone(&pickedEvidenceOpt.as_ref().unwrap());

                // extract op of seq
                let opTerm:Term = retSeqOp(&entryAndUnfied.1);


                { // info
                    let implSeqAsStr = convTermToStr(&entryAndUnfied.1); // unified
                    let actAsStr:String = convTermToStr(&opTerm);
                    let pickedExp:f64 = bestEntry.0;
                    if nar.cfgVerbosity > 0 {println!("descnMaking: found best act = {}   implSeq={}    exp = {}", &actAsStr, &implSeqAsStr, pickedExp)};
                }
                
                // try to decode op into args and name
                let decodedOpOpt: Option<(Vec<Term>,String)> = decodeOp(&opTerm);
                if decodedOpOpt.is_some() { // we can only exec op if op is valid format
                    //let decodedOpArgsAndName:(Vec<Term>,String) = decodedOpOpt.unwrap();

                    pickedAction = Some(opTerm.clone());
                    
                    // add anticipated event
                    let expIntervalIdx:i64 =
                        if pickedEvidence.lock().unwrap().expDt.is_some() {
                            pickedEvidence.lock().unwrap().expDt.unwrap()
                        }
                        else {0}; // else it needs a default interval
                    let interval:i64 = nar.expIntervalsTable[expIntervalIdx as usize];
                    let deadline:i64 = nar.t + interval; // compute real deadline by exponential interval
                    nar.anticipatedEvents.push(AnticipationEvent {
                        evi:Arc::clone(&pickedEvidence),
                        deadline:deadline,
                    });
                }
            }
        }
    }
    
    match &pickedAction {
        Some(_) => {},
        None => {
            if nar.cfgEnBabbling && nar.ops.len() > 0 { // we have to have ops to sample from
                // TODO< better distribution >
                let p = nar.rng.gen_range(0, nar.ops.len()*9);
                if p < nar.ops.len() {
                    let idx = p;
                    let opName: &String = &nar.ops[idx].retName(); // sel op

                    let callTerm:Term = encodeOp(&vec![Term::SetExt(vec![Box::new(Term::Name("SELF".to_string()))])], opName);
                    
                    if nar.cfgVerbosity > 5 {println!("procedural: babbling: picked act {}", &convTermToStr(&callTerm));};
                    
                    pickedAction = Some(callTerm.clone());
                }
            }
        }
    }
    
    
    match &pickedAction {
        Some(term) => {
            let (opArgs, opName) = decodeOp(&term).unwrap();

            // scan for action
            for iOp in &nar.ops {
                if iOp.retName() == opName.clone() {
                    iOp.call(&opArgs); // call op
                    break;
                }
            }
            
            println!("{}!", &convTermToStr(&term)); // print execution

            nar.trace.push(Rc::new(SimpleSentence {name:term.clone(),evi:nar.t,occT:nar.t}));
        },
        None => {},
    }
    
    
    // limit trace (AIKR)
    if nar.trace.len() > 20 {
        nar.trace = (&nar.trace[nar.trace.len()-20..]).to_vec();
    }

    // limit evidence (AIKR)
    if nar.t % 101 == 1 {
        NarMem::limitMemory(&mut nar.evidenceMem.write(), nar.cfg__nConcepts as usize);
    }


    // give goal system resources
    if nar.t % 3 == 0 {
        let enGoalSystem = true; // DISABLED because we want to test it without deriving goals!
        if enGoalSystem {
            NarGoalSystem::sampleAndInference(&mut nar.goalSystem, nar.t, &nar.evidenceMem.read(), &mut nar.rng);
        }
    }

    if nar.t % 13 == 1 {
        NarGoalSystem::limitMemory(&mut nar.goalSystem, nar.t);
    }
    
    nar.t+=1; // increment time of NAR
}


/// is the term a op which can be called
fn checkIsCallableOp(nar: &ProcNar, term:&Term) -> bool {
    let opArgsAndNameOpt: Option<(Vec<Term>,String)> = decodeOp(&term);
    if !opArgsAndNameOpt.is_some() {
        return false; // isn't callable because it isn't 
    }
    let (_opArgs, opName) = opArgsAndNameOpt.unwrap();
    
    // TODO< check for {SELF} as first argument! >

    for i in &nar.ops {
        if &i.retName() == &opName {
            return true;
        }
    }
    false
}

// abstraction over term

/// return predicate of impl seq
pub fn retPred(term:&Term) -> Term {
    match term {
        Term::Stmt(Copula::PREDIMPL, _subj, pred) => {
            (**pred).clone()
        },
        _ => {
            panic!("expected pred impl!");
        }
    }
}

pub fn retImplSeqOp(term:&Term) -> Term {
    match term {
        Term::Stmt(Copula::PREDIMPL, subj, _pred) => {
            match &**subj {
                Term::Seq(seq) => {
                    *seq[1].clone()
                },
                _ => {panic!("expected seq!");}
            }
        },
        _ => {panic!("expected pred impl!");}
    }
}

pub fn retSeqOp(term:&Term) -> Term {
    match &term {
        Term::Seq(seq) => {
            *seq[1].clone()
        },
        _ => {panic!("expected seq!");}
    }
}

pub fn retSeqCond(term:&Term) -> Term {
    match term {
        Term::Stmt(Copula::PREDIMPL, subj, _pred) => {
            match &**subj {
                Term::Seq(seq) => {
                    *seq[0].clone()
                },
                _ => {panic!("expected seq!");}
            }
        },
        _ => {panic!("expected pred impl!");}
    }
}

/// decodes a operator into the arguments and name
/// returns None if the term can't be decoded
/// expects term to be <{(arg0 * arg1 * ...)} --> ^opname>
pub fn decodeOp(term:&Term) -> Option<(Vec<Term>,String)> {
    match term {
        Term::Stmt(Copula::INH, subj, pred) => {
            match &**pred {
                Term::Name(predName) => {
                    match &**subj {
                        Term::SetExt(subj2) if subj2.len() == 1 => {
                            match &*subj2[0] {
                                Term::Prod(args) if args.len() >= 1 => {
                                    return Some((args.iter().map(|v| (**v).clone()).collect(), predName.clone()));
                                },
                                _ => {return None;}
                            }
                        },
                        _ => {return None;}
                    }
                },
                _ => {return None;}
            }
        },
        _ => {return None;}
    }
}

/// encode op, used to get called from external code
pub fn encodeOp(args:&Vec<Term>, name:&String) -> Term {
    let argProd = Term::Prod(args.iter().map(|v| Box::new(v.clone())).collect()); // build product of arg
    Term::Stmt(Copula::INH, Box::new(Term::SetExt(vec![Box::new(argProd)])), Box::new(Term::Name(name.clone())))
}

/// event
/// string and evidence
/// (emulation of sentence and term)
#[derive(Clone)]
pub struct SimpleSentence {
    pub name:Term,
    pub evi:i64, // evidence id
    pub occT:i64, // occurcence time
}

/// helper to return indices of events with OPS
pub fn calcIdxsOfOps(nar: &ProcNar, trace:&Vec<Rc<SimpleSentence>>) -> Vec<i64> {
    let mut res = Vec::new();
    for idx in 0..trace.len() {
        if checkIsCallableOp(nar, &trace[idx].name) {
            res.push(idx as i64);
        }
    }
    res
}


/// helper to find the minimal index of the exponential interval table
pub fn findMinTableIdx(interval:i64, expIntervalsTable:&Vec<i64>) -> i64 {
    for idx in 1..expIntervalsTable.len() {
        if expIntervalsTable[idx] > interval {
            return idx as i64 - 1;
        }
    }
    return expIntervalsTable.len() as i64 - 1;
}

/// helper to debug evidence to console
pub fn debugEvidence(procNar: &ProcNar) {
    println!("EVIDENCE:");
    for iEvi in &mem_ret_evidence_all_nonunique(procNar) {
        let iEvi2 = iEvi.lock().unwrap();

        let implSeqAsStr = convTermToStr(&iEvi2.term);

        let evi:&Evidence = &iEvi2.evi.as_ref().unwrap();
        let (pos,cnt) = match evi {
            Evidence::CNT{pos,cnt} => {(pos,cnt)},
            _ => {panic!("expected CNT");}
        };

        let expDtAsStr = if iEvi2.expDt.is_some() {format!("+EXPDT{}", iEvi2.expDt.unwrap())} else {"".to_string()}; // convert expDt to string if it exists
        println!("{} {} {}/{}", &implSeqAsStr, expDtAsStr, pos, cnt);
    }
}

/// anticipated event
#[derive(Clone)]
pub struct AnticipationEvent {
    /// evidence
    pub evi:Arc<Mutex<SentenceDummy>>,
    /// deadline in absolute cycles
    pub deadline:i64,
}

/// trait for a op, all implementations implement a op
pub trait Op {
    fn retName(&self) -> String; /// return name of the op
    fn call(&self, args:&Vec<Term>);
}
