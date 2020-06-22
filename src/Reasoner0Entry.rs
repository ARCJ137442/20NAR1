#![allow(non_snake_case)]
#![allow(dead_code)]

use rand::Rng;
use std::cell::RefCell;
use std::rc::Rc;

use ::Nars;
use ::AeraishPerceptionComp;
use ::AeraishPerceptionComp::{PerceptItem};
use ::expRepresent0;

pub fn reasoner0Entry() {
    let mut t:i64 = 0; // discrete time
    let mut maxT:Option<i64> = Some(700);


    let mut nar:Nars::Nar = Nars::narInit();
    
    let mut rng = rand::thread_rng();


    
    let mut envPong = RefCell::new(PongEnv {
        batVelX:0.0,
        batX:7.0,
        ballX:3.0,
        score:0.0,
    });
    let envPongRc = Rc::new(envPong);

    nar.ops.push(Box::new( OpPong {
        env: Rc::clone(&envPongRc),
        opDir: 1.0,
        selfName: "^L".to_string(),
    }));

    nar.ops.push(Box::new( OpPong {
        env: Rc::clone(&envPongRc),
        opDir: -1.0,
        selfName: "^R".to_string(),
    }));
    
    // current perception of the NAR"channel"
    let mut currentPerceived : Vec< PerceptItem::< ClsnObj > > = Vec::new();
    

    
    loop { // reasoner/modification mainloop


        { // calc score
            let distX:f64 = ((*envPongRc).borrow().batX - (*envPongRc).borrow().ballX).abs();
            if distX <= 1.1 {
                (*envPongRc).borrow_mut().score+=1.0; // add score because bat is in the center
            }
        }
    
        // select option to focus on
        // we hardcoded it so it always returns the first option, which is the only one
        let selFocusItem:usize = pickByMass(&[1.0, 0.8], rng.gen_range(0.0, 1.0));
        
        if selFocusItem == 0 { // do we want to spend the time in the NARS reasoning?
            Nars::narStep0(&mut nar);

            {
                // build relations between perceived proto-"objects"
                // NOTE< hardcoded for line following ! >
                
                if currentPerceived.len() >= 2 { // we need to perceive at least two proto-objects
                    let mut a:ClsnObj = currentPerceived[0].dat.clone();
                    let mut b:ClsnObj = currentPerceived[1].dat.clone();
                    
                    if a.objCat != b.objCat { // categories must be different to allow forming of relationship
                        // sort by class, because it reduces the amount of concepts
                        if a.objCat > b.objCat {
                            let t = a;
                            a = b;
                            b = t;
                        }

                        let diffX:f64 = a.posX - b.posX;
                        let diffY:f64 = a.posY - b.posY;
                        
                        if true {
                            if diffX > 1.0 {
                                nar.trace.push(Nars::SimpleSentence {name:format!("{}-{}-x{}", a.objCat, b.objCat, "r"),evi:nar.t,occT:nar.t});
                            }
                            else if diffX < -1.0 {
                                nar.trace.push(Nars::SimpleSentence {name:format!("{}-{}-x{}", a.objCat, b.objCat, "l"),evi:nar.t,occT:nar.t});
                            }
                            else {
                                nar.trace.push(Nars::SimpleSentence {name:format!("{}-{}-x{}", a.objCat, b.objCat, "c"),evi:nar.t,occT:nar.t});
                            }
                        }


                        if true { // do we want to handle y events too?
                            if diffY > 1.0 {
                                nar.trace.push(Nars::SimpleSentence {name:format!("{}-{}-y{}", a.objCat, b.objCat, "r"),evi:nar.t,occT:nar.t});
                            }
                            else if diffY < -1.0 {
                                nar.trace.push(Nars::SimpleSentence {name:format!("{}-{}-y{}", a.objCat, b.objCat, "l"),evi:nar.t,occT:nar.t});
                            }
                            else {
                                nar.trace.push(Nars::SimpleSentence {name:format!("{}-{}-y{}", a.objCat, b.objCat, "c"),evi:nar.t,occT:nar.t});
                            }
                        }
                    }
                }
            }
    
            if nar.trace.len() > 0 {
                println!("{} {}", nar.trace[nar.trace.len()-1].name, (*envPongRc).borrow().ballX - (*envPongRc).borrow().batX);
            }
            
            Nars::narStep1(&mut nar);
            
            let mut envPong = (*envPongRc).borrow_mut();
            envPong.batX += envPong.batVelX; //envPongRc.get().batVelX;
            
            // limit bat
            if envPong.batX < 0.0 {
                envPong.batX = 0.0;
            }
            if envPong.batX > 10.0 {
                envPong.batX = 10.0;
            }
        }
        else if selFocusItem == 1 { // perceive outside sensor
            // TODO< call into real perception here to perceive environment >

            let mut perceived : Vec< PerceptItem::< ClsnObj > > = Vec::new();
            { // fill with dummy percepts for testing
                println!("[d] percept: fill with dummy perceptions");

                perceived.push(PerceptItem::<ClsnObj> {
                    dat:ClsnObj{
                        objCat:1, // object category, found with some kind of classifier
                        conf:0.98, // classification confidence

                        posX:(*envPongRc).borrow().ballX,
                        posY:0.1,
                    }, // actual data
                    salience:0.5,
                    novelity:0.01,
                });

                perceived.push(PerceptItem::<ClsnObj> {
                    dat:ClsnObj{
                        objCat:0, // object category, found with some kind of classifier
                        conf:0.98, // classification confidence

                        posX:(*envPongRc).borrow().batX,
                        posY:0.1,
                    }, // actual data
                    salience:0.5,
                    novelity:0.01,
                });

            }

            // TODO< call into process for attention modulation to manipulate PerceptItem.salience >

            // sort by PerceptItem.salience
            perceived.sort_by(|a, b| b.salience.partial_cmp(&a.salience).unwrap());

            // filter with simple attention based on limited throughput
            perceived = AeraishPerceptionComp::limit(&perceived, 10);

            // set as global perceived of this (NAR)"channel"
            currentPerceived = perceived;
        }
        // TODO< add AERA planning reasoning case >
        // TODO< add self control things case >
        
        
        // logic to decide when to break up
        if maxT.is_some() {
            if t >= maxT.unwrap() {
                break; // exit this loop
            }
        }
        t+=1;
    }



    
    // debug all evidence of NAR
    println!("");
    println!("EVIDENCE:");
    for iEvi in &nar.evidence {
        let implSeqAsStr = format!("({},{})=/>{}",(*iEvi).borrow().seqCond,(*iEvi).borrow().seqOp,(*iEvi).borrow().pred);
        println!("{} +EXPDT{} {}/{}", &implSeqAsStr, (*iEvi).borrow().expDt, (*iEvi).borrow().eviPos, (*iEvi).borrow().eviCnt);
    }
    
    { // print environment score
        println!("[i] env score = {}", (*envPongRc).borrow().score);
    }


    println!("[d] reasoner: DONE!");
}

// pick a option by mass
// /param selVal value for selection in range [0.0;1.0]
pub fn pickByMass(massArr:&[f64], selVal:f64) -> usize {
    let sum:f64 = massArr.iter().sum();
    let mut rem = selVal*sum;
    let mut idx = 0;
    for iv in massArr {
        if rem < *iv {
            return idx;
        }
        rem -= iv;
        idx+=1;
    }
    
    massArr.len()-1 // sel last
}


// classification of a object in the "glocal" perceptive field
#[derive(Clone)]
pub struct ClsnObj {
    pub objCat:i64, // object category, found with some kind of classifier
    pub conf:f64, // classification confidence

    pub posX:f64, // position in perceptive field
    pub posY:f64, // position in perceptive field
}




// pong environment
#[derive(Copy, Clone)]
pub struct PongEnv {
    pub batVelX:f64,
    pub batX:f64,
    pub ballX:f64,
    pub score:f64,
}



// ops for pong environment
pub struct OpPong {
    pub env: Rc<RefCell<PongEnv>>, // points at environment
    pub opDir: f64, // direction which is set when this op is called
    pub selfName: String, // name of this op
}


// Implement the `Animal` trait for `Sheep`.
impl Nars::Op for OpPong {
    fn retName(&self) -> String {
        self.selfName.clone()
    }
    fn call(&self, args:&Vec<String>) {
        (*self.env).borrow_mut().batVelX = self.opDir;
        println!("CALL {}", self.selfName);
    }
}