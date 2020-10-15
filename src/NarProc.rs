use rand::Rng;
use std::cell::RefCell;
use std::rc::Rc;

use crate::NarStamp::*;
use crate::NarSentence::*;
use crate::Tv::*;
use crate::Term::*;
use crate::TermApi::*;
use crate::NarGoalSystem;

// contains all necessary variables of a procedural NAR
pub struct ProcNar {
    pub cfgIntervalExpBase:f64, // base for the exponential intervals
    pub cfgIntervalMax:i64, // maximal interval time

    pub cfgPerceptWindow:i64, // perception window for current events
    pub cfgDescnThreshold:f64,

    pub cfgNMaxEvidence:i64, // maximal number of evidence
    pub cfgVerbosity:i64, // how verbose is the reasoner, mainly used for debugging

    pub evidence: Vec<Rc<RefCell<SentenceDummy>>>,
    
    pub trace: Vec<SimpleSentence>,
    pub anticipatedEvents: Vec<AnticipationEvent>,

    pub ops: Vec<Box<dyn Op>>, // all registered ops

    pub t:i64, // NAR time


    pub rng: rand::rngs::ThreadRng,

    //table with exponential intervals
    pub expIntervalsTable:Vec<i64>,

    pub goalSystem: NarGoalSystem::GoalSystem,
}

// init and set to default values
pub fn narInit() -> ProcNar {
    let mut nar = ProcNar {
        cfgIntervalExpBase: 1.5,
        cfgIntervalMax: 40,
        cfgPerceptWindow: 2,
        cfgDescnThreshold: 0.48,
        cfgNMaxEvidence: 5000,
        cfgVerbosity: 0, // be silent
        evidence: Vec::new(),
        trace: Vec::new(),
        anticipatedEvents: Vec::new(),
        ops: Vec::new(),
        t: 0,

        rng: rand::thread_rng(),

        expIntervalsTable: Vec::new(),

        goalSystem: NarGoalSystem::GoalSystem{entries:Vec::new(), nMaxEntries:20},
    };


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

// does one reasoner step
pub fn narStep0(nar:&mut ProcNar) {
    if nar.cfgVerbosity > 0 {println!("ae# = {}", nar.anticipatedEvents.len());}; // debug number of anticipated events
    
    // remove confirmed anticipations
    for perceptIdx in 0..nar.cfgPerceptWindow as usize {
        if nar.trace.len() > perceptIdx {
            let curEvent:&Term = &nar.trace[nar.trace.len()-1-perceptIdx].name;
            
            let mut newanticipatedEvents = Vec::new();
            for iDeadline in &nar.anticipatedEvents {
                let evi = (*iDeadline).evi.borrow();
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
                let mut mutEvi = (*iDeadlineViolated).evi.borrow_mut();
                
                match mutEvi.evi.as_ref().unwrap() {
                    Evidence::CNT{pos,cnt} => {
                        mutEvi.evi = Some(Evidence::CNT{pos:*pos,cnt:cnt+1}); // add negative evidence
                    },
                    _ => {panic!("expected CNT!");}
                }
            }
        }
        
        
        // TODO< refactor this as filtering >
        {
            let mut newanticipatedEvents = Vec::new();
            for iDeadline in &nar.anticipatedEvents {
                if iDeadline.deadline > nar.t {
                    newanticipatedEvents.push(iDeadline.clone());                        
                }
            }
            
            nar.anticipatedEvents = newanticipatedEvents;
        }
        

    }

    let cfgPerceptionSamplesPerStep = 4; // how ofter should event-FIFO get sampled for perception in cycle?
    
    if nar.trace.len() >= 3 { // add evidence
        for _sampleIt in 0..cfgPerceptionSamplesPerStep {
            // filter middle by ops and select random first event before that!
            let idxsOfOps:Vec<i64> = calcIdxsOfOps(&nar.trace);
            if idxsOfOps.len() > 0 { // there must be at least one op to sample


                let mut idx1 = 0;
                {
                    let idx1Idx = nar.rng.gen_range(0, idxsOfOps.len());
                    idx1 = idxsOfOps[idx1Idx] as usize;
                }
                
                if idx1 > 0 {
                    
                    let rng0:i64 = nar.rng.gen_range(0, 2);
                    
                    let idx0 = nar.rng.gen_range(0, idx1);
                    let mut idx2 = nar.trace.len()-1; // last event is last
                    

                    // is the name a op?
                    let checkIsOp=|name:&Term| {
                        for i in &nar.ops {
                            if checkEqTerm(&i.retName(), &name) {
                                return true;
                            }
                        }
                        false
                    };

                    // TODO< rewrite to logic which scans for the first op between idxLast and idx1, select random event as idx2 between these!
                    
                    // check if we can select previous event
                    {
                        let sel = nar.trace[nar.trace.len()-1-1].clone();
                        if rng0 == 1 && nar.trace.len()-1-1 > idx1 && !checkIsOp(&sel.name) {
                            idx2 = nar.trace.len()-1-1;
                        }
                    }


                    let mut idxs = vec![idx0,idx1,idx2];
                    idxs.sort();

                    
                    // middle must be op
                    if checkIsOp(&nar.trace[idxs[1]].name) {
                        // first and last must not be op
                        if
                            !checkIsOp(&nar.trace[idxs[0]].name)  &&
                            !checkIsOp(&nar.trace[idxs[2]].name) && 
                            nar.trace[idxs[0]].name != nar.trace[idxs[2]].name
                        {
                            
                            // found a potential sequence to be perceived
                            
                            let e0 = &nar.trace[idxs[0]];
                            let e1 = &nar.trace[idxs[1]];
                            let e2 = &nar.trace[idxs[2]];
                            
                            if nar.cfgVerbosity > 0 {println!("perceive ({},{})=/>{}", convTermToStr(&e0.name), convTermToStr(&e1.name), convTermToStr(&e2.name));};
                            
                            let dt:i64 = e2.occT - e1.occT;
                            // compute exponential delta time
                            let expDt:i64 = findMinTableIdx(dt, &nar.expIntervalsTable);
                            
                            let mut addEvidence:bool = true; // do we need to add new evidence?
                            
                            {
                                for iEERc in &nar.evidence {
                                    let iEE = &mut(*iEERc).borrow_mut();
                                    
                                    if !checkOverlap(&iEE.stamp, &newStamp(&vec!(e0.evi,e1.evi,e2.evi))) { // evidence must no overlap!
                                        if
                                            iEE.expDt.unwrap() >= expDt && // check for greater because we want to count evidence for longer intervals too, because longer ones are "included"
                                            
                                            // does impl seq match?
                                            checkEqTerm(&retSeqCond(&iEE.term), &e0.name) &&
                                            checkEqTerm(&retSeqOp(&iEE.term), &e1.name) &&
                                            checkEqTerm(&retPred(&iEE.term), &e2.name)
                                        {
                                            iEE.stamp = merge(&iEE.stamp, &newStamp(&vec!(e0.evi,e1.evi,e2.evi)));
                                            match iEE.evi.as_ref().unwrap() {
                                                Evidence::CNT{pos,cnt} => {
                                                    iEE.evi = Some(Evidence::CNT{pos:pos+1,cnt:cnt+1}); // bump positive counter
                                                },
                                                _ => {panic!("expected CNT!");}
                                            }
                                            
                                            if false {println!("dbg - REV")};
                                            
                                            addEvidence = false; // because we revised
                                        }                                
                                    }
        
                                }
                            }
                            
                            if addEvidence {
                                nar.evidence.push(Rc::new(RefCell::new(SentenceDummy {
                                    punct:EnumPunctation::JUGEMENT,
                                    t:None,
                                    stamp:newStamp(&vec!(e0.evi,e1.evi,e2.evi)),
                                    expDt:Some(expDt),
                                    term:Rc::new(s(Copula::PREDIMPL, &seq(&vec![e0.name.clone(), e1.name.clone()]), &e2.name.clone())), // (e0 &/ e1) =/> e2
                                    evi:Some(Evidence::CNT{pos:1,cnt:1})
                                })));
                            }
                        }
                        
                    }
                }
            }
        }
        
    }
    
}

pub fn narStep1(nar:&mut ProcNar) {    
    let mut pickedAction:Option<Term> = None;
    
    
    match &pickedAction {
        Some(_act) => {},
        None => {
            let mut bestEntry:(f64, Option<Rc<RefCell<NarGoalSystem::Entry>>>) = (0.0, None); // best entry from goal system to execute

            // * search if we can satisfy goal
            for perceptIdx in 0..nar.cfgPerceptWindow as usize {
                if nar.trace.len() > perceptIdx {

                    let checkedState:Term = nar.trace[nar.trace.len()-1-perceptIdx].name.clone();

                    // check if current state "leads" to action
                    // tuple is (exp, entity)
                    let thisEntry: (f64, Option<Rc<RefCell<NarGoalSystem::Entry>>>) = NarGoalSystem::selHighestExpGoalByState(&nar.goalSystem, &checkedState);

                    if thisEntry.0 > bestEntry.0 { // if exp is higher -> is a better choice
                        bestEntry = thisEntry;
                    }
                }
            }



            // * pick action and expected event to anticipations
            if bestEntry.0 > nar.cfgDescnThreshold && bestEntry.1.is_some() {
                let entity:Rc<RefCell<NarGoalSystem::Entry>> = bestEntry.1.unwrap();
                let pickedEvidenceOpt: &Option<Rc<RefCell<SentenceDummy>>> = &entity.borrow().evidence;
                
                if pickedEvidenceOpt.is_some() {
                    let pickedEvidence: Rc<RefCell<SentenceDummy>> = Rc::clone(&pickedEvidenceOpt.as_ref().unwrap());
                    { // info
                        let implSeqAsStr = convTermToStr(& (*pickedEvidence).borrow().term);
                        let act:Term = retSeqOp(& (*pickedEvidence).borrow().term);
                        let actAsStr:String = convTermToStr(&act);
                        let pickedExp:f64 = bestEntry.0;
                        println!("descnMaking: found best act = {}   implSeq={}    exp = {}", &actAsStr, &implSeqAsStr, pickedExp);
                    }
    
    
                    pickedAction = Some(retSeqOp(& (*pickedEvidence).borrow().term));
                    
                    // add anticipated event
                    let expIntervalIdx:i64 =
                        if (*pickedEvidence).borrow().expDt.is_some() {
                            (*pickedEvidence).borrow().expDt.unwrap()
                        }
                        else {0}; // else it needs a default interval
                    let interval:i64 = nar.expIntervalsTable[expIntervalIdx as usize];
                    let deadline:i64 = nar.t + interval; // compute real deadline by exponential interval
                    nar.anticipatedEvents.push(AnticipationEvent {
                        evi:Rc::clone(&pickedEvidence),
                        deadline:deadline,
                    });
                }
            }
        },
    }
    
    match &pickedAction {
        Some(_act) => {},
        None => {
            // TODO< better distribution >
            let p = nar.rng.gen_range(0, 18);
            if p < nar.ops.len() {
                let idx = p;
                pickedAction = Some(nar.ops[idx].retName());
            }
        }
    }
    
    
    match &pickedAction {
        Some(act) => {
            // scan for action
            for iOp in &nar.ops {
                if checkEqTerm(&iOp.retName(), &act) {
                    iOp.call(&vec![]); // call op
                    break;
                }
            }

            nar.trace.push(SimpleSentence {name:act.clone(),evi:nar.t,occT:nar.t});
        },
        None => {},
    }
    
    
    // limit trace (AIKR)
    if nar.trace.len() > 20 {
        nar.trace = (&nar.trace[nar.trace.len()-20..]).to_vec();
    }

    // limit evidence (AIKR)
    if nar.t % 101 == 1 && nar.evidence.len() > nar.cfgNMaxEvidence as usize {
        nar.evidence.sort_by(|a, b| calcExp(&retTv(&*b.borrow()).unwrap()).partial_cmp(&calcExp(&retTv(&*a.borrow()).unwrap())).unwrap()); // order by importance
        nar.evidence = nar.evidence[..nar.evidence.len().min(nar.cfgNMaxEvidence as usize)].to_vec(); // keep under AIKR
    }


    // give goal system resources
    if nar.t % 3 == 0 {
        let enGoalSystem = true; // DISABLED because we want to test it without deriving goals!
        if enGoalSystem {
            NarGoalSystem::sampleAndInference(&mut nar.goalSystem, nar.t, &nar.evidence, &mut nar.rng);
        }
    }

    if nar.t % 13 == 1 {
        NarGoalSystem::limitMemory(&mut nar.goalSystem, nar.t);
    }
    
    nar.t+=1; // increment time of NAR
}

// abstraction over term

// return predicate of impl seq
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

pub fn retSeqOp(term:&Term) -> Term {
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

// event
// string and evidence
// (emulation of sentence and term)
#[derive(Clone)]
pub struct SimpleSentence {
    pub name:Term,
    pub evi:i64, // evidence id
    pub occT:i64, // occurcence time
}

// helper to return indices of events with OPS
pub fn calcIdxsOfOps(trace:&Vec<SimpleSentence>) -> Vec<i64> {
    let mut res = Vec::new();
    for idx in 0..trace.len() {
        if convTermToStr(&trace[idx].name).chars().next().unwrap() == '^' { // is it a op?
            res.push(idx as i64);
        } 
    }
    
    return res;
}


// helper to find the minimal index of the exponential interval table
pub fn findMinTableIdx(interval:i64, expIntervalsTable:&Vec<i64>) -> i64 {
    for idx in 1..expIntervalsTable.len() {
        if expIntervalsTable[idx] > interval {
            return idx as i64 - 1;
        }
    }
    return expIntervalsTable.len() as i64 - 1;
}

// anticipated event
#[derive(Clone)]
pub struct AnticipationEvent {
    pub evi:Rc<RefCell<SentenceDummy>>, // evidence
    pub deadline:i64, // deadline in absolute cycles
}

// trait for a op, all implementations implement a op
pub trait Op {
    fn retName(&self) -> Term; // return name of the op
    fn call(&self, args:&Vec<Term>);
}