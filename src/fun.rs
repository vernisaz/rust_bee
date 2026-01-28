use std::{collections::HashMap,
        cell::RefCell,
        rc::{Rc, Weak},
        io::{self, Write},
        path::Path,
        time::SystemTime,
        fs::{self, OpenOptions, File, remove_file, create_dir_all, remove_dir, remove_dir_all, copy, rename},
        env,
        process::{Command, Stdio},
        ops::Deref,
        path::{MAIN_SEPARATOR_STR,PathBuf,MAIN_SEPARATOR},
        fmt, error::Error};
use simcolor::{Colorized};
#[cfg(unix)]
use std::os::unix::fs::OpenOptionsExt;
use crate::lex::{process_template_value, VarVal, VarType};
use crate::log::Log;
//use http::{Request,Response};
use crate::util::{self,format_time,has_root};
use crate::get_property;
use crate::ver;
use crate::CWD;

pub const PREV_VAL : &str = "~~";

// type FunCall = fn(Vec<Lexem>) -> Option<()>;

type CalcResult = Result<f64, CalcErr>;

// TODO move to the lex module
#[derive(Debug, PartialEq,Clone, Default)]
pub enum BlockType { // rename to BlockKind
    Main,
    Target,
    Dependency,
    If,
    #[default]
    Scope,
    Eq,
    Function,
    Neq,
    Then,
    Else,
    Or,
    And,
    Not,
    For,
    While,
    Case,
    Choice
}

#[derive(/*Debug,*/ Default)]
pub struct GenBlock {
    pub name: Option<String>,
    pub block_type: BlockType,
    pub dir:Option<String>, // working directory
    pub flex: Option<String>,
    pub out: Option<String>,
    pub vars: HashMap<String, VarVal>,
    pub params: Vec<String>, // for a function, perhsps should be a tuple as parameter(value,type)
    pub children: Vec<GenBlockTup>,
    pub deps: Vec<GenBlockTup>,
    //pub parent: Option<WeakGenBlock>,
    pub parent: Option<GenBlockTup>,
    pub script_line: u32
}

#[derive(PartialEq, Debug, Copy, Clone, Default)]
enum CalcState {
    #[default]
    Start,
    Oper,
    Var,
    Val,
    Exp,
    Skip
}

#[derive(Debug, Copy, Clone, Default)]
enum Op {
    #[default]
    Plus,
    Minus,
    Div,
    Mul
}

#[derive(Debug)]
enum CalcErrCause {
    NaN,
    NVar,
    DZero,
    InvOp,
    CntPar,
    InvPar
}

type CalcErr = (CalcErrCause, usize);

#[derive(Clone, Debug)]
pub struct GenBlockTup(pub Rc<RefCell<GenBlock>>);

#[allow(dead_code)]
pub type WeakGenBlock = Weak<RefCell<GenBlock>>; // use Rc::new_cyclic

impl Deref for GenBlockTup {
    type Target = Rc<RefCell<GenBlock>>;
    fn deref(&self) -> &Self::Target {
       &self.0
    }
}

impl fmt::Debug for GenBlock {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("GenBlock")
            .field("name", &self.name)
            .field("block_type", &self.block_type)
            .field("dir", &self.dir)
            .field("flex", &self.flex)
            .field("out", &self.out)
            .field("vars", &self.vars)
            .field("params", &self.params)
            .field("children", &self.children)
            .field("parent and other", &format_args!("<omitted to prevent an infinite loop>"))
            .finish()
    }
}

impl GenBlock {
    pub fn new (block_type: BlockType) -> GenBlock {
        GenBlock {
            block_type,
            ..Default::default()
        }
    }

    pub fn search_up(&self, name: &str) -> Option<VarVal> {
        let var = self.vars.get(name);
        match var {
            None => {
                match &self.parent {
                    None => None,
                    Some(parent) => parent.search_up(name)
                }
            }
            Some(var) =>  Some(var.clone())
        }
    }

    pub fn prev_or_search_up(&self, name: &String, prev: &Option<VarVal>) -> Option<VarVal> {
        if PREV_VAL == name {
            prev.clone()
        } else {
            self.search_up(name)
        }
    }
    
    pub fn script_path(&self) -> String {
        self.search_up(crate::SCRIPT).map(|v| v.value).unwrap_or_default()
    }
}

impl GenBlockTup {
    pub fn add(&self, node: GenBlockTup) -> GenBlockTup {
        //(node.0).borrow_mut().parent = Some(Rc::downgrade(&self.0));
        (*node).borrow_mut().parent = Some(GenBlockTup(Rc::clone(&self.0)));
        // try link_opt =&mut RefCell::get_mut(Rc::get_mut(linselfk).unwrap()).parent;
        let result = GenBlockTup(Rc::clone(&node.0));
        self.0.borrow_mut().children.push(node);
        result
    }

    pub fn add_dep(&self, node: GenBlockTup) -> GenBlockTup {
       // (node.0).borrow_mut().parent = Some(Rc::downgrade(&self.0));
       (node.0).borrow_mut().parent = Some(GenBlockTup(Rc::clone(&self.0)));
        let result = GenBlockTup(Rc::clone(&node.0));
        self.0.borrow_mut().deps.push(node);
        result
    }

    pub fn add_var(&self, name: String, val: VarVal) -> Option<VarVal> {
       // println!("borrow_mut()"        );
        //let mut current_bl = self.0.borrow_mut();
        self.0.borrow_mut().vars.insert(name, val)
    }
    
    pub fn remove_var(&self, name: &String) -> Option<VarVal> {
        self.0.borrow_mut().vars.remove(name)
    }

    pub fn search_up(&self, name: &str) -> Option<VarVal> {
        let  current_bl = self.0.borrow();
       // let mut current_vars = current_bl.vars;
        let var = current_bl.vars.get(name);
        match var {
            None => {
                match &current_bl.parent {
                    None => None,
                    Some(parent) => parent.search_up(name)
                }
            },
            Some(var) => Some(var.clone())
        }
    }

    pub fn search_up_block(&self, name: &String) -> Option<GenBlockTup> {
        let mut current_bl = self.clone();
        loop {
            let current_bare = current_bl.borrow();
            // println!{"checking {:?} for {name}", parent_bare.block_type}
             if current_bare.vars.contains_key(name) {
                break Some(current_bl.clone())
             } else {
                let parent = current_bare.parent.clone()?;
                drop(current_bare);
                current_bl = parent
             }
        }
    }

    pub fn prev_or_search_up(&self, name: &String, prev: &Option<VarVal>) -> Option<VarVal> {
        if PREV_VAL == name {
            prev.clone()
        } else {
            self.search_up(name)
        }
    }

    pub fn parent(& self) -> Option<GenBlockTup> {
        //println!("borrowed {:?} - {:?}", self.0.borrow().name, self.0.borrow().block_type);
        self.0.borrow().parent.clone()
    }

    pub fn eval_dep(&self, log: &Log, prev_res: &Option<VarVal>) -> bool {
        let dep = self.0.borrow();
        let len = dep.children.len();
        //println!{"depb {dep:?}"}
        if len == 0 {
            return dep.out.is_none() || "true" == dep.out.as_ref().unwrap()
        } else if len == 1 {
            let dep_task = &dep.children[0];
            let dep_block = dep_task.0.borrow();
            match dep_block.block_type {
                BlockType::Function => {
                    match dep_block.name.clone().unwrap().as_str() {
                        "target" => {
                            log.debug(&format!("evaluating target: {}", dep_block.params[0]));
                            let target = self.get_target(&dep_block.params[0]);
                            match target {
                                Some(target) => {
                                    //let target_bor = target.0.borrow_mut();
                                    return exec_target(log, & target)
                                },
                                _ => log.warning(&format!("Target {} not found and ignored", dep_block.params[0])),
                            }
                        },
                        "anynewer" => {
                            log.debug(&format!("evaluating anynewer: {}", dep_block.params.len()));
                            let p1 = process_template_value(log, &dep_block.params[0], &dep, prev_res);
                            let p2 = process_template_value(log, &dep_block.params[1], &dep, prev_res);
                            log.debug(&format!("anynewer dep parameters: {}, {}", p1, p2));
                            // TODO get cwd here 
                            if self.search_up(CWD) .is_none() {
                                log.warning("No CWD set")
                            }
                            return exec_anynewer(self, &p1, &p2)
                        },
                        _ => todo!("function: {:?}", dep_block.name)
                    } 
                },
                BlockType::Eq => {
                    let len = dep_block.children.len();
                    if len  > 0 {
                        let p1 = &dep_block.children[0];
                        let p1_block = p1.0.borrow();
                        let r1 : Option<VarVal> =
                         match p1_block.block_type {
                             BlockType::Function => {
                                p1.exec_fun(log, &p1_block, prev_res)
                             },
                             _ => { todo!("block: {:?}", p1_block.block_type)}
                        };
                        let r2 : Option<VarVal> =
                            if len == 2 {
                                let p2 = &dep_block.children[1];
                                let p2_block = p2.0.borrow();
                                  match p2_block.block_type {
                                    BlockType::Function => {
                                        p2.exec_fun(log, &p2_block, prev_res)
                                    },
                                    _ => { todo!("block: {:?}", p2_block.block_type);
                                    }
                                }
                            } else {
                                None
                            };
                        //println!("comparing: {:?} and {:?}", r1, r2);
                        log.debug(&format!("comparing: {:?} and {:?}", r1, r2));
                        match r1 {
                            None => {
                                match r2 {
                                    None => return true,
                                    _ => return false
                                }
                            },
                            Some(r1) => {
                                match r2 {
                                    None => return false,
                                    Some(r2) => return r1.value == r2.value
                                }
                            }
                        }
                       // return r1 == r2;
                    }
                },
                BlockType::Or => {
                    //let len = dep_block.children.len();
                    log.debug(&format!("orig {} children", &dep_block.children.len()));
                    for child in &dep_block.children {
                       if child.exec(log, prev_res).unwrap_or(VarVal::from_bool(false)).is_true() {
                            return true
                       }
                    }
                    return false
                },
                _ => todo!("the operation {:?} isn't supported yet at {}:{}: ", dep_block.block_type, dep.script_path(), dep.script_line)
            }
        } else {
            log.error(&format!("{} children not supported in a dependency at {}:{}: ", len, dep.script_path(), &dep.script_line))
        }
        false
    }

    pub fn get_top_block(& self) -> GenBlockTup {
        let mut curr =self.clone();
        loop {
            let parent = curr.parent();
            match parent {
                None => return curr.clone(),
                Some(parent) => curr = parent
            }
        }
    }

    // TODO consider returning ref
    pub fn get_target(&self, name: &String) -> Option<GenBlockTup> {
        let top_block = &self.get_top_block();
        let naked_block = top_block.borrow();
        for ch in &naked_block.children {
            let ch_block = ch.borrow();
            if ch_block.block_type == BlockType::Target 
                && let Some(blk_name) = &ch_block.name && blk_name == name {
                return  Some(ch.clone())
            }
        }
        None
    }

    pub fn exec(&self, log: &Log, prev_res: &Option<VarVal>) -> Option<VarVal> {
        let block_type = &self.borrow().block_type.clone();
        log.debug(&format!("processing block of {:?}", block_type));
        match  block_type {
            BlockType::Scope | BlockType::Then | BlockType::Else | BlockType::Choice => {
                let mut res = prev_res.clone();
                let children = &self.0.borrow().children.clone();
                for child in children {
                    res = child.exec(log, &res)
                } 
                res 
            },
            BlockType::Main => {
                let mut res = None;
                let children = &self.0.borrow().children.clone();
                for child in children {
                    let child_nak = child.borrow();
                    if child_nak. block_type == BlockType::Function && child_nak.name == Some("include".into()) 
                    || child_nak. block_type == BlockType::Target {
                        continue
                    }
                    drop(child_nak);
                    res = child.exec(log, &res);
                } 
                res
            }
            BlockType::If => {
                let naked_block = self.borrow();
                let children = &naked_block.children;
                let mut res = children[0].exec(log, prev_res);
                log.debug(&format!("if cond evaluated as {:?}", res));
                if res.as_ref().unwrap_or(&VarVal::from_bool(false)).is_true() {
                    if children[1].borrow().block_type == BlockType::Then { 
                      res = children[1].exec(log, prev_res)
                    }
                } else if children.len() == 2 &&  children[1].borrow().block_type == BlockType::Else {
                     res = children[1].exec(log, prev_res)
                } else if children.len() == 3 &&  children[2].borrow().block_type == BlockType::Else {
                    res = children[2].exec(log, prev_res)
                }
                if children.len() > 3 {
                    log.error(&format!("Unexpected block(s) {} at {}:{}: ", children.len(), naked_block.script_path(), &naked_block.script_line))
                }
                res
            },
            BlockType::Function => {
                let naked_block = self.borrow();
                log.debug(&format!("function; {:?}", naked_block.name));
                for param in &naked_block.params {
                    log.debug(&format!("parameter; {}", param))
                } 
                self.exec_fun(log, &naked_block, prev_res)
            },
            BlockType::For => {
                let mut res = prev_res.clone();
                let mut range = Vec::new();
                let naked_block = self.borrow();
                let Some(name) = &naked_block.name.clone() else {
                    log.error(&format!("A 'for' variable isn't specified at {}:{}: ", naked_block.script_path(), naked_block.script_line));
                    return None
                };
                // dir as range
                let Some(range_as_opt) = &naked_block.dir.clone() else {
                    log.error(&format!("A 'for' range isn't specified at {}:{}: ", naked_block.script_path(), naked_block.script_line));
                    return None
                };
                if range_as_opt.is_empty() {
                    log.error(&format!("A 'for' range isn't specified at {}:{}: ", naked_block.script_path(), naked_block.script_line));
                    return None
                }
                let range_as_var = self.prev_or_search_up(range_as_opt, prev_res);
                
                if let Some(range_as_val) = range_as_var {
                    if range_as_val.val_type == VarType::Array {
                        for var_el in range_as_val.values {
                            range.push(var_el.clone())
                        }
                    } else {
                        let Some(sep_can) = &naked_block.flex.clone() else {
                            log.error(&format!("A 'for' values separator isn't specified at {}:{}: ", naked_block.script_path(), naked_block.script_line));
                            return None
                        };
                        let sep_var = self.search_up(sep_can);
                        let sep_val = match sep_var {
                            None => sep_can,
                            Some(val) => &val.value.clone(),
                        };
                        // expand template variables
                        let range_as_val = process_template_value(log, &range_as_val.value, &naked_block, prev_res);
                        let values = range_as_val.split(sep_val);
                        for var_el in values {
                            range.push(var_el.to_string())
                        }
                    }
                } else {
                    let Some(sep_can) = &naked_block.flex.clone() else {
                            log.error(&format!("A 'for' values separator isn't specified at {}", naked_block.script_line));
                            return None
                    };
                    let sep_var = self.search_up(sep_can);
                    let sep_val = match sep_var {
                        None => sep_can,
                        Some(val) => &val.value.clone(),
                        };
                    let values = range_as_opt.split(sep_val);
                    for var_el in values {
                        range.push(var_el.to_string())
                    }
                }
                let children = &naked_block.children.clone();
                drop(naked_block);
                for (index, element) in range.iter().enumerate() {
                    let var_element = VarVal{val_type: VarType::Generic, value: element.clone(), values: Vec::new()};
                    let var_index = VarVal{val_type: VarType::Number, value: format!("{}", index), values: Vec::new()};
                    {
                        let mut naked_block = self.0.borrow_mut();
                        naked_block.vars.insert(name.to_string(), var_element);
                        naked_block.vars.insert("~index~".to_string(), var_index);
                    }
                    
                    for child in children {
                        res = child.exec(log, &res)
                    } 
                }     
                res    
            },
            // none below operations assume an assign as part, therefore no clone children
            BlockType::Or => {
                let naked_block = self.0.borrow();
                let children = &naked_block.children;
                for child in children {
                    if child.exec(log, prev_res).unwrap_or_default().is_true() {
                        return Some(VarVal::from_bool(true))
                    }
                }
                Some(VarVal::from_bool(false))
            },
            BlockType::And => {
                let naked_block = self.0.borrow();
                let children = &naked_block.children;
                for child in children {
                    let res = child.exec(log, prev_res).unwrap_or_default().is_true();
                    if !res {
                        return Some(VarVal::from_bool(false))
                   }
                }
                Some(VarVal::from_bool(true))
            },
            BlockType::Not => {
                let naked_block = self.0.borrow();
                let children = &naked_block.children;
                if children.len() > 1 {
                    log.error(&format!("Unexpected block(s) {} at {}:{}: ", children.len(), naked_block.script_path(), &naked_block.script_line))
                }
                Some(VarVal::from_bool(! children[0].exec(log, prev_res).unwrap_or_default().is_true()))
            },
            BlockType::Eq => {
                let naked_block = self.0.borrow();
                let children = &naked_block.children;
                let len = children.len();
                if len < 1 {
                    log.error(&format!("At least one argument has to be specified in eq at {}:{}: ", naked_block.script_path(), &naked_block.script_line))
                }

                let mut before_res = children[0].exec(log, prev_res);
                for child in children.iter().take(len).skip(1) {
                    let res = child.exec(log, prev_res);
                    match res {
                        None => {
                            if let Some(_before_some) = before_res { return Some(VarVal::from_bool(false)) }
                        },
                        Some(ref res_some) => {
                            match before_res {
                                None => return Some(VarVal::from_bool(false)),
                                Some(ref before_some) => if before_some.value != res_some.value {
                                    return Some(VarVal::from_bool(false))
                                }
                            }
                        }
                    }
                    
                    before_res =  res
                }
                Some(VarVal::from_bool(before_res.is_none()))
            },
            BlockType::Neq => {
                let naked_block = self.0.borrow();
                let children = &naked_block.children;
                let len = children.len();
                if len < 1 {
                    log.error(&format!("At least one argument has to be specified in neq at {}:{}: ", naked_block.script_path(), &naked_block.script_line))
                }
                let first = children[0].exec(log, prev_res);
                if len > 1 {
                    for child in children.iter().take(len).skip(1)  {
                        let current = child.exec(log, prev_res);
                        match (&current, &first) {
                            (Some(current), Some(first)) => if current.value == first.value {continue},
                            (None, None) => continue,
                            _ => (),
                        }
                        return Some(VarVal::from_bool(true))
                    }
                    return Some(VarVal::from_bool(false))
                }
                
                Some(VarVal::from_bool(first.is_some()))
            },
            BlockType::While => {
                let mut res = prev_res.clone();

                let naked_block = self.0.borrow();
                let control = naked_block.name.clone().unwrap();
                let control_var = self.search_up(&control);
                if control_var.is_none() {
                    log.error(&format!("No 'while' control variable {} at {}", &control, &naked_block.script_line));
                    return None
                }
                let children = naked_block.children.clone();
                //drop(naked_block);
                let mut val = control_var.unwrap().is_true();
                while val {
                    for child in &children {
                        res = child.exec(log, &res)
                    } 
                    let control_var = self.search_up(&control); // will be always found
                    val = control_var.unwrap().is_true()
                }
                res
            },
            BlockType::Case => {
                let mut res = prev_res.clone();

                let naked_block = self.borrow();
                let control = naked_block.name.as_ref().unwrap().to_owned();
                if let Some(var) = self.search_up(&control) {
                    let children = &naked_block.children.clone();
                    let mut chosen = false;
                    let var = match var.val_type {
                        VarType::Environment  => {
                            match env::var(&var.value) {
                                Ok(val) => val,
                                Err(_e) => var.value 
                            }
                        },
                        VarType::Property => {
                            if let Some(val) = get_property(&var.value) {
                                val
                            } else {
                                var.value
                            }
                        },
                        _ => var.value
                    };
                    for child in children {
                        //println!{"case {:?} / {}", child.borrow().name, var}
                       // TODO there is no check that else is the final choice, perhaps add it in the future
                        if child.borrow().block_type == BlockType::Else  {
                            if !chosen {
                                res = child.exec(log, &res)
                            }
                            break
                        }
                        let choice = <Option<String> as Clone>::clone(&child.borrow().name).unwrap_or("".into());
                        let patterns = choice.split("|");
                        for pattern in patterns {
                            let trimmed = pattern.trim();
                            if var == trimmed {
                                chosen = true;
                                res = child.exec(log, &res);
                                break
                            }
                        }
                    }
                } else {
                    log.error(&format!("No 'case' variable {} at {}:{}: ", &control, naked_block.script_path(), &naked_block.script_line))
                }
                res
            },
            _ => {
                let naked_block = self.borrow();
                todo!("not implemented block: {:?}, {:?} at {}:{}: ", naked_block.block_type, &naked_block.name, naked_block.script_path(), &naked_block.script_line)
                }
        }
    }

    pub fn exec_fun(&self, log: &Log, fun_block: & GenBlock, res_prev: &Option<VarVal>) -> Option<VarVal> {
        let name = fun_block.name.as_ref().unwrap().as_str();
        let write_lambda = |file:&mut File, fname| {let len = fun_block.params.len();
                    for  i in 1..len {
                       if write!(file, "{}", self.parameter(log, i, fun_block, res_prev)).is_err() {
                            log.error(&format!{"Writing in {} failed at {}:{}: ", fname, fun_block.script_path(), &fun_block.script_line});
                            break
                        }
                    }
                };
        // TODO for arrays
        let is_true_lambda = |current| 
            match self.prev_or_search_up(current, res_prev) {
            Some(var) if var.val_type == VarType::Array => var.is_true(),
            _ =>    "true" == *self.expand_parameter(log, current, fun_block, res_prev)};
        match name {
            "display" => {
                println!("{}", util::insert_ctrl_char(&self.parameter(log, 0, fun_block, res_prev)));
                io::stdout().flush().unwrap();
                if fun_block.params.len() > 1 {
                    log.error(&format!{"Display parameters are ignored after first one at {}:{}: ", fun_block.script_path(), &fun_block.script_line})
                }
                return res_prev.clone()
            },
            "now" => {
                if no_parameters(fun_block) {return Some(VarVal::from_string(format_system_time(SystemTime::now())))}
                let fmt_str = *self.parameter(log, 0, fun_block, res_prev);
                return Some(VarVal::from_string(format_time(&fmt_str, SystemTime::now())))
            },
            "write" => {
                let mut fname = *self.parameter(log, 0, fun_block, res_prev);
                if !has_root(&fname) && let Some(cwd) = fun_block.search_up(CWD) {
                    fname = cwd.value + MAIN_SEPARATOR_STR + &fname
                }
                let file = File::create(&fname);
                if let Ok(mut file) = file {
                    write_lambda(&mut file, &fname)
                } else {
                    log.error(&format!{"File {} can't be opened for writing at {}:{}: ", fname, fun_block.script_path(), &fun_block.script_line})
                } 
            }
            "writex" if cfg!(not(unix)) => {
                let mut fname = *self.parameter(log, 0, fun_block, res_prev);
                if !has_root(&fname) && let Some(cwd) = fun_block.search_up(CWD) {
                    fname = cwd.value + MAIN_SEPARATOR_STR + &fname
                }
                let file = File::create(&fname);
                if let Ok(mut file) = file {
                    write_lambda(&mut file, &fname)
                } else {
                    log.error(&format!{"File {} can't be opened for writing at {}:{}: ", fname, fun_block.script_path(), &fun_block.script_line})
                } 
            }
            "writea" => {
                let mut fname = *self.parameter(log, 0, fun_block, res_prev);
                if !has_root(&fname) && let Some(cwd) = fun_block.search_up(CWD) {
                    fname = cwd.value + MAIN_SEPARATOR_STR + &fname
                }
                if let Ok(mut file) =  OpenOptions::new()
                    .read(true)
                    .append(true) 
                    .create(true)
                    .open(&fname) {
                    write_lambda(&mut file, &fname)
                } else {
                    log.error(&format!{"File {} can't be opened for writing at {}:{}: ", fname, fun_block.script_path(), &fun_block.script_line})
                } 
            }
            #[cfg(unix)]
            "writex" if cfg!(unix) => {
                let mut fname = *self.parameter(log, 0, fun_block, res_prev);
                if !has_root(&fname) && let Some(cwd) = fun_block.search_up(CWD) {
                    fname = cwd.value + MAIN_SEPARATOR_STR + &fname
                }
                match OpenOptions::new()
                    .create(true)
                    .write(true)
                    .truncate(true)
                    .mode(0o700)
                    .open(&fname) {
                    Ok(mut file) => write_lambda(&mut file, &fname),
                    Err(_) => log.error(&format!{"File {} can't be opened for writing at {}:{}: ", fname, fun_block.script_path(), &fun_block.script_line}),
                } 
            }
            "assign" => return self.exec_assign(log, fun_block, res_prev),
            "neq" => {
                log.debug(&format!("comparing {:?} and {:?}", self.parameter(log, 0, fun_block, res_prev), self.parameter(log, 1, fun_block, res_prev)));

                return  Some(VarVal::from_bool(self.parameter(log, 0, fun_block, res_prev) != self.parameter(log, 1, fun_block, res_prev) ))
            },
            "eq" => {
                // TODO reuse common code with neq
                log.debug(&format!("comparing eq {:?} and {:?}", self.parameter(log, 0, fun_block, res_prev), self.parameter(log, 1, fun_block, res_prev)));
                return Some(VarVal::from_bool(self.parameter(log, 0, fun_block, res_prev) == self.parameter(log, 1, fun_block, res_prev)) )
            },
            "exec" | "aexec" => {
                let mut exec : String  = fun_block.flex.as_ref().unwrap().to_string();
                // look for var first
                if let Some(exec1) = fun_block.search_up(&exec) { exec = *process_template_value(log, &exec1.value, fun_block, res_prev);}
                let mut params: Vec<_> = Vec::new();
                for i in 0..fun_block.params.len() {
                    let param = &fun_block.params[i];
                    let val = self.prev_or_search_up(param, res_prev);
                    // TODO add resolving using last result ~~
                    log.debug(&format!("exec params: {:?} for {:?}", fun_block.params, val));
                    if let Some(param) = val {
                        if !param.values.is_empty() { // array
                            for param in param.values {
                                params.push(*process_template_value(log, &param, fun_block, res_prev))
                            }
                        } else if param.val_type != VarType::Array {
                            params.push(*process_template_value(log, &param.value, fun_block, res_prev))
                        }
                    } else {
                        params.push(*self.parameter(log, i, fun_block, res_prev))
                    } 
                }
                let dry_run = self.search_up("~dry-run~");
                let mut cwd = String::new();

                let mut calc_cwd = |work_dir_val: &String| {
                    if !work_dir_val.is_empty() {
                        let mut work_dir =
                        match fun_block.search_up(work_dir_val) {
                            Some(work_dir_val1) => { *process_template_value(log, &work_dir_val1.value, fun_block, res_prev)},
                            None => *process_template_value(log, work_dir_val, fun_block, res_prev)
                        };
                        //println!{"calc work dir {work_dir}"}
                        if !has_root(&work_dir) {
                            let cwd = fun_block.search_up(CWD);
                            //println!{"found cwd {cwd:?}"}
                            if let Some(cwd) = cwd {
                                work_dir = cwd.value + std::path::MAIN_SEPARATOR_STR + &work_dir
                            }
                        }
                        let path =  Path::new(&work_dir);
                        if path.exists() {
                            cwd = crate::util::normalize_path(path).display().to_string();
                        }
                    }
                };
                //println!{"parent dir {:?} of {:?} -> {:?}", fun_block.dir, fun_block.name, fun_block.flex}
                if fun_block.dir.is_some() {
                    let work_dir_val = fun_block.dir.as_ref().unwrap().to_string();
                    calc_cwd(&work_dir_val)
                } else {
                    // take it from the target cwd
                    let work_dir = fun_block.search_up(CWD);
                    if let Some(work_dir) = work_dir {
                        cwd = work_dir.value
                    }
                }
                
                if let Some(_dry_run) = dry_run {
                   log.log(&format!("Command: {:?} {:?} in {}", exec, params, cwd));
                   return Some(VarVal::from_i32(0))
                } else if "aexec" == name {
                    // TODO add a possibility of a user unput for async command
                    let mut command = Command::new(&exec);
                    let status = if cwd.is_empty() { command
                        .args(&params)
                        .envs(crate::get_properties())
                        .stdin(Stdio::null())
                         .spawn()
                     } else {
                        command.current_dir(&cwd).args(&params)
                            .stdin(Stdio::null())
                            .envs(crate::get_properties())
                            .spawn()
                     };
                     if let Ok(status) = status {
                        return Some(VarVal::from_i32(status.id() as i32))
                     }
                     log.error(&format!("Command {} with {:?} in {} failed to start asynchronically at {}:{}: , reason {}", exec, params, cwd, fun_block.script_path(), fun_block.script_line, status.err().unwrap()))
                } else if fun_block.out .is_some() {
                    let output = if cwd.is_empty() { Command::new(&exec)
                        .args(&params)
                        .envs(crate::get_properties())
                        .output() 
                    } else {
                            Command::new(&exec).envs(crate::get_properties()).current_dir(&cwd).args(&params)
                            .output()
                    };
                    // command is always async, simply output is waiting
                    let Ok(output) = output else {
                        log.error(&format!("Command {} with {:?} in {} failed to start at {}:{}: , reason {:?}", exec, params, cwd, fun_block.script_path(), fun_block.script_line, output.err()));
                        return None
                    };
                    // TODO more error handling
                    if output.status.success() {
                        let stdout = String::from_utf8_lossy(&output.stdout);
                        let parent_block = fun_block.parent.clone().unwrap();
                        let mut parent_block_mut = parent_block.borrow_mut();
                        parent_block_mut.vars.insert(fun_block.out.clone().unwrap(), VarVal::from_string(stdout.trim()));
                        
                        return Some(VarVal::from_i32(output.status.code().unwrap()))
                    } else {
                       log.error(&format!("Command {} with {:?} in {} failed to start at {}:{}: , reason {}", exec, params, cwd, fun_block.script_path(), fun_block.script_line, String::from_utf8_lossy(&output.stderr)))
                    }
                } else {
                    let status = if cwd.is_empty() { Command::new(&exec)
                        .args(&params)
                        .envs(crate::get_properties())
                        .status() 
                    } else {
                            Command::new(&exec).envs(crate::get_properties()).current_dir(&cwd).args(&params)
                            .status()
                    };
                    match status {
                        Ok(status) =>  match status.code() {
                            Some(code) => {
                                return Some(VarVal::from_i32(code))},
                                //self.parent().unwrap().add_var("~~".to_string(), VarVal{val_type: VarType::Number, value: code.to_string(), values: Vec::new()});},
                            None   => log.error(&format!("The process terminated by signal at {:?}:{}", fun_block.script_path(), &fun_block.script_line))
                        }
                        Err(err) => log.error(&format!("Command {} with {:?} in {} failed to start at {:?}:{}, reason {}", exec, params, cwd, fun_block.script_path(), fun_block.script_line, err))
                    } 
                }
            },
            "or" => return Some(VarVal::from_bool(fun_block.params.iter().any(is_true_lambda))),
            "and" => return Some(VarVal::from_bool(fun_block.params.iter().all(is_true_lambda))),
            "scalar" | "join" => { // vector var, separator
                let sep = if fun_block.params.len() > 1 {
                    *self.parameter(log, 1, fun_block, res_prev)
                } else {"\t".to_string()};
                let var = &self.array_to_string(&fun_block.prev_or_search_up(&fun_block.params[0], res_prev), &sep, res_prev);
                log.debug(&format!{"array to str : {:?}", var});
                return var.as_ref().map(VarVal::from_string)
            },
            "filename" => {
                let param = *self.parameter(log, 0, fun_block, res_prev);
                let dot_pos = param.rfind('.');
                let slash_pos = param.rfind(MAIN_SEPARATOR);
                match slash_pos {
                    None => {
                        match dot_pos {
                            Some(dot_pos) => {
                                return Some(VarVal::from_string(&param[0..dot_pos]));
                            },
                            None => return Some(VarVal::from_string(&param)),
                        }
                    },
                    Some(slash_pos) => {
                        match dot_pos {
                            Some(dot_pos) => {
                                return Some(VarVal::from_string(&param[slash_pos+1..dot_pos]));
                            },
                            None => return Some(VarVal::from_string(&param[slash_pos+1..]))
                        }
                    }
                }
            },
            "ask" | "prompt" => {
                // TODO add ask1 function to read only one char not requiring enter
                let len = fun_block.params.len();
                // consider using trait write - write!{writer, "..."}
                // TODO use - write!{stdout()}
                print!("{} ", util::insert_ctrl_char(&self.parameter(log, 0, fun_block, res_prev)));
                io::stdout().flush().unwrap();
                let mut user_input = String::new();
                let stdin = io::stdin();
                let res = stdin.read_line(&mut user_input);
                if res.is_err() {
                    log.error(&format!{"An error in getting a user input, the default input is used at {}:{}: ", fun_block.script_path(), &fun_block.script_line})
                }
                user_input = user_input.trim().to_string();
                if user_input.is_empty() && len > 1 {
                    user_input = *self.parameter(log, 1, fun_block, res_prev)
                }
                //println!("");
                return Some(VarVal::from_string(&user_input))
            },
            "timestamp" => {
                if no_parameters(fun_block) {
                    log.error(&format!{"No argument for timestamp at {}:{}: ", fun_block.script_path(), &fun_block.script_line});
                } else {
                    let mut fname = *self.parameter(log, 0, fun_block, res_prev);
                    if !has_root(&fname) && let Some(cwd) = fun_block.search_up(CWD) {
                        fname = cwd.value + MAIN_SEPARATOR_STR + &fname
                    }
                    let ts = timestamp(&fname);
                    match ts {
                        Some(timestamp) => return Some(VarVal::from_string(&timestamp)),
                        None => return None
                    }
                }   
            },
            "cropname" => { // the approach targets a file path, so for arbitrary name add a CWD to mask
                let mut fname = *self.parameter(log, 0, fun_block, res_prev);
                
                let Some(cwd) = fun_block.search_up(CWD) else {
                    log.error(&format!{"File can't be cropped because CWD isn't set at {}:{}: ", fun_block.script_path(), &fun_block.script_line});
                    return  Some(VarVal::from_string(fname))
                };
                if fun_block.params.len() < 2 {
                    log.error(&format!{"'cropname' requires at least 2 parameters at {}:{}: ", fun_block.script_path(), &fun_block.script_line});
                    return Some(VarVal::from_string(fname))
                }
                if !has_root(&fname) {
                    fname = cwd.value.clone() + MAIN_SEPARATOR_STR + &fname
                }
                let mut mask = *self.parameter(log, 1, fun_block, res_prev);
                let crop_end = mask.starts_with("*");
                // TODO consider substitute with 3rd parameter
                if crop_end {
                    mask = mask[1..].to_owned();
                    if fname.ends_with(&mask) {
                        if fun_block.params.len() == 3 {
                            let subs = *self.parameter(log, 2, fun_block, res_prev);
                            return Some(VarVal::from_string(fname[0..fname.len()-mask.len()].to_owned() + &subs))
                        }
                        return Some(VarVal::from_string(&fname[0..fname.len()-mask.len()]))
                    }
                } else {
                    let end_mask = if mask.ends_with("*") {
                        mask.pop();
                        String::new()
                    } else if let Some(star_pos) = mask.find('*') {
                        let end_mask = mask[star_pos+1..].to_owned();
                        mask = mask[0..star_pos].to_owned();
                        end_mask
                    } else {
                        String::new()
                    };
                    if !has_root(&mask) {
                        mask = cwd.value + MAIN_SEPARATOR_STR + &mask
                    }
                    if fname.starts_with(&mask) && (end_mask.is_empty() || fname.ends_with(&end_mask)) {
                        if fun_block.params.len() == 3 {
                            let subs = *self.parameter(log, 2, fun_block, res_prev);
                            let subs: Vec<_> = subs.splitn(2, '*').collect();
                            if subs.len() == 1 {
                                return Some(VarVal::from_string(subs[0].to_owned() + &fname[mask.len()..]))
                            } else {
                                return Some(VarVal::from_string(subs[0].to_owned() + &fname[mask.len()..(fname.len()-end_mask.len())] + subs[1]))
                            }
                        }
                        return Some(VarVal::from_string(&fname[mask.len()..fname.len()-end_mask.len()]))
                    }
                }
                
                return Some(VarVal::from_string(fname))
            }
            "read" => {
                let mut fname = *self.parameter(log, 0, fun_block, res_prev);
                if !has_root(&fname) && let Some(cwd) = fun_block.search_up(CWD) {
                    fname = cwd.value + MAIN_SEPARATOR_STR + &fname
                }
                let file_content = &fs::read_to_string(&fname).ok();
                return match file_content {
                    Some(content) => Some(VarVal::from_string(content)),
                    None => {
                        log.error(&format!{"File {} can't be opened for reading or a read error at {}:{}: ", fname, fun_block.script_path(), &fun_block.script_line});
                        None
                    }
                }
            },
            "absolute" | "canonicalize" => {
                let mut path = *self.parameter(log, 0, fun_block, res_prev);
                if !has_root(&path) && let Some(cwd) = fun_block.search_up(CWD) {
                    path = cwd.value + MAIN_SEPARATOR_STR + &path
                }
                #[cfg(any(unix, target_os = "redox"))]
                if name == "canonicalize" && let Ok(can_path) = fs::canonicalize(&path) { path =  can_path.into_os_string().into_string().unwrap() }
                #[cfg(target_os = "windows")]
                {path = crate::util::normalize_path( Path::new(&path)).display().to_string();}
                return Some(VarVal::from_string(path))
            }
            "newerthan" => {
                // it compares a modification date of files specified by 1st parameter
                // in a form path/.ext1 with files specified by second parameter in
                // the same form and return an array of files from first parameters with 
                // newer modification time. File names are prependent with directory names 
                // relative to the parameter path
                // check if 1 or 2 parameters only
                let len = fun_block.params.len();
                let (dir1, ext1) = dir_ext_param(&self.parameter(log, 0, fun_block, res_prev));
                if dir1.is_none() || ext1.is_none() {
                    log.error(&format!("Parameter {} doesn't have path/ext pattern at {}:{}: ", &self.parameter(log, 0, fun_block, res_prev), fun_block.script_path(), &fun_block.script_line));
                    return None
                }
                let (mut dir2, ext2) =
                    if len > 1 { 
                        dir_ext_param(&self.parameter(log, 1, fun_block, res_prev))
                    } else {
                        (None,None)
                    };
                let mut dir1 = dir1?;
                if let Some(cwd) = fun_block.search_up(CWD) {
                    if !has_root(&dir1) {
                        dir1 = cwd.value.clone() + MAIN_SEPARATOR_STR + &dir1
                    }
                    if let Some(ref dir2v) = dir2 && !has_root(dir2v) {
                        dir2 = Some(cwd.value + MAIN_SEPARATOR_STR + dir2v)
                    }
                }
                log.debug(&format!{"newerthen: {:?}/{:?} then {:?}/{:?}", &dir1, &ext1, &dir2, &ext2});
                return Some(VarVal::from_vec(&find_newer(&dir1, &ext1.unwrap(), &dir2, &ext2)))
            },
            "anynewer" => {
                log.debug(&format!("evaluating anynewer: {}", fun_block.params.len()));
                let mut p1 = *self.parameter(log, 0, fun_block, res_prev);
                let mut p2 = *self.parameter(log, 1, fun_block, res_prev);
                if (!has_root(&p1) || !has_root(&p2)) &&
                    let Some(cwd) = fun_block.search_up(CWD) {
                    if !has_root(&p1) {
                        p1 = cwd.value.clone() + MAIN_SEPARATOR_STR + &p1
                    }
                    if !has_root(&p2) {
                        p2 = cwd.value + MAIN_SEPARATOR_STR + &p2
                    }
                }
                log.debug(&format!("anynewer parameters: {}, {}", p1, p2));
                return Some(VarVal::from_bool(newest(&p1) > newest(&p2)))
            }
            "gt" => {
                if fun_block.params.len() != 2 {
                    log.error(&format!{"Greater than requires 2 parameters, but specified {} at {}:{}: ", &fun_block.params.len(), fun_block.script_path(), fun_block.script_line}) 
                } else {
                    let p1 = *self.parameter(log, 0, fun_block, res_prev);
                    let p2 = *self.parameter(log, 1, fun_block, res_prev);
                    log.debug(&format!("Comparing {} to {} at greater", &p1, &p2));
                    // TODO baybe check val_type == Number ?
                    if let Ok(val) = p1.parse::<f64>() && let Ok(val2) = p2.parse::<f64>() {
                        return Some(VarVal::from_bool(val > val2))
                    }
                    return Some(VarVal::from_bool(p1 > p2))
                }
            },
            "lt" => {
                if fun_block.params.len() != 2 {
                    log.error(&format!{"Littler than requires 2 parameters, but specified {} at {}:{}: ", &fun_block.params.len(), fun_block.script_path(), fun_block.script_line}) 
                } else {
                    let p1 = *self.parameter(log, 0, fun_block, res_prev);
                    let p2 = *self.parameter(log, 1, fun_block, res_prev);
                    log.debug(&format!("Comparing {} to {} at littler", &p1, &p2));
                    // TODO baybe check val_type == Number ?
                    if let Ok(val) = p1.parse::<f64>() && let Ok(val2) = p2.parse::<f64>() {
                        return Some(VarVal::from_bool(val < val2))
                    }
                    return Some(VarVal::from_bool(p1 < p2))
                }
            },
            "not" => return Some(VarVal::from_bool(!VarVal::from_string(*self.parameter(log, 0, fun_block, res_prev)).is_true())),
            "contains" | "find" => {
                if fun_block.params.len() != 2 {
                    log.error(&format!{"Contains requires 2 parameters, but specified {} at {}:{}: ", &fun_block.params.len(), fun_block.script_path(), fun_block.script_line}) 
                } else {
                    let p1 = *self.parameter(log, 0, fun_block, res_prev);
                    let p2 = *self.parameter(log, 1, fun_block, res_prev);
                    return Some(VarVal::from_bool(p1.contains(&p2)))
                }
            }
            "as_url" => {
               let param = self.prev_or_search_up(&fun_block.params[0], res_prev);
               log.debug(&format!{"param: {:?}", param});
               if let Some(param) = param {
                   match param.val_type {
                    VarType::RepositoryRust => {
                        if let Some(pos) = param.value.find('@') {
                            return Some(VarVal::from_string(format!("https://crates.io/api/v1/crates/{}/{}/download", &param.value[0..pos], &param.value[pos+1..])))
                        }
                    },
                    VarType::RepositoryMaven | VarType::Generic => {
                        let parts = param.value.split(':');
                        let mav_parts: Vec<_> = parts.collect();
                        //https://repo1.maven.org/maven2/com/baomidou/mybatis-plus-boot-starter/3.5.3.1/mybatis-plus-boot-starter-3.5.3.1.jar
                        return Some(VarVal::from_string(format!("https://repo1.maven.org/maven2/{}/{}/{}/{}-{}.jar", &mav_parts[0].replace(".", "/"), &mav_parts[1], &mav_parts[2], &mav_parts[1], &mav_parts[2])))
                    },
                    _ => ()
                   }
               }
            },
            "as_jar" => {
                let param = self.prev_or_search_up(&fun_block.params[0], res_prev);
                log.debug(&format!{"jar param: {:?}", param});
                if let Some(param) = param {
                   match param.val_type {
                        VarType::RepositoryMaven | VarType::Generic => {
                            let parts = param.value.split(':');
                            let parts: Vec<_> = parts.collect();
                            if parts.len() !=3 {
                                log.error(&format!("Expected 3 parts of maven URL but found {} at {}:{}: ", parts.len(), fun_block.script_path(), &fun_block.script_line));
                                return None
                            }
                            //let mav_parts: Vec<_> = parts.collect();
                            return Some(VarVal::from_string(format!("{}-{}.jar", parts[1], parts[2])))
                        },
                        _ => ()
                    }
                }
            }
            "array" => {
                let mut res : Vec<_> = Vec::new();
                for i in 0..fun_block.params.len() {
                    // TODO make the approach as a util method
                    // TODO although there is a note about string interpolation, perhaps do it only for final use
                    // destinations as evaluate parameters in a function or a block
                    match fun_block.prev_or_search_up(&fun_block.params[i], res_prev) {
                        Some(param1) => { 
                            if param1.val_type == VarType::Array {
                                res.extend_from_slice(&param1.values) // consider to massage a value  *process_template_value(log, param1.values[k], &fun_block, res_prev);
                            } else {
                                res.push(*self.parameter(log, i, fun_block, res_prev))
                            }
                        },
                        None => { let param = *self.parameter(log, i, fun_block, res_prev);
                            if !param.is_empty() {
                                res.push(param)
                            } else if fun_block.params.len() > 1 {
                                log.error(&format!{"An empty parameter {} is ignored at {}:{}: ", i, fun_block.script_path(), &fun_block.script_line})
                            }
                        }
                    }
                }
                //println!{"vec -> {:?}", &res};
                return Some(VarVal::from_vec(&res))
            },
            "file_filter" | "filter" => { // remove from an array parameter all matching parameters 1..n
                let param = self.prev_or_search_up(&fun_block.params[0], res_prev);
                if let Some(param) = param && param.val_type == VarType::Array {
                    let filter_vals = fun_block.params[1..].iter().map(|filter| process_template_value(log, filter, fun_block, res_prev)).collect::<Vec<_>>();
                    let files = param.values;
                    let cwd =
                        match fun_block.search_up(CWD) {
                        Some(cwd) => cwd.value,
                        None => "".to_string()
                    };
                    let vec = files.into_iter().filter(|file| {
                        let mut file = *process_template_value(log, file, fun_block, res_prev);
                        if !has_root(&file) {
                            file = cwd.clone() + MAIN_SEPARATOR_STR + &file
                        }
                        let p = Path::new(&file);//println!{"checking {p:?}"}
                        if !p.exists() {return false} 
                        let name = p.file_name().unwrap().to_str().unwrap();
                        for filter in &filter_vals {
                            if matches(name, filter) {
                                return false
                            }
                        }
                        true
                    }).collect();
                    return Some(VarVal::from_vec(&vec))
                } else {
                    log.error(&format!{"Variable {} not found or not an array at {}:{}: ", fun_block.params[0], fun_block.script_path(), &fun_block.script_line})
                }
            },
            "panic" => {
                panic!("{} at {}:{}: ", self.parameter(log, 0, fun_block, res_prev), fun_block.script_path(), fun_block.script_line)
            },
            "element" => { // the function allows to extract or set an element of an array
                if fun_block.params.len() < 2 {
                    log.error(&format!{"The 'element' requires 2 or 3 parameters, but specified {} at {}:{}: ", &fun_block.params.len(), fun_block.script_path(), fun_block.script_line}) ;
                    return None
                }
                let name = &fun_block.params[0];
                let Some(var_block) = fun_block.parent.clone().unwrap().search_up_block(name) else {
                    log.error(&format!{"Specified argument {} wasn't found at {}:{}: ", &name,  fun_block.script_path(), fun_block.script_line});
                    return None 
                };
                
                let index_param = match self.prev_or_search_up(&fun_block.params[1], res_prev) {
                    None => fun_block.params[1].to_owned(),
                    Some(val) => val.value.clone()
                };
                let index : usize = index_param.parse().unwrap_or_default();
                let val = if fun_block.params.len() > 2 {
                    Some(*self.parameter(log, 2, fun_block, res_prev))
                } else { None};
                let mut parent_bare = var_block.0.borrow_mut();
                let var = parent_bare.vars.get_mut(name)?;
                if var.val_type == VarType::Array {
                    if var.values.is_empty() || index > var.values.len() -1 {
                        log.error(&format!{"Specified index {} doesn't exist in the array {} at {}:{}: ",  index, &name, fun_block.script_path(), fun_block.script_line});
                        return None
                    }
                    let res = Some(VarVal::from_string(&var.values[index]));
                    if let Some(val) = val { // set
                        var.values[index] = val
                    }
                    return res // get/set
                } else {
                    log.error(&format!{"Specified argument {} isn't an array at {}:{}: ",  &name, fun_block.script_path(), fun_block.script_line});
                }
            },
            "set_env" => {
                if fun_block.params.len() != 2 {
                    log.error(&format!{"Set environment requires 2 parameters, but specified {} at {}:{}: ", &fun_block.params.len(), fun_block.script_path(), fun_block.script_line}) 
                } else {
                    let key = *self.parameter(log, 0, fun_block, res_prev);
                    let val = *self.parameter(log, 1, fun_block, res_prev);
                    log.debug(&format!("Set env {} to {}", &key, val));
                    //unsafe { env::set_var(key, val) }
                    crate::set_property(&key, &val)
                }
            },
            "env" => {
                let key = *self.parameter(log, 0, fun_block, res_prev);
                if let Some(val) = crate::get_property(&key) {
                    return Some(VarVal::from_string(val))
                }
                if let Ok(val) = env::var(key) {
                    return Some(VarVal::from_string(val))
                }
            },
            "files" => {
                let mut res : Vec<_> = Vec::new();
                let cwd = fun_block.search_up(CWD);
                for i in 0.. fun_block.params.len() {
                    let mut file = *self.parameter(log, i, fun_block, res_prev);
                    if !has_root(&file) && let Some(ref path) = cwd { file = path.value.clone() + MAIN_SEPARATOR_STR + &file }
                    let sep = MAIN_SEPARATOR;
                    let mut file_indices = file.char_indices();//.nth_back(4).unwrap().0
                    // str ends / then search will be in the current dir and all subdirs
                    let recursive = if file_indices.nth_back(0).unwrap().1 == sep { file = file[0..=file_indices.nth_back(0).unwrap().0].to_string(); true }
                        else {false};
                    let path= PathBuf::from(&file);
                    let filename = path.file_name().unwrap().to_str().unwrap().to_string();
                    // TODO introduce esc * in future
                    if let Some(star_pos) = filename.find("*") {
                        let dir = path.parent().unwrap_or(Path::new(MAIN_SEPARATOR_STR));
                        let mut chars = filename.chars();
                        let (start, end) = if chars.nth(0).unwrap() == '*' {
                            (None, Some(&filename[1..]))
                        } else if chars.last().unwrap() == '*' {
                            (Some(&filename[0..star_pos]), None)
                        } else {
                            (Some(&filename[0..star_pos]), Some(&filename[star_pos+1..]))
                        };
                        fill_dir(&mut res, dir, &start, &end, recursive, false)
                    } else {
                        res.push(file)
                    }
                }
                return Some(VarVal::from_vec(&res))
            },
            "range" | "slice" => {
                let start = *self.parameter(log, 1, fun_block, res_prev);
                let start: usize = start.parse().ok()?;
                
                let str = *self.parameter(log, 0, fun_block, res_prev);
                if !str.is_empty () {
                    let end = if fun_block.params.len() > 2 
                    {(*self.parameter(log, 2, fun_block, res_prev)).parse().ok()?} else {str.len()};
                    return Some(VarVal::from_string(&str[start..end]))
                } else if let Some(var) = fun_block.search_up(&fun_block.params[0]) && var.val_type == VarType::Array {
                    let end = if fun_block.params.len() > 2 
                        {(*self.parameter(log, 2, fun_block, res_prev)).parse().ok()?} else {var.values.len()};
                    return Some(VarVal::from_vec(&var.values[start..end].to_vec()))
                } 
            }
            "cp" => {
                let mut res : Vec<_> = Vec::new();
                let len = fun_block.params.len();
                let cwd =
                    match fun_block.search_up(CWD) {
                    Some(cwd) => cwd.value,
                    None => "".to_string()
                    };
                for mut i in (0.. len).step_by(2) {
                    let mut file_from = *self.parameter(log, i, fun_block, res_prev);
                    if !has_root(&file_from) {
                        file_from = cwd.clone() + MAIN_SEPARATOR_STR + &file_from
                    }
                    i += 1;
                    let mut file_to = *self.parameter(log, i, fun_block, res_prev);
                    if !has_root(&file_to) {
                        file_to = cwd.clone() + MAIN_SEPARATOR_STR + &file_to
                    }
                     if !file_from.is_empty() && !file_to.is_empty() {
                        if PathBuf::from(file_to.clone()).is_dir() {
                            file_to += &(MAIN_SEPARATOR_STR.to_owned() + PathBuf::from(file_from.clone()).file_name().unwrap().to_str().unwrap())
                        }
                        if copy(&file_from, &file_to).is_ok() {
                            res.push(file_to) // possibly size copied
                        }
                    }
                }
                return Some(VarVal::from_vec(&res))
            },
            "mv" => {
                let mut res : Vec<_> = Vec::new();
                let cwd =
                    match fun_block.search_up(CWD) {
                    Some(cwd) => cwd.value,
                    None => "".to_string()
                    };
                for mut i in (0.. fun_block.params.len()).step_by(2) {
                    let mut file_from = *self.parameter(log, i, fun_block, res_prev);
                    if !has_root(&file_from) {
                        file_from = cwd.clone() + MAIN_SEPARATOR_STR + &file_from
                    }
                    i += 1;
                    let mut file_to = *self.parameter(log, i, fun_block, res_prev);
                    if !has_root(&file_to) {
                        file_to = cwd.clone() + MAIN_SEPARATOR_STR + &file_to
                    }

                     if !file_from.is_empty() && !file_to.is_empty() {
                        if PathBuf::from(file_to.clone()).is_dir() {
                            file_to += &(MAIN_SEPARATOR_STR.to_owned() + PathBuf::from(file_from.clone()).file_name().unwrap().to_str().unwrap())
                        }
                        if rename(&file_from, &file_to).is_ok() {
                            res.push(file_to) 
                        }
                    }
                }
                return Some(VarVal::from_vec(&res))
            },
            "mkd" => {
                let mut res : Vec<_> = Vec::new();
                 let cwd =
                    match fun_block.search_up(CWD) {
                    Some(cwd) => cwd.value,
                    None => "".to_string()
                    };
                for i in 0.. fun_block.params.len() {
                    let mut file = *self.parameter(log, i, fun_block, res_prev);
                    if !file.is_empty() {
                        if !has_root(&file) {
                            file = cwd.clone() + MAIN_SEPARATOR_STR + &file
                        }
                        if create_dir_all(&file).is_ok() {
                            res.push(file)
                        }
                    }
                }
                return Some(VarVal::from_vec(&res))
            },
            "rm" => {
                let mut res : Vec<_> = Vec::new();
                let cwd =
                    match fun_block.search_up(CWD) {
                    Some(cwd) => cwd.value,
                    None => "".to_string()
                    };
                for i in 0.. fun_block.params.len() {
                    let mut file = *self.parameter(log, i, fun_block, res_prev);
                    if !file.is_empty() {
                        if !has_root(&file) {
                            file = cwd.clone() + MAIN_SEPARATOR_STR + &file
                        }
                        if remove_file(&file).is_ok() {
                            res.push(file)
                        }
                    }
                }
                return Some(VarVal::from_vec(&res))
            },
            "rmdir" | "rmdira" => {
                let mut res : Vec<_> = Vec::new();
                 let cwd =
                    match fun_block.search_up(CWD) {
                    Some(cwd) => cwd.value,
                    None => "".to_string()
                    };
                for i in 0.. fun_block.params.len() {
                    let mut file = *self.parameter(log, i, fun_block, res_prev);
                    if !file.is_empty() {
                        if !has_root(&file) {
                            file = cwd.clone() + MAIN_SEPARATOR_STR + &file
                        }
                        if name == "rmdira" && remove_dir_all(&file).is_ok() || remove_dir(&file).is_ok(){
                            res.push(file)
                        }
                    }
                }
                return Some(VarVal::from_vec(&res))
            },
            "calc" => {
                if fun_block.params.len() > 1 {
                    // return a vector then
                    let mut res = Vec::new();
                    for i in 0..fun_block.params.len() {
                        match self.calc(*self.parameter(log, i, fun_block, res_prev)) {
                            Ok(cur) => res .push(cur.to_string()),
                            _ => continue
                        }
                    }
                    return Some(VarVal::from_vec(&res))
                } else {
                    match self.calc(*self.parameter(log, 0, fun_block, res_prev)) {
                        Ok(res) => {
                            return Some(VarVal::from_f64(res))
                        },
                        Err(err) => {
                            log.error(&format!{"Error {:?} in {}  at {}:{}: ", err, *self.parameter(log, 0, fun_block, res_prev), fun_block.script_path(), fun_block.script_line}) 
                        }
                    } 
                }
            },
            "number" => {
                let val = *self.parameter(log, 0, fun_block, res_prev);
                let num = if val.is_empty() {
                    0
                } else {
                    val.parse::<i32>().ok() ?
                };
                return Some(VarVal::from_i32(num))
            }
            "zip" => {
                // variable parameters
                // first zip file name and location
                // parameter 1+...
                // -A <blank> zip entry path, next : content to zip (script generated), -E adds execute permission on Unix
                // -C <blank> zip entry path, next dir with possible file name mask and all 
                // sub directories
                // -B : file or wildcard files to add without sub directories, and it can be var name of an array
                let mut zip_path = *self.parameter(log, 0, fun_block, res_prev);
                let cwd = fun_block.search_up(CWD);
                if !has_root(&zip_path) && let Some(ref cwd) = cwd { zip_path = cwd.value.clone() + MAIN_SEPARATOR_STR + &zip_path }
                if zip_path.find('.').is_none() {
                    zip_path += ".zip"
                }
                let mut zip = simzip::ZipInfo::new_with_comment(zip_path.clone(),
                    &format!{"Zipped by RustBee {}", ver::version().0});
                zip.prohibit_duplicates();
                let flatten_params = &fun_block.params;//&fun_block.flatten_params(&res_prev);
                let mut current_op = 1;
                // consider also flatten vec first and then iterate
                while current_op < flatten_params.len() {
                    let op = *self.expand_parameter(log, &flatten_params[current_op], fun_block, res_prev);
                    //println!{"{op} -> {}", &flaten_params[current_op]}
                    if op.starts_with("-A") || op.starts_with("-E") {
                        let name = &op[3..].trim_start();
                        //normalize_path(&mut name);
                        current_op += 1;
                        // think of to work with array parameters
                        let cont = *self.expand_parameter(log, &flatten_params[current_op], fun_block, res_prev);
                        let mut entry = simzip::ZipEntry::new(name, cont.as_bytes().to_vec());
                        if op.starts_with("-E") {
                            entry.attributes.insert(simzip::Attribute::Exec);
                        }
                        if !zip.add(entry) {
                            log.warning(&format!{"Zip entry {} already exists", &name})
                        }
                    } else if op.starts_with("-C") {
                        let path = if op.len() > 3 {
                            Some(&op[3..])
                        } else { None };
                        current_op+=1;
                        let mut files = *self.expand_parameter(log, &flatten_params[current_op], fun_block, res_prev);
                        // consider also processing arrays
                        //println!{"files ext {files}"}
                        if !has_root(&files) && let Some(ref cwd) = cwd { files = cwd.value.clone() + MAIN_SEPARATOR_STR + &files }
                        let files = Path::new(&files);
                        assert!(&files.has_root());
                        let parent_files = files.parent().unwrap_or(Path::new("."));
                        let filename = files.file_name().unwrap().to_str().unwrap().to_string();
                        
                        if let Some(pos) = filename.find("*") {
                            let mut chars = filename.chars();
                            let (start, end) = if chars.nth(0).unwrap() == '*' {
                                (None, Some(&filename[1..]))
                            } else if chars.last().unwrap() == '*' {
                                (Some(&filename[0..pos]), None)
                            } else {
                                (Some(&filename[0..pos]), Some(&filename[pos+1..]))
                            };
                            zip_dir(log, &mut zip, parent_files, path, start, end)
                        } else if files.is_dir() {
                            zip_dir(log, &mut zip, files, path, None, None)
                        } else if files.is_file() {
                            if !zip.add(simzip::ZipEntry::from_file(files.as_os_str().to_str().unwrap(), path.map(str::to_string).as_ref())) {
                                log.warning(&format!{"Zip entry {1:?}/{0} already exists", &files.as_os_str().to_str().unwrap(), &path})
                            }
                        } else {
                            log.error(&format!{"Path {files:?} can't be zipped at {}:{}: ", fun_block.script_path(), &fun_block.script_line})
                        }
                    } else if op.starts_with("-B") { // probably -C takes all cases
                         let path = if op.len() > 3 {
                            Some(&op[3..])
                        } else { None };
                        current_op+=1;
                        // -B <some path>
                        // name of var the value of the var is a var name holding array of paths
                        //
                        let zipped_path = self.prev_or_search_up(&flatten_params[current_op], res_prev) ;
                       //println!{"found pd {zipped_path:?} for {}", &flatten_params[current_op]}
                        // nested to nested case?
                        let values = 
                        if let Some(zipped_path) = zipped_path {
                            if zipped_path.val_type == VarType::Array {
                                //println!{"it's array of {}", zipped_path.values.len()}
                                zipped_path.values
                            } else {
                                //println!{"not array, {}", zipped_path.value}
                                vec![zipped_path.value]
                            }
                        } else {
                            let mut values = vec![];
                            let val = *self.expand_parameter(log, &flatten_params[current_op], fun_block, res_prev);
                            //println!{"single {val}"}
                            values.push(val.clone());
                            values
                        };
                        // build an array of an array of values, an array one element of the
                        // found value or an array of 1 element of the parameter value
                        for mut entry in values {
                            // interpolation first
                            entry = *process_template_value(log, &entry, fun_block, res_prev);
                            
                            if !has_root(&entry) && let Some(ref cwd) = cwd { entry = cwd.value.clone() + MAIN_SEPARATOR_STR + &entry }
                            let entry_path = Path::new(&entry);
                            let parent_files = entry_path.parent().unwrap_or(Path::new("."));
                            let filename = entry_path.file_name().unwrap().to_str().unwrap().to_string();
                            if let Some(pos) = filename.find("*") {
                                let mut chars = filename.chars();
                                let (start, end) = if chars.nth(0).unwrap() == '*' {
                                    (None, Some(&filename[1..]))
                                } else if chars.last().unwrap() == '*' {
                                    (Some(&filename[0..pos]), None)
                                } else {
                                    (Some(&filename[0..pos]), Some(&filename[pos+1..]))
                                };
                                match parent_files.read_dir() {
                                    Ok(dir) => {
                                        for entry in dir.flatten() {
                                            let name = entry.file_name().to_str().unwrap().to_owned();
                                            if entry.file_type().unwrap().is_file() &&
                                            (start.is_some() && name.starts_with(start.unwrap()) &&
                                                end.is_some() && name.ends_with(end.unwrap()) ||
                                            start.is_none() && end.is_some() && name.ends_with(end.unwrap()) ||
                                            start.is_some() && name.starts_with(start.unwrap()) && end.is_none() ||
                                            start.is_none() && end.is_none()) &&
                                            !zip.add(simzip::ZipEntry::from_file(entry.path().as_os_str().to_str().unwrap(), path.map(str::to_string).as_ref())) {
                                                log.warning(&format!{"Zip entry {1:?}/{0} already exists", &name, &path} )
                                            }
                                        }
                                    }
                                    _ => log.warning(&format!{"Zip: can't process directory {parent_files:?}"})
                                }
                            } else if entry_path.is_file() {
                                if !zip.add(simzip::ZipEntry::from_file(entry_path.as_os_str().to_str().unwrap(), path.map(str::to_string).as_ref())) {
                                    log.warning(&format!{"Zip entry {1:?}/{0} already exists", &entry_path.as_os_str().to_str().unwrap(), &path} )
                                }
                            } else if entry_path.is_dir() {
                                match entry_path.read_dir() {
                                    Ok(dir) => {
                                        for entry in dir {
                                            if let Ok(entry) = entry && entry.file_type().unwrap().is_file()
                                             && !zip.add(simzip::ZipEntry::from_file(entry.path().as_os_str().to_str().unwrap(), path.map(str::to_string).as_ref())) {
                                                log.warning(&format!{"Zip entry {1:?}/{0} already exists", &entry.path().as_os_str().to_str().unwrap(), &path} )
                                            }
                                        }
                                    }
                                    _ => log.warning(&format!{"Zip: can't read directory{entry_path:?}"})
                                }
                            } else {
                                log.warning(&format!{"Zip: unknown : {entry_path:?}"})
                            }
                        }
                    }
                    current_op += 1
                }
                match zip.store() {
                    Ok(()) => return Some(VarVal::from_string(zip_path)),
                    Err(msg) => log.error(&format!{"Zip: {msg} at {}:{}: ", fun_block.script_path(), &fun_block.script_line})
                }
            },
            "cfg" => {
                let cfg_path;
                if cfg!(target_os = "macos") {
                    match std::env::var("HOME") {
                        Ok(path) => cfg_path = format!("{path}/Library/Application Support"),
                        Err(_) => cfg_path = String::new(),
                    }
                } else if cfg!(unix) {
                    match std::env::var("HOME") {
                        Ok(path) => cfg_path = format!("{path}/.config"),
                        Err(_) => cfg_path = String::new(),
                    }
                } else if cfg!(windows) {
                    match std::env::var("LOCALAPPDATA") {
                        Ok(path) => cfg_path = path,
                        Err(_) => cfg_path = String::new()
                    }
                } else {
                    cfg_path = String::new();
                }
                return Some(VarVal::from_string(cfg_path))
            }
            _ => todo!("no such function: {:?} at {}:{}: ", fun_block.name, fun_block.script_path(), &fun_block.script_line)
        }
        None
    }

    pub fn exec_assign(&self, log: &Log, fun_block: &GenBlock, res_prev: &Option<VarVal>) -> Option<VarVal> {
        let name = fun_block.params[0].to_owned();
        let mut parent = self.parent().unwrap(); // because fun block can't be Main
        let mut close_scope = None;
        loop {
            //println!("borrowed {:?} - {:?}", parent.0.borrow().name, parent.0.borrow().block_type);
            // TODO if not found in Main then go back in the closest scope
            let parent_bare = parent.0.borrow();
            if close_scope .is_none() && (parent_bare.block_type == BlockType::Scope || parent_bare.block_type == BlockType::Target 
                || parent_bare.block_type == BlockType::Main ) {
                close_scope = Some(parent.clone())
            }
            if parent_bare.vars.contains_key(&name) {
                break
            } else if parent_bare.block_type == BlockType::Main {
                if let Some(close_scope) = close_scope {
                    drop(parent_bare);
                    parent = close_scope
                }
                break
            } else {
                drop(parent_bare);
                match parent.parent() { //parent.parent().unwrap()
                    Some(value) => parent = value,
                    _ => break,
                } //println!{"-> {parent:?}"}
            }
        }
        // println!{"insert var {} in block {:?}", &name, parent.borrow().block_type};
        if fun_block.params.len() == 1 { // clear assignment
            let mut parent_nak = parent.0.borrow_mut();
            parent_nak.vars.remove(&name)
        } else {
            let val = match fun_block.prev_or_search_up(&fun_block.params[1], res_prev) {
                Some(var) => var,
                None => VarVal::from_string(*process_template_value(log, &fun_block.params[1], fun_block, res_prev))
            };
            let mut parent_nak = parent.0.borrow_mut();
            parent_nak.vars.insert(name, val)
        }
    }

    pub fn parameter(&self, log: &Log, i: usize, fun_block: &GenBlock, res_prev: &Option<VarVal>) -> Box<String> {
        if !fun_block.params.is_empty() && i < fun_block.params.len() {
            self.expand_parameter(log, &fun_block.params[i], fun_block, res_prev)
        } else {
            log.error(&format!("Calling for parameter {i} in non existing parameter of {:?} at {}:{}: ", fun_block.name, fun_block.script_path(), &fun_block.script_line));
            Box::new(String::new())
        }
    }

    pub fn expand_parameter(&self, log: &Log, param_val: &String, fun_block: &GenBlock, res_prev: &Option<VarVal>) -> Box<String> {
        let param = fun_block.prev_or_search_up(param_val, res_prev);
        log.debug(&format!("looking for {:?} of {:?} as {:?}", param_val, &fun_block.block_type, param));
        match param {
            None => process_template_value(log, param_val, fun_block, res_prev),
            // TODO extend  val.value accordingly val.val_type
            Some(val) => {
                let var = match val.val_type {
                    VarType::Environment  => {
                        match env::var(&val.value) {
                            Ok(val) => val,
                            Err(_e) => val.value 
                        }
                    },
                    VarType::Property => {
                        if let Some(val) = get_property(&val.value) {
                            val
                        } else {
                            val.value
                        }
                    },
                    //VarType::Array => util:: vec_to_str( &val.values),
                    _ => if val.values.is_empty() {val.value} else {util:: vec_to_str( &val.values)}
                };
                process_template_value(log, &var.to_string(), fun_block, res_prev)
            }
        }
    }
    
    fn array_to_string(&self, val: &Option<VarVal>, sep: &str, res_prev: &Option<VarVal>) -> Option<String> {
        let Some(vec_param) = val else { return None};
        if vec_param.val_type == VarType::Array {
            Some(vec_param.values.clone().into_iter().map(|v| if let Some(v) = self.prev_or_search_up(&v, res_prev) {self.array_to_string(&Some(v), sep, res_prev).unwrap_or_default()} else {v}).collect::<Vec<_>>().join(sep))
        } else {
            Some(vec_param.value.clone())
        }
    }
    
    fn calc(&self, str: String) -> CalcResult {
        let chars = str.chars();
        let mut pos = 0usize;
        let mut state = Default::default();
        let mut buf_var = vec![' '; 512]; // buf for var name
        let mut name_pos = 0usize;
        let mut res  = 0.0;
        let mut op = Default::default();
        let mut deffered_res = 0.0;
        let mut deffered_op : Option<Op> = None;
        let mut exp_val : f64 = Default::default();
        let mut prev_state = Default::default();
        let mut sub_res_vec = Vec::new();
        let mut last_non_blank = Default::default(); 
        for c in chars {
            pos +=1;
            match c {
                ' ' | '\t' | '\n' | '\r' => {
                    match state {
                      CalcState::Start => (), 
                      // TODO state after exp ->  
                      CalcState::Exp => {
                        prev_state = state
                      },
                      CalcState::Var | CalcState::Val=> {
                        prev_state = state;
                        state = CalcState::Skip;
                        last_non_blank = name_pos;
                        buf_var [name_pos] = c;
                            name_pos += 1
                      },
                      CalcState::Oper => {
                        state = CalcState::Start
                      },
                      CalcState::Skip => {
                        buf_var [name_pos] = c;
                        name_pos += 1
                      },
                    }
                },
               '*' | '/' => {
                   // operates with previous 1 if first in chain
                   if state == CalcState::Skip {
                        name_pos = last_non_blank;
                        state = prev_state
                     }
                    match state {
                         CalcState::Start => {
                            return Err((CalcErrCause::InvOp, pos))
                        },
                        CalcState::Var => {
                            state = CalcState::Oper;
                            let var : String = buf_var[0..name_pos].iter().collect();
                            if let Some(val) = self.search_up( &var) {
                                if let Ok(val) = val.value.parse::<f64>() {
                                    match op {
                                        Op::Div => {
                                            if val == 0.0 {
                                                return Err((CalcErrCause::DZero, pos))
                                            }
                                            deffered_res /= val},
                                        Op::Mul => deffered_res *= val,
                                        Op::Plus | Op::Minus => deffered_res = val,
                                    }
                                    
                                } else { 
                                    return Err((CalcErrCause::NaN, pos))
                                }
                            } else {
                                return Err((CalcErrCause::NVar, pos))
                            }
                            name_pos = 0;
                            op = match c {
                                '*' => Op::Mul,
                                '/' => Op::Div,
                                _ => todo!()
                            };
                         }, 
                         CalcState::Val => {
                           state = CalcState::Oper;
                            let val : String = buf_var[0..name_pos].iter().collect();
                            if let Ok(val) = val.parse::<f64>() { 
                                match op {
                                    Op::Div => {
                                        if val == 0.0 {
                                            return Err((CalcErrCause::DZero, pos))
                                        }
                                        deffered_res /= val},
                                    Op::Mul => deffered_res *= val,
                                    Op::Plus | Op::Minus => deffered_res = val,
                                }
                                
                            } else { 
                                return Err((CalcErrCause::NaN, pos))
                            }
                            
                            name_pos = 0;
                            op = match c {
                                '*' => Op::Mul,
                                '/' => Op::Div,
                                _ => todo!()
                            };
                        },  
                        CalcState::Exp => {
                            state = CalcState::Oper;
                            match op {
                                Op::Div => {
                                    if exp_val == 0.0 {
                                        return Err((CalcErrCause::DZero, pos))
                                    }
                                    deffered_res /= exp_val},
                                Op::Mul => deffered_res *= exp_val,
                                Op::Plus | Op::Minus  => deffered_res = exp_val,
                            };
                            name_pos = 0;
                            op = match c {
                                '*' => Op::Mul,
                                '/' => Op::Div,
                                _ => todo!()
                            };
                          },
                        _ => todo!("state: {:?}", state)
                    }
                },
                '+' | '-'  => {
                    if state == CalcState::Skip {
                        name_pos = last_non_blank;
                        state = prev_state
                    }
                    match state {
                      CalcState::Start => {
                        buf_var [name_pos] = c;
                            name_pos += 1;
                            state = CalcState::Val;
                      },  
                      CalcState::Var => {
                        state = CalcState::Oper;
                        let var : String = buf_var[0..name_pos].iter().collect();
                        if let Some(val) = self.search_up( &var) {
                            //println!{"st {state:?} car {c} val {}", val.value};
                            if let Ok(val) = val.value.parse::<f64>() {
                                match op {
                                    Op::Div => {
                                        if val == 0.0 {
                                            return Err((CalcErrCause::DZero, pos))
                                        }
                                        deffered_res /= val},
                                    Op::Mul => deffered_res *= val,
                                    Op::Plus => deffered_res += val,
                                    Op::Minus => deffered_res -= val,                              
                                }
                            } else { 
                                return Err((CalcErrCause::NaN, pos))
                            }
                        } else {
                            return Err((CalcErrCause::NVar, pos))
                        }
                        name_pos = 0;
                        match deffered_op {
                            Some(Op::Plus) | None =>  { res += deffered_res; },
                            Some(Op::Minus) => { res -= deffered_res; },
                            _ => ()
                        }
                        deffered_res = 0.0;
                        op = Default::default();
                        deffered_op = match c {
                            '+' => Some(Op::Plus),
                            '-' => Some(Op::Minus),
                            _ => None
                        };
                      },
                      CalcState::Val => {
                        state = CalcState::Oper;
                        let val : String = buf_var[0..name_pos].iter().collect();
                        if let Ok(val) = val.parse::<f64>() {
                            match op {
                                Op::Div => {
                                    if val == 0.0 {
                                        return Err((CalcErrCause::DZero, pos))
                                    }
                                    deffered_res /= val},
                                Op::Mul => deffered_res *= val,
                                Op::Plus => deffered_res += val,
                                Op::Minus => deffered_res -= val,
                            }
                           // println!{"val {} dop {:?} = {} dfr {}", val, deffered_op, res, deffered_res};
                        } else {
                            return Err((CalcErrCause::NaN, pos))
                        }
                        name_pos = 0;
                        match deffered_op {
                            Some(Op::Plus) | None =>  { res += deffered_res; },
                            Some(Op::Minus) => { res -= deffered_res; },
                            _ => ()
                        }
                        deffered_res = 0.0;
                        op = Default::default();
                        deffered_op = match c {
                            '+' => Some(Op::Plus),
                            '-' => Some(Op::Minus),
                            _ => None
                        };
                      },
                      CalcState::Exp => {
                        // TODO figure why no state change in exp
                        state = CalcState::Oper;
                        match op {
                            Op::Div => {
                                if exp_val == 0.0 {
                                    return Err((CalcErrCause::DZero, pos))
                                }
                                deffered_res /= exp_val},
                            Op::Mul => deffered_res *= exp_val,
                            Op::Plus => deffered_res += exp_val,
                            Op::Minus => deffered_res -= exp_val,
                        };
                        name_pos = 0;
                        match deffered_op {
                            Some(Op::Plus) | None =>  { res += deffered_res; },
                            Some(Op::Minus) => { res -= deffered_res; },
                            _ => ()
                        }
                        deffered_res = 0.0;
                        op = Default::default();
                        deffered_op = match c {
                            '+' => Some(Op::Plus),
                            '-' => Some(Op::Minus),
                            _ => None
                        };
                      },
                      _ => todo!("state: {:?}", state)
                    }
                },
                '0'..='9' | '.' => {
                    if state == CalcState::Skip {
                        state = prev_state;
                    }
                    match state {
                        CalcState::Start | CalcState::Oper | CalcState::Exp => {
                            state = CalcState::Val;
                            buf_var [name_pos] = c;
                            name_pos += 1;
                     },  
                        CalcState::Var | CalcState::Val => {
                            buf_var [name_pos] = c;
                            name_pos += 1;
                        },
                        _ => todo!("state: {:?}", state)
                      }
                },
               '(' => { 
                    if state == CalcState::Skip {
                        state = prev_state
                    }
                    match state {
                        CalcState::Start | CalcState::Oper => {
                            sub_res_vec.push((res,deffered_res,op,deffered_op));
                            res = Default::default();
                            deffered_res = Default::default();
                            op = Default::default();
                            deffered_op = Default::default()
                        },
                        _ => return Err((CalcErrCause::InvPar, pos))
                    }
                },
                ')' => { 
                    if state == CalcState::Skip {
                        name_pos = last_non_blank;
                        state = prev_state
                    }
                    match state {
                        CalcState::Var => {
                            let var : String = buf_var[0..name_pos].iter().collect();
                            if let Some(val) = self.search_up( &var) {
                                if let Ok(val) = val.value.parse::<f64>() {
                                    match op {
                                        Op::Div => {
                                            if val == 0.0 {
                                                return Err((CalcErrCause::DZero, pos))
                                            }
                                            deffered_res /= val},
                                        Op::Mul => deffered_res *= val,
                                        Op::Plus => deffered_res += val,
                                        Op::Minus => deffered_res -= val,
                                    };
                                    match deffered_op {
                                        Some(Op::Plus) =>  { res += deffered_res; },
                                        Some(Op::Minus) => { res -= deffered_res; },
                                        _ => ()
                                    }
                                   // println!{"=val {} dop {:?} = {} old {}", val, deffered_op, res, deffered_res};
                                } else {
                                    return Err((CalcErrCause::NaN, pos))
                                }
                            } else {
                                return Err((CalcErrCause::NVar, pos))
                            }
                        },
                        CalcState::Val => {
                                    let val : String = buf_var[0..name_pos].iter().collect();
                                    if let Ok(val) = val.parse::<f64>() {
                                        
                                        match op {
                                            Op::Div => {
                                                if val == 0.0 {
                                                    return Err((CalcErrCause::DZero, pos))
                                                }
                                                deffered_res /= val},
                                            Op::Mul => deffered_res *= val,
                                            Op::Plus => deffered_res += val,
                                            Op::Minus => deffered_res -= val,
                                        }
                                        match deffered_op {
                                            Some(Op::Plus) =>  { res += deffered_res; },
                                            Some(Op::Minus) => { res -= deffered_res; },
                                            _ => ()
                                        }
                                        //println!{"=val {} dop {:?} = {} old {} op {:?}", val, deffered_op, res, deffered_res, op};
                                    } else {
                                        return Err((CalcErrCause::NaN, pos))
                                    }
                        },
                        _ => return Err((CalcErrCause::InvPar, pos))
                    };
                    let prev = sub_res_vec.pop();
                    if let Some(prev) = prev {
                        state = CalcState::Exp;
                        exp_val = res;
                        res = prev.0;
                        deffered_res = prev.1;
                        op = prev.2;
                        deffered_op = prev.3
                    } else {
                        return Err((CalcErrCause::CntPar, pos))
                    }
                },
                 _ => { // other chars
                    if state == CalcState::Skip {
                        
                        state = prev_state
                    }
                        match state {
                            CalcState::Start | CalcState::Oper => {
                                state = CalcState::Var;
                                buf_var [name_pos] = c;
                                name_pos += 1;
                         },  
                            CalcState::Var | CalcState::Val => {
                                buf_var [name_pos] = c;
                                name_pos += 1;
                            },
                            
                            _ => todo!("state: {:?}", state)
                          }
                }
            }
        }
        if state == CalcState::Skip {
            name_pos = last_non_blank;
            state = prev_state
        }
        match state {
            CalcState::Start => (),  
            CalcState::Var => {
                let var : String = buf_var[0..name_pos].iter().collect();
                //println!{"st {state:?} var {var}"};
                if let Some(val) = self.search_up( &var) {
                    if let Ok(val) = val.value.parse::<f64>() {
                        exp_val = val
                    } else {
                        return Err((CalcErrCause::NaN, pos))
                    }
                } else {
                    return Err((CalcErrCause::NVar, pos))
                }
            },
            CalcState::Val => {
                let val : String = buf_var[0..name_pos].iter().collect();
                if let Ok(val) = val.parse::<f64>() {
                    exp_val = val
                } else {
                    return Err((CalcErrCause::NaN, pos))
                }
            },
            CalcState::Exp => (),
            _ => todo!("state: {:?}", state)
        }
        match state {
            CalcState::Exp | CalcState::Val | CalcState::Var => {
                match op {
                    Op::Div => {
                        if exp_val == 0.0 {
                            return Err((CalcErrCause::DZero, pos))
                        }
                        deffered_res /= exp_val},
                    Op::Mul => deffered_res *= exp_val,
                    Op::Plus => deffered_res += exp_val,
                    Op::Minus => deffered_res -= exp_val,
                };
                match deffered_op {
                    Some(Op::Plus) | None =>  { res += deffered_res; },
                    Some(Op::Minus) => { res -= deffered_res; },
                    _ => ()
                }
            }
            _ => ()
        }
        Ok(res)
    }
}

pub fn run(log: &Log, block: GenBlockTup, targets: &mut Vec<String>) -> Result<(),Box<dyn Error>> {
    block.exec(log, &None); // execute Main
    if targets.is_empty() { 
        let mut tar_name : Option<String> = None;
        for ch in &block.borrow().children.clone() {
            let ch_block = ch.0.borrow();
            if ch_block.block_type == BlockType::Target {
                tar_name = Some(ch_block.name.as_ref().unwrap().to_string())
            }
        }
        let Some(tar_name) = tar_name else {
            return Err(Box::new("No targets found in the script".red()))
        };
        targets.push(tar_name)
    }
    log.log(&format!("targets: {:?}", targets));
    'targets: for target in targets {
        log.log(&format!("processing for '{}' of {}", target, block.borrow().children.len()));
        let children = block.borrow().children.clone();
        for bl in children {
           // let clone_bl = bl.clone();
            let ch_block = bl.0.borrow();
            if ch_block.block_type == BlockType::Target && ch_block.name.as_ref().unwrap() == target { 
                drop(ch_block);
                log.log(&format!("target: {}", exec_target(log, & bl)));
                continue 'targets
            }
        }
        let target = target.clone().bold();
        return Err(format!("No target '{target}' found").red().into())
    }
    
    Ok(())
}

pub fn exec_target(log: &Log, target_bl: & GenBlockTup) -> bool {
    // dependencies
    let mut need_exec = false;
    
    let gl_cwd = target_bl.search_up(CWD);
    let mut target = target_bl.borrow_mut();
    let dir = target.dir.clone();
    log.debug(&format!("processing: {} deps of {:?} in {dir:?}", &target.deps.len(), &target.name));
    if let Some(dir) = dir {
        let dir_val = dir.to_string();
        if !dir_val.is_empty() {
            // dir can't include ${xxx} for now, because ${ is considered as a block start
            //let mut dir = *process_template_value(log, &dir_val, &target, &None);
            let mut dir = match target.search_up(&dir_val) {
                None => dir_val,
                Some(var) => var.value
            };
            // calculate it upon current cwd
            if !has_root(&dir) && let Some(cwd) = gl_cwd {
                dir = cwd.value + std::path::MAIN_SEPARATOR_STR + &dir
            }
            let path =  Path::new(&dir);
            if path.exists() {
                let cwd = crate::util::normalize_path(path).display().to_string() ;
                target.vars.insert(String::from(CWD),  VarVal::from_string(cwd));
            } else {
                log.error(&format!{"The target directory {path:?} doesn't exist"})
            }
        }
    }
    drop(target);
    let target = target_bl.borrow();
    for dep in &target.deps {
        need_exec |= dep.eval_dep(log, &None)
    }
    if !need_exec && target.search_up("~force-build-target~").is_some() {
        need_exec = true
    }
    
    if need_exec {
        let mut res = None;
        let children = target.children.clone();
        drop(target);
        for child in children {
            res = child.exec(log, &res)
        }
    } else {
        log.debug(&format!("no need to run: {:?}", &target_bl.borrow().name))
    }
    need_exec
} 

fn no_parameters(fun: &GenBlock) -> bool {
    fun.block_type == BlockType::Function && fun.params.len() < 2 && (fun.params.is_empty() || 
        fun.params.len() == 1 && fun.params[0].is_empty())
}

pub fn timestamp(p: &str) -> Option<String> {
    Some(format_system_time(fs::metadata(p).ok()?.modified().ok()?))
}

pub fn format_system_time(time: SystemTime) -> String {
    let dur = time.duration_since(SystemTime::UNIX_EPOCH).unwrap();
    let (y,m,d,h,min,s,_w) = time:: get_datetime(1970, dur.as_secs());
    //println!{"week {} - {}", w, DAYS_OF_WEEK[w as usize]} ;
    format!("{:0>2}{:0>2}{:0>2}T{:0>2}{:0>2}{:0>2}Z", y,m,d,h,min,s) // see ISO 8601
}

pub fn exec_anynewer(block:&GenBlockTup, p1: &String, p2: &String) -> bool {
    let Some(cwd) = block.search_up(CWD) else {
        // unlikely ~cwd~ isn't set
        return newest(p1) > newest(p2)
    };
    
    let t1 = if has_root(p1) {newest(p1)} else {
        newest(&(cwd.value.clone() + MAIN_SEPARATOR_STR + p1))
    };
    let t2 = if has_root(p2) {newest(p2)} else {
        newest(&(cwd.value + MAIN_SEPARATOR_STR + p2))
    };
    //println!{"modified {:?} and {:?}", t1, t2};
    t1 > t2
}

fn dir_ext_param(parameter: &str) -> (Option<String>,Option<String>) {
    let path_end = parameter.rfind('/');
    if path_end.is_none() {
        return (None,None)
    }
    let pos = path_end.unwrap();
    let path = &parameter[0..pos];
    if pos == parameter.len() {
        return (Some(path.to_string()),None)
    }   
    let ext = &parameter[pos+1..];
    (Some(path.to_string()),Some(ext.to_string()))
}

// TODO implement as pushing in passing through vector
fn find_newer(dir1: &str, ext1: &str, dir2 : &Option<String>, ext2 : &Option<String>) -> Vec<String> {
    let mut result = Vec::new();
        
    let paths = fs::read_dir(dir1);
    if paths.is_err() {
        return result
    }
    //println!{"find newerthen: {:?}/{:?} then {:?}/{:?}", &dir1, &ext1, &dir2, &ext2};
    let dir = paths.unwrap();
    for file1 in dir {
        let file1_path = &file1.as_ref().unwrap().path().into_os_string().into_string().unwrap();
        let file1_name = &file1.as_ref().unwrap().file_name().into_string().unwrap();
        if file1.unwrap().file_type().unwrap().is_dir() {
            let file2_str = dir2.as_ref().map(|file2| format!{"{}/{}", file2, file1_name});
            result = [result, find_newer(file1_path, ext1, &file2_str, ext2)].concat();
        } else if file1_name.ends_with(ext1) {
            match dir2 {
                Some(dir2) => {
                    
                    let file2 = format!{"{}/{}{}", &dir2, &file1_name[0..file1_name.len()-ext1.len()], &ext2.as_ref().unwrap()};
                    
                    let t1 = last_modified(file1_path);
                    let t2 = last_modified(&file2);
                    //println!{"comparing: {:?}:{:?}<>{:?}:{:?}", &file1_path, &t1, &file2, &t2};
                    if t2.is_none() || t1.unwrap() > t2.unwrap() {
                        //println!{"none or newer: {:?}>{:?}", t1.unwrap() ,t2.unwrap_or(std::time::UNIX_EPOCH) };
                        result.push(file1_path.to_string())
                    }
                },
                None => result.push(file1_path.to_string())
            }
        }
    }
   // println!{"newer: {:?}", result};
    result
}

pub fn newest(mask : &str) -> Option<SystemTime> {
    //println!{"find newest in {mask}"}
    let path1 = Path::new(mask);
    let parent1 = path1.parent().unwrap(); // can be empty, check
    // check if the parent is '*' (wildcard) and if so,
    // set traverse flag and and to get a parent again
    let name1 = path1.file_name().unwrap();
    let str_name1 = name1.to_str().unwrap();
    let pos1 = str_name1.find('*'); // TODO add checking for more *
    if let Some(pos) = pos1 {
        let mut last: Option<SystemTime> = None;
        let dir = fs::read_dir(parent1).ok()?;
        for entry in dir {
            let entry = entry.unwrap();
            let path = entry.path();
            if path.is_file() { 
                  if let Some(path1) = path.file_name() && let Some(file_path) = path1.to_str() &&
                      (str_name1.len() == 1 || 
                     (pos == 0 && file_path.ends_with(&str_name1[1..])) ||
                     (pos == str_name1.len()-1 && file_path.starts_with(&str_name1[0..pos])) ||
                     (file_path.starts_with(&str_name1[0..pos]) && file_path.ends_with(&str_name1[pos+1..]) && file_path.len() >= str_name1.len())) {
                        let current_last = last_modified(&path.into_os_string().into_string().unwrap());
                        match last {
                            None => last = current_last,
                            Some(time) => {
                                if let Some(time2) = current_last && time2 > time {
                                    last = current_last;
                                }
                            }
                        }
                  }
             } else {
                let dir_entry_path = entry.path().into_os_string().into_string().unwrap();
                // maybe entry.join("str_name1")
                let last_dir = newest(&format!{"{}{}{str_name1}", dir_entry_path, std::path::MAIN_SEPARATOR}) ;
                //let last_dir = newest(&entry.join(str_name1))
                match last {
                    None => last = last_dir,
                    Some(time) => {
                        if let Some(time2) = last_dir && time2 > time {
                             last = last_dir
                        }
                    }
                }
            }
        } 
        last
    } else {
        last_modified(path1.to_str().unwrap())
    }
}

pub fn last_modified(file: &str) -> Option<SystemTime> {
    fs::metadata(file).ok()?.modified().ok()
}

fn matches(name: &str, filter: &str) -> bool {
    /*! - the function checks if a name matches to the filter with a possible wild card */
    let star_pos = filter.find('*');
    match star_pos {
        None=> {
            name == filter
        },
        Some (pos)=> {
            let len = name.len();
            match pos {
                0 => name.ends_with(&filter[1..]),
                last if last == len - 1 => name.starts_with(&filter[0..last]),
               _  => {
                    let start = &filter[0..pos];
                    let end = &filter[pos+1..];
                    name.starts_with(start) && name.ends_with(&end)
               }
            }
        } 
    }
}

fn zip_dir (log: &Log, zip: &mut simzip::ZipInfo, dir: &Path, path:Option<&str>, mask_start: Option<&str>, mask_end: Option<&str>) {
    //println!("zipping {dir:?} in {path:?} {mask_start:?}-{mask_end:?}");
    if let Ok(dir) = dir.read_dir()  {
        for entry in dir {
            if let Ok(entry) = entry && let Ok(file_type) = entry.file_type() { 
                let name = entry.file_name().to_str().unwrap().to_owned();
                if file_type.is_file() {
                    if (mask_start.is_some() && name.starts_with(mask_start.unwrap()) &&
                        mask_end.is_some() && name.ends_with(mask_end.unwrap()) ||
                    mask_start.is_none() && mask_end.is_some() && name.ends_with(mask_end.unwrap()) ||
                    mask_start.is_some() && name.starts_with(mask_start.unwrap()) && mask_end.is_none() ||
                    mask_start.is_none() && mask_end.is_none())
                        && !zip.add(simzip::ZipEntry::from_file(entry.path().as_os_str().to_str().unwrap(), path.map(str::to_string).as_ref())) {
                        log.warning(&format!{"Zip entry {1:?}/{0} already exists", &entry.path().as_os_str().to_str().unwrap(), &path})
                    }
                } else if file_type.is_dir() {
                    let zip_path = match path {
                        None => name,
                        Some(path) => path.to_owned() + "/" + &name
                    };
                    zip_dir(log, zip, &entry.path(), Some(&zip_path), mask_start, mask_end)
                }   
            }               
        }
    }
}

fn fill_dir( res: &mut Vec<String>, dir: &Path, start: &Option<&str>, end: &Option<&str>, subdir: bool, dir_name: bool) {
    if let Ok(dir) = dir.read_dir() {
        for entry in dir.flatten() {
             if let Ok(entry_type) = entry.file_type() {
                 let name = entry.file_name().to_str().unwrap().to_owned();
                 let accept = !start.is_none() && name.starts_with(start.unwrap()) &&
                     !end.is_none() && name.ends_with(end.unwrap()) ||
                 start.is_none() && !end.is_none() && name.ends_with(end.unwrap()) ||
                 !start.is_none() && name.starts_with(start.unwrap()) && end.is_none() ||
                 start.is_none() && end.is_none(); 
                 if entry_type.is_file() {
                     if accept {res.push(entry.path().into_os_string().into_string().unwrap())}
                 } else if entry_type.is_dir() {
                     if subdir {
                         fill_dir(res, &entry.path(), start, end, subdir, dir_name)
                     }
                     if dir_name && accept {
                         res.push(entry.path().into_os_string().into_string().unwrap())
                     }
                 }
             }
        }
    }
}

