use Tv::*;
use Term::*;
use NarseseParser::parseNarsese;
use NarSentence::*;
use NarStamp::*;
use NarWorkingCycle::*;

pub struct Nar {
    pub mem:Mem2, // actual (declarative) memory
}

pub fn createNar() -> Nar {
    Nar{mem:createMem2()}
}

pub fn inputT(nar:&mut Nar, term:&Term, punct:EnumPunctation, tv:&Tv) {
    println!("[v] input {}", convTermToStr(term));

    let stamp = newStamp(&vec![nar.mem.stampIdCounter]);
    nar.mem.stampIdCounter+=1;
    let sentence = newEternalSentenceByTv(&term,punct,&tv,stamp);

    memAddTask(&mut nar.mem, &sentence, true);
}

// input narsese
pub fn inputN(nar:&mut Nar, narsese:&String) {
    match parseNarsese(narsese) {
        Some((term, tv, punct)) => {
            inputT(nar, &term, punct, &tv);
        },
        None => {
            // TODO< handle error correctly by returning a error >
            println!("ERR - couldn't parse!");
        }
    }
}

pub fn cycle(nar:&mut Nar) {
    reasonCycle(&mut nar.mem);
}