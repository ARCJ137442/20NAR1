use std::cell::RefCell;
use std::rc::Rc;
use std::sync::{Arc, Mutex};

use crate::Tv::*;
use crate::Term::*;
use crate::NarseseParser::parseNarsese;
use crate::NarSentence::*;
use crate::NarStamp::*;
use crate::NarWorkingCycle::*;
use crate::NarProc;
use crate::NarGoalSystem;

pub struct Nar {
    pub procNar:NarProc::ProcNar, // procedural NAR

    pub mem:Mem2, // actual (declarative) memory

    pub cfgVerbosityInput:i32, // verbosity of input
}

pub fn createNar() -> Nar {
    Nar{
        procNar:NarProc::narInit(),
        mem:createMem2(),
        cfgVerbosityInput:1, // enable verbose input by default
    }
}

// for eternal
pub fn inputT(nar:&mut Nar, term:&Term, punct:EnumPunctation, tv:&Tv) {
    inputT2(nar, term, punct, tv, false);
}

pub fn inputT2(nar:&mut Nar, term:&Term, punct:EnumPunctation, tv:&Tv, isEvent:bool) {    
    let stamp = newStamp(&vec![nar.mem.stampIdCounter]);
    nar.mem.stampIdCounter+=1;
    let mut sentence = newEternalSentenceByTv(&term,punct,&tv,stamp);

    if nar.cfgVerbosityInput >= 1 {
        println!("[v] input {}", convSentenceTermPunctToStr(&sentence, true));
    }

    if isEvent {
        if punct == EnumPunctation::GOAL {
            // add to goals
            NarGoalSystem::addEntry(&mut nar.procNar.goalSystem, nar.procNar.t, Arc::new(sentence), None, 0);
        }
        else {
            // add event
            nar.procNar.trace.push(NarProc::SimpleSentence {name:term.clone(),evi:nar.procNar.t,occT:nar.procNar.t});
        }

        return;
    }

    // compute if the term is a temporal term
    let isTemporal = match term {
        Term::Stmt(Copula::PREDIMPL, _, _) => {true},
        _ => {false}
    };

    if isTemporal {
        // add to temporal knowledge
        sentence.evi = Some(Evidence::CNT{pos:1,cnt:1}); // we need to transcribe TV
                                                         // TODO< transcribe TV in a better way, we need to approximate freq and conf! >
        
        NarProc::mem_add_evidence(&mut nar.procNar, &sentence);
    }
    else {
        if punct == EnumPunctation::GOAL {
            println!("ERR : eternal goals are not supported!");
        }
        else {
            memAddTask(&mut nar.mem, &sentence, true);
        }
    }
}

// input narsese
// return if narsese was parsed and had no error
pub fn inputN(nar:&mut Nar, narsese:&String) -> bool {
    match parseNarsese(narsese) {
        Some((term, tv, punct, isEvent)) => {
            inputT2(nar, &term, punct, &tv, isEvent);
            true
        },
        None => {
            // TODO< handle error correctly by returning a error >
            println!("ERR - couldn't parse!");
            false
        }
    }
}

pub fn cycle(nar:&mut Nar) {
    reasonCycle(&mut nar.mem);
}
