// module which implements basic NLP functionality
// 
// implementation spawns a "worker NAR" to process the preprocessed sentence

use std::cell::RefCell;
use std::rc::Rc;

use Nar::*;
use Term::*;
use TermApi::*;
use NarWorkingCycle::{Task2, debugCreditsOfTasks, QHandler};
use Tv::*;
use NarStamp::newStamp;
use NarSentence::{SentenceDummy, EnumPunctation, Evidence};

pub fn process(natural:&String)->Option<SentenceDummy> {
    let mut workerNar = createNar();


    let tokens: Vec<&str> = natural.split_whitespace().collect(); // split into tokens

    // convert tokens to inheritance representation and feed into NAR
    {
        let mut idx:usize = 0;
        while idx < tokens.len() {
            let idxAsStr = format!("{}", idx);
            
            if (tokens[idx] == "a" || tokens[idx] == "an") && idx+1 < tokens.len() {
                let token2nd = tokens[idx+1];
                let term:Term = s(Copula::INH, &Term::SetExt(vec![Box::new(p2(&Term::Name(token2nd.to_string()), &Term::Name(idxAsStr)))]), &Term::Name("a2".to_string()));
                inputT(&mut workerNar, &term, EnumPunctation::JUGEMENT, &Tv{f:1.0,c:0.998});

                idx+=2;
            }
            else if tokens[idx] == "is" {
                let term:Term = s(Copula::INH, &Term::SetExt(vec![Box::new(p2(&Term::Name("is".to_string()), &Term::Name(idxAsStr)))]), &Term::Name("rel2".to_string()));
                inputT(&mut workerNar, &term, EnumPunctation::JUGEMENT, &Tv{f:1.0,c:0.998});

                idx+=1;
            }
            else {
                println!("INFO - skipped token!");
                idx+=1;
            }
        }
    }

    // relation positive
    //ex:  a dog is a animal
    //ex:  an dog is an animal
    inputN(&mut workerNar, &"<(<{($1*0)} --> a2>&&<{(is*2)} --> rel2>&&<{($2*3)} --> a2>) ==> <{($1*$2)} --> isRel>>. {1.0 0.998}".to_string());

    // ask question directly
    let mut answerHandler:NlpAnswerHandler = NlpAnswerHandler{answer:None};
    let answerHandlerRef = Rc::new(RefCell::new(answerHandler));
    let rc2 = Rc::clone(&answerHandlerRef);
    {
        let sentence = SentenceDummy {
            term:Rc::new( s(Copula::INH, &Term::QVar("0".to_string()), &Term::Name("isRel".to_string())) ),
            t:None, // time of occurence 
            punct:EnumPunctation::QUESTION,
            stamp:newStamp(&vec![999]),
            evi:Evidence::TV(Tv{f:1.0,c:0.9}),
            expDt:None
        };

        workerNar.mem.questionTasks.push(Box::new(Task2 {
            sentence:sentence,
            handler:Some(answerHandlerRef),
            bestAnswerExp:0.0, // because has no answer yet
            prio:1.0,
        }));
    }

    for iCycle_ in 0..200 { // give worker NAR time to reason
        cycle(&mut workerNar);
    }

    // for debugging
    debugCreditsOfTasks(&workerNar.mem);


    let res = rc2.borrow_mut().answer.clone();
    res // return answer of question
}

struct NlpAnswerHandler {
    answer: Option<SentenceDummy>, // holds the answer if it was found
}

impl QHandler for NlpAnswerHandler {
    fn answer(&mut self, question:&Term, answer:&SentenceDummy) {
        self.answer = Some(answer.clone());
    }
}