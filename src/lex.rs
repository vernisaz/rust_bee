// lex analizer
use std::{fs::File,
          io::{self, Read},
          collections::HashMap,
          env,
          cell::RefCell,
          rc::Rc,
         };
use log::Log;
use fun::{GenBlock, BlockType, GenBlockTup};
use fun::PREV_VAL;
use get_property;
use util::{has_root, vec_to_str};

const BUF_SIZE: usize = 256;

const MAX_LEX_LEN: usize = 16_384;

#[allow(dead_code)]
#[derive(Debug, Clone, PartialEq, Default)]
pub enum VarType {
    #[default]
    Generic,
    Property,
    Directory,
    Path,
    Array,
    File,
    Environment,
    Number,
    Date,
    Bool,
    Eval,
    Function,
    Url,
    RepositoryMaven,
    RepositoryRust
}

#[allow(dead_code)]
#[derive(PartialEq, Debug)]
pub enum Lexem {
    Variable(String), 
    Value(String), 
    Comment(String),
    Type(String),
    Range(usize, usize),
    Function(String),
    Parameter(String), // potential Option<Vec<Lexem>> can be attached as the parameter comments
    BlockHdr(String),
    BlockEnd(Option<String>),
    EOF
}

#[allow(dead_code)]
#[derive(PartialEq, Debug, Copy, Clone)]
enum LexState {
    Begin,
    QuotedStart,
    InLex,
    InQtLex,
    InQtValue,
    Escape,
    EscapeValue,  // --> EscapeQtValue
    EscapeBreakValue,
    BlankOrEnd,
    RangeStart,
    Comment,
    InType,
    StartValue,
    InValue,
    RangeEnd,
    InParam,
    InParamBlank,
    InQtParam,
    StartParam,
    EndFunction,
    IgnoredBlankToEnd,
    BlankInValue,
    BlockStart,
    BlockEnd,
    EscapeParam,
    EscapeQtParam, 
    EndQtParam,
    InBreak,
    InArrayVal,
    EscapeEndArray,
    End,
    UnrecoverableErr
}

#[allow(dead_code)]
#[derive(PartialEq, Debug, Copy, Clone)]
enum TemplateState {
    InVal,
    VarStart,  // $
    LeftBrack,
    RightBrack,
    InVar,
}

#[derive(PartialEq, Debug, Copy, Clone)]
enum HdrState {
    InType,
    NameStart,  // $
    WorkDiv,
    PathDiv,
    InName,
    InPath,
    InWork,
    InNameBlank,
    InWorkBlank,
    InPathBlank,
    InNameQt,
    InPathQt,
    InWorkQt,
}
 
#[derive(Debug, Clone)]
pub struct VarVal {
    pub val_type: VarType,
    pub value: String, // TODO make it enum based on type
    pub values: Vec<String>, // TODO make it Option<Vec<String>>
}

pub struct Reader {
    buf: [u8;BUF_SIZE],
    pos: usize,
    end: usize,
    line: u32,
    line_offset: u16,
    reader: File,
}

impl VarVal {
    pub fn from_string(str: impl Into<String>) -> VarVal {
        VarVal{val_type: VarType::Generic, value: str.into(), values: Vec::new()}  
    }

    pub fn from_bool(boole: bool) -> VarVal {
        VarVal{val_type: VarType::Bool, value: if boole {"true".to_string()} else {"false".to_string()}, values: Vec::new()}  
    }

    pub fn from_i32(number: i32) -> VarVal {
        VarVal{val_type: VarType::Number, value: format!{"{}", number}, values: Vec::new()}  // 
    }

    pub fn from_f64(number: f64) -> VarVal {
        VarVal{val_type: VarType::Number, value: format!{"{number}"}, values: Vec::new()}  // 
    }

    pub fn from_vec(vec: &Vec<String>) -> VarVal {
        VarVal{val_type: VarType::Array, value: "".to_string(), values: vec.clone()}  
    }

    pub fn is_true(& self) -> bool {
        match self.val_type {
            VarType::Environment  => {
                match env::var(self.value.to_string()) {
                    Ok(val) => val == "true",
                    Err(_e) => self.value == "true"
                }
            },
            VarType::Property => {
                if let Some(val) = get_property(&self.value) {
                    val == "true"
                } else {
                    self.value == "true"
                }
            },
            VarType::Array => self.values.iter().any(|current| !current.is_empty()),
            VarType::Number => !self.value.is_empty() && self.value.parse::<i32>().unwrap_or_default() != 0,
            _ => self.value == "true" // consider adding interpolation
        }
    }
}

impl Default for VarVal {
    fn default() -> Self {
        VarVal::from_bool(false)
    }
}

impl Reader {
// can use fs:read_to_string to get an entire file in a string, can be simpler
    fn next(&mut self) -> Option<char> {
        self.pos += 1;
        if self.pos >= self.end {
            self.end = self.reader.read(&mut self.buf).unwrap();
            
            match self.end {
               0 =>  return None,
               _ => ()
            }
            self.pos = 0;
        }
        self.line_offset += 1;
        // check if it can be UTF8
        let mut byte : u32 = self.buf[self.pos] as u32;
        if (byte & 0b1000_0000) != 0 { // UTF8
            let mut num_byte = 
                if (byte & 0b1111_0000) == 0b1111_0000 {
                    byte &= 0b0000_0111; 3
                } else if (byte & 0b1110_0000) == 0b1110_0000 {
                    byte &= 0b0000_1111; 2
                } else if (byte & 0b1100_0000) == 0b1100_0000 {
                    byte &= 0b0001_1111; 1
                } else {0};

            let mut c32 : u32 = byte;
            while num_byte > 0 {
                self.pos += 1;
                if self.pos >= self.end {
                    self.end = self.reader.read(&mut self.buf).unwrap();
                    if self.end == 0 {
                        return None
                    }
                    self.pos = 0;
                }
                //println!("b-{:x}", c32);
                c32 =  (c32 << 6) | ((self.buf[self.pos] as u32) & 0b0011_1111);
                num_byte -= 1
            }
            //println!("{:x}", c32);
            return Some(std::char::from_u32(c32).unwrap_or(std::char::REPLACEMENT_CHARACTER))
        }
        Some(char::from(self.buf[self.pos]))
    }
}

fn open(file: &str) -> io::Result<Reader> {

    Ok(Reader {
        reader : File::open(file)?,
        line : 1,
        buf : [0; 256],
        pos : 0,
        end : 0,
        line_offset : 0,
    })
}

fn read_lex(log: &Log, reader: &mut Reader, mut state: LexState) -> (Lexem, LexState, u32) {
    let mut buffer : [char; MAX_LEX_LEN] = [' '; MAX_LEX_LEN];
    
   // let mut buffer = String::with_capacity(MAX_LEX_LEN);
    let mut buf_fill: usize = 0;
    let mut last_nb = 0;
    let mut c1 = reader.next();
    let mut prev_state = LexState::Begin;
    let mut prev_buffer : [char; MAX_LEX_LEN] = [' '; MAX_LEX_LEN];
   // let mut prev_buffer = String::with_capacity(MAX_LEX_LEN);
    let mut buf_prev_fill : usize = 0;
    while let Some(c) = c1 {
        match c {
            '"' => {
                match state {
                    LexState::Begin => state = LexState::QuotedStart,
                    LexState::InLex | LexState::InParam | LexState::InArrayVal => {
                        buffer[buf_fill] = c;
                        buf_fill += 1;
                    },
                    LexState::InQtLex => {
                        //let lexstr: String = buffer[0..buf_fill].iter().collect();
                        last_nb = buf_fill;
                        state = LexState::IgnoredBlankToEnd;
                    },
                    LexState::InQtValue => {
                        state = LexState::InValue;
                        last_nb = buf_fill;
                        //last_nb = buffer.chars().count()
                        //return (Lexem::Value(buffer[0..buf_fill].iter().collect()), state);
                    },
                    LexState::Escape => {
                        state = LexState::InQtLex ;
                        buffer[buf_fill] = c;
                        buf_fill += 1;
                    },
                    LexState::EscapeValue => {
                        state = LexState::InQtValue ;
                        buffer[buf_fill] = c;
                        buf_fill += 1;
                    }
                    LexState::EscapeParam => {
                        state = LexState::InParam ;
                        buffer[buf_fill] = c;
                        buf_fill += 1;
                    },
                    LexState::InQtParam => {
                        state = LexState::InParam;
                    }
                    LexState::StartParam => {
                        state = LexState::InQtParam;
                    },
                    LexState::StartValue => {
                        state = LexState::InQtValue;
                    },
                    LexState::Comment | LexState::BlankOrEnd => {
                        buffer[buf_fill] = c;
                        buf_fill += 1;
                    },
                    LexState::EscapeBreakValue => {
                        buffer[buf_fill] = '\\';
                        buf_fill += 1;
                        buffer[buf_fill] = c;
                        buf_fill += 1;
                        state = LexState::InValue;
                    },
                    LexState::EscapeEndArray => {
                        buffer[buf_fill] = '\\';
                        buf_fill += 1;
                        buffer[buf_fill] = c;
                        buf_fill += 1;
                        state = LexState::InArrayVal
                    },

                    LexState::InBreak => {
                        buffer[buf_fill] = c;
                        buf_fill += 1; 
                        state = LexState::InValue;
                    },
                    LexState::EscapeQtParam => {
                        buffer[buf_fill] = c;
                        buf_fill += 1; 
                        state = LexState::InQtParam;
                    }
                    _ => todo!("state: {:?} at {}", state, reader.line)
                }
                
            },
            ' ' | '\t' => {
                match state {
                    LexState::Begin | LexState::BlockStart => (),
                    LexState::QuotedStart => {
                        buffer[buf_fill] = c;
                        buf_fill += 1;
                        state = LexState::InQtLex;
                    },
                    LexState::InLex => {
                        state = LexState::BlankOrEnd;
                        last_nb = buf_fill;
                        buffer[buf_fill] = c;
                        buf_fill += 1;
                    },
                    LexState::Escape => {
                        state = LexState::InLex;
                        buffer[buf_fill] = c;
                        buf_fill += 1;
                    },
                    LexState::InParamBlank | LexState::InQtParam | LexState::InQtValue | LexState::InArrayVal |
                     LexState::BlankOrEnd | LexState::Comment | LexState::BlankInValue => {
                        buffer[buf_fill] = c;
                        buf_fill += 1;
                    },
                    LexState::InParam => {
                        state = LexState::InParamBlank;
                        last_nb = buf_fill;
                        buffer[buf_fill] = c;
                        buf_fill += 1;
                    },
                    LexState::InType => {
                        state = LexState::Begin;
                        return (Lexem::Type(buffer[0..buf_fill].iter().collect()), state, reader.line); // TODO add offset
                    },
                    LexState::EndFunction => state = LexState::Begin,
                    LexState::StartValue | LexState::BlockEnd | LexState::IgnoredBlankToEnd => {

                    },
                    LexState::InValue => {
                        state = LexState::BlankInValue;
                        last_nb = buf_fill;
                        buffer[buf_fill] = c;
                        buf_fill += 1;
                    },
                    LexState::StartParam => {

                    },
                    LexState::EscapeValue => {
                        buffer[buf_fill] = '\\';
                        buf_fill += 1;
                        buffer[buf_fill] = c;
                        buf_fill += 1;
                        state = LexState::InQtValue;
                    },
                    LexState::EscapeBreakValue => {
                        buffer[buf_fill] = '\\';
                        buf_fill += 1;
                        last_nb = buf_fill;
                        buffer[buf_fill] = c;
                        buf_fill += 1;
                        state = LexState::BlankInValue;
                    },
                    LexState::InBreak => {
                        buffer[buf_fill] = c;
                        buf_fill += 1; 
                        state = LexState::InValue;
                    },
                    LexState::EscapeEndArray => {
                        buffer[buf_fill] = '\\';
                        buf_fill += 1;
                        buffer[buf_fill] = c;
                        buf_fill += 1;
                        state = LexState::InArrayVal
                    },
                    _ => todo!("state: {:?} at {}", state, reader.line)
                }

            },
            '\\' => {
                match state {
                    LexState::InQtLex | LexState::QuotedStart => state = LexState::Escape,
                    LexState::InParam => state = LexState::EscapeParam,
                    LexState::InQtValue => state = LexState::EscapeValue,
                    LexState::InQtParam => state = LexState::EscapeQtParam,
                    LexState::Escape => {
                        state = LexState::InLex; // was InQtLex
                        buffer[buf_fill] = c;
                        buf_fill += 1;
                    },
                    LexState::InLex => {
                        state = LexState::Escape
                    }
                    LexState::Begin => {
                        state = LexState::InLex;
                        buffer[buf_fill] = c;
                        buf_fill += 1;
                    },
                    LexState::BlankOrEnd => {
                        state = LexState::InLex;
                    },
                    LexState::StartParam => {
                        state = LexState::EscapeParam;
                    }
                    LexState::InParamBlank => {
                        state = LexState::InParam;
                        buffer[buf_fill] = c;
                        buf_fill += 1;
                    },
                    LexState::Comment => {
                        buffer[buf_fill] = c;
                        buf_fill += 1;
                    },
                    LexState::EscapeValue => {
                        buffer[buf_fill] = '\\';
                        buf_fill += 1;
                        buffer[buf_fill] = c;
                        buf_fill += 1;
                        state = LexState::InQtValue;
                    },
                    LexState::EscapeParam => {
                        buffer[buf_fill] = c;
                        buf_fill += 1;
                        state = LexState::InParam;
                    },
                    LexState::InValue => {
                        state = LexState::EscapeBreakValue;
                    },
                    LexState::EscapeBreakValue | LexState::EscapeEndArray => {
                        buffer[buf_fill] = '\\';
                        buf_fill += 1;
                    },
                    LexState::InBreak => {
                        buffer[buf_fill] = c;
                        buf_fill += 1; 
                        state = LexState::InValue;
                    },
                    LexState::InArrayVal => {
                        state = LexState::EscapeEndArray
                    },
                    LexState::End => break,
                    _ => todo!("state: {:?} at {}", state, reader.line)
                }
            },
            '#' => {
                match state {
                    LexState::Begin | LexState::EndFunction | LexState::Comment 
                        | LexState::BlockStart | LexState::BlockEnd => {
                        state = LexState::Comment;
                        buffer[buf_fill] = c;
                        buf_fill += 1;
                    },
                    LexState::InValue | LexState::StartValue => { // separate in value since # has to be collected toward to comment
                        state = LexState::Comment;
                        return (Lexem::Value(buffer[0..buf_fill].iter().collect()), state, reader.line);
                    },
                    LexState::BlankInValue | LexState::InBreak | LexState::BlankOrEnd => { // separate in value since # has to be collected toward to comment
                        state = LexState::Comment;
                        return (Lexem::Value(buffer[0..last_nb].iter().collect()), state, reader.line);
                    },
                    LexState::InQtValue | LexState::InQtParam | LexState::InQtLex => {
                        buffer[buf_fill] = c;
                        buf_fill += 1;
                    },
                    LexState::EscapeValue => {
                        buffer[buf_fill] = '\\';
                        buf_fill += 1;
                        buffer[buf_fill] = c;
                        buf_fill += 1;
                        state = LexState::InQtValue
                    }
                    LexState::EscapeBreakValue => {
                        buffer[buf_fill] = '\\';
                        buf_fill += 1;
                        state = LexState::Comment;
                        return (Lexem::Value(buffer[0..buf_fill].iter().collect()), state, reader.line);
                    },
                    LexState::InArrayVal | LexState::StartParam | LexState::InParam => {
                        prev_state = state ;
                        prev_buffer [0..buf_fill].clone_from_slice(&buffer[0..buf_fill]);
                        buf_prev_fill = buf_fill;
                        buf_fill = 0;
                        state = LexState::Comment;
                        buffer[buf_fill] = c;
                        buf_fill += 1; // here is no current val return and then no comment return
                        // probaby improve in future to collect and return comments
                    },
                    LexState::EscapeEndArray => {
                        buffer[buf_fill] = c;
                        buf_fill += 1;
                        state = LexState::InArrayVal
                    },
                    _ => todo!("state: {:?} at {}", state, reader.line)
                }
            },
            '\n' | '\r' => {
                if c == '\n' {
                    reader.line += 1;
                    reader.line_offset = 0;
                }
                match state {
                    LexState::Comment => {
                        state = prev_state; prev_state = LexState::Begin;
                        if state != LexState::InArrayVal {
                            return (Lexem::Comment(buffer[0..buf_fill].iter().collect()), state, reader.line);
                        } else {
                            let comment : String = buffer[0..buf_fill].iter().collect();
                            log.debug(&format!("Commentary: {}, line: {}/{}", comment, reader.line, reader.line_offset));
                        }
                        // perhaps just accumulate all the array comments here
                        buffer [0..buf_prev_fill].clone_from_slice(&prev_buffer[0..buf_prev_fill]);
                        buf_fill = buf_prev_fill;
                    },
                    LexState::Begin | LexState::BlockStart | LexState::StartParam | LexState::RangeStart => {
                    },
                    LexState::InValue | LexState::StartValue => {
                        state = LexState::Begin;
                        return (Lexem::Value(buffer[0..buf_fill].iter().collect()), state, reader.line);
                    },
                    LexState::BlankInValue => {
                        state = LexState::Begin;
                        return (Lexem::Value(buffer[0..last_nb].iter().collect()), state, reader.line);
                    },
                    LexState::EndFunction | LexState::BlockEnd => {
                        state = LexState::Begin; 
                    },
                    LexState::InType => {
                        state = LexState::Begin;
                        return (Lexem::Type(buffer[0..buf_fill].iter().collect()), state, reader.line);
                    },
                   LexState::InQtParam | LexState::InParamBlank | LexState::InQtValue  | LexState::InArrayVal => {
                        buffer[buf_fill] = c;
                        buf_fill += 1;
                    },
                    LexState::EscapeValue => {
                        buffer[buf_fill] = '\\';
                        buf_fill += 1;
                        buffer[buf_fill] = c;
                        buf_fill += 1;
                        state = LexState::InQtValue
                    }
                    LexState::InParam => {
                        state = LexState::InParamBlank;
                        last_nb = buf_fill;
                        buffer[buf_fill] = c;
                        buf_fill += 1;
                    },
                    LexState::EscapeEndArray => {
                        buffer[buf_fill] = '\\';
                        buf_fill += 1;
                        buffer[buf_fill] = c;
                        buf_fill += 1;
                        state = LexState::InArrayVal
                    },
                    LexState::InLex | LexState::BlankOrEnd => {
                        state = LexState::BlankOrEnd;
                    },
                    LexState::EscapeBreakValue | LexState::InBreak => {
                        state = LexState::InBreak;
                    },
                    _ => todo!("state: {:?} at {}", state, reader.line)
                }
            },
            '[' => {
                match state {
                    LexState::BlankOrEnd => state = LexState::RangeStart,
                    LexState::InQtLex | LexState::InQtValue | LexState::InQtParam | 
                    LexState::InArrayVal  => {
                        buffer[buf_fill] = c;
                        buf_fill += 1;
                    },
                    LexState::InLex => {
                        state = LexState::RangeStart;
                        
                        //let lexstr: String = buffer[0..buf_fill].iter().collect();
                        return (Lexem::Variable(buffer[0..buf_fill].iter().collect()), state, reader.line);
                    },
                    LexState::Comment | LexState::InValue | LexState::InParam => {
                        buffer[buf_fill] = c;
                        buf_fill += 1;
                    },
                    LexState::InParamBlank | LexState::EndFunction | LexState::StartParam=> {
                        state = LexState::InParam;
                        buffer[buf_fill] = c;
                        buf_fill += 1;
                    },
                    LexState::EscapeValue => {
                        buffer[buf_fill] = '\\';
                        buf_fill += 1;
                        buffer[buf_fill] = c;
                        buf_fill += 1;
                        state = LexState::InQtValue
                    }
                    LexState::EscapeParam => {
                        buffer[buf_fill] = '\\';
                        buf_fill += 1;
                        buffer[buf_fill] = c;
                        buf_fill += 1;
                        state = LexState::InParam;
                    },
                    LexState::StartValue => {
                        state = LexState::InArrayVal;
                        buffer[buf_fill] = c;
                        buf_fill += 1;
                    },
                    LexState::EscapeBreakValue => {
                        buffer[buf_fill] = '\\';
                        buf_fill += 1;
                        buffer[buf_fill] = c;
                        buf_fill += 1;
                        state = LexState::InValue;
                    },
                    LexState::InBreak => {
                        buffer[buf_fill] = c;
                        buf_fill += 1; 
                        state = LexState::InValue;
                    }
                    LexState::EscapeEndArray => {
                        buffer[buf_fill] = '\\';
                        buf_fill += 1;
                        buffer[buf_fill] = c;
                        buf_fill += 1;
                        state = LexState::InArrayVal
                    }
                    _ => todo!("state: {:?} at {}", state, reader.line)
                }
            },
            ']' => {
                match state {
                    LexState::Comment | LexState::InValue | LexState::InParam |
                    LexState::InQtLex | LexState::InQtValue | LexState::InQtParam 
                     => {
                        buffer[buf_fill] = c;
                        buf_fill += 1;
                    },
                    LexState::StartValue | LexState::InBreak => {
                        state = LexState::InValue;
                        buffer[buf_fill] = c;
                        buf_fill += 1;
                    },
                    LexState::EscapeBreakValue => {
                        buffer[buf_fill] = '\\';
                        buf_fill += 1;
                        buffer[buf_fill] = c;
                        buf_fill += 1;
                        state = LexState::InValue;
                    },
                    LexState::EscapeParam => {
                        buffer[buf_fill] = '\\';
                        buf_fill += 1;
                        buffer[buf_fill] = c;
                        buf_fill += 1;
                        state = LexState::InParam;
                    },
                    LexState::EscapeValue => {
                        buffer[buf_fill] = '\\';
                        buf_fill += 1;
                        buffer[buf_fill] = c;
                        buf_fill += 1;
                        state = LexState::InQtValue
                    }
                    LexState::EscapeEndArray => {
                        state = LexState::InArrayVal;
                        buffer[buf_fill] = c;
                        buf_fill += 1;
                    },
                    LexState::InArrayVal => {
                        state = LexState::Begin;
                        buffer[buf_fill] = c;
                        buf_fill += 1; 
                        // probably add type the array value and call process_array_value here first
                        return (Lexem::Value(buffer[0..buf_fill].iter().collect()), state, reader.line);
                    },
                    LexState::InType => {
                        // syntax error
                        log.error(&format!{"Unexpected symbol ] in type at {}:{}", reader.line, reader.line_offset});
                        state = LexState::UnrecoverableErr;
                        return (Lexem::EOF, state, reader.line);
                    },
                    _ => todo!("state: {:?} at {}", state, reader.line)
                }
            },
            '{' => {
                match state {
                    LexState::InValue | LexState::InQtParam | LexState::InQtLex | LexState::InParam | LexState::Comment | LexState::InQtValue | LexState::InArrayVal => {
                        buffer[buf_fill] = c;
                        buf_fill += 1;
                    },
                    LexState::InLex | LexState::BlankOrEnd => {
                        state = LexState::BlockStart;
                        return (Lexem::BlockHdr(buffer[0..buf_fill].iter().collect()), state, reader.line);
                    },
                    LexState::BlockEnd | LexState::Begin => {
                        state = LexState::BlockStart;
                        return (Lexem::BlockHdr("".to_string()), state, reader.line);
                    },
                    LexState::InParamBlank | LexState::StartParam => {
                        state = LexState::InParam;
                        buffer[buf_fill] = c;
                        buf_fill += 1;
                    },
                    LexState::BlockStart => {
                        return (Lexem::BlockHdr("".to_string()), state, reader.line);
                    },
                    LexState::EscapeValue => {
                        buffer[buf_fill] = '\\';
                        buf_fill += 1;
                        buffer[buf_fill] = c;
                        buf_fill += 1;
                        state = LexState::InQtValue
                    }
                    LexState::EscapeBreakValue => {
                        buffer[buf_fill] = '\\';
                        buf_fill += 1;
                        buffer[buf_fill] = c;
                        buf_fill += 1;
                        state = LexState::InValue;
                    },
                    LexState::InBreak => {
                        buffer[buf_fill] = c;
                        buf_fill += 1; 
                        state = LexState::InValue;
                    },
                    LexState::EscapeEndArray => {
                        buffer[buf_fill] = '\\';
                        buf_fill += 1;
                        buffer[buf_fill] = c;
                        buf_fill += 1;
                        state = LexState::InArrayVal
                    },
                    _ => todo!("state: {:?} at {}", state, reader.line)
                }
            },
            '}' => {
                //println!("{:?}", state);
                match state {
                    LexState::Begin | LexState::BlockStart | LexState::BlockEnd => {
                        state = LexState::BlockEnd;
                    
                        return (Lexem::BlockEnd(None), state, reader.line);
                    },
                    LexState::InParam | LexState::InValue | LexState::InQtParam | LexState::Comment |
                    LexState::InQtValue | LexState::InArrayVal => {
                        buffer[buf_fill] = c;
                        buf_fill += 1;
                    },
                    LexState::InParamBlank => {
                        state = LexState::InParam;
                        buffer[buf_fill] = c;
                        buf_fill += 1;
                    },
                    LexState::InLex | LexState::BlankOrEnd | LexState::EndFunction => {
                        state = LexState::BlockEnd;
                    // decide what to do with lex value ????
                        
                        return (Lexem::BlockEnd(Some(buffer[0..buf_fill].iter().collect())), state, reader.line);
                    },
                    LexState::EscapeValue => {
                        buffer[buf_fill] = '\\';
                        buf_fill += 1;
                        buffer[buf_fill] = c;
                        buf_fill += 1;
                        state = LexState::InQtValue
                    }
                    LexState::EscapeBreakValue => {
                        buffer[buf_fill] = '\\';
                        buf_fill += 1;
                        buffer[buf_fill] = c;
                        buf_fill += 1;
                        state = LexState::InValue;
                    },
                    LexState::InBreak => {
                        buffer[buf_fill] = c;
                        buf_fill += 1; 
                        state = LexState::InValue;
                    },
                    LexState::EscapeEndArray => {
                        buffer[buf_fill] = '\\';
                        buf_fill += 1;
                        buffer[buf_fill] = c;
                        buf_fill += 1;
                        state = LexState::InArrayVal
                    },
                    _ => todo!("state: {:?} at {}", state, reader.line)
                }
            },
            ';' => {
                match state {
                    LexState::EndFunction | LexState::BlockEnd | LexState::Begin => {
                        state = LexState::Begin; 
                    }, 
                    LexState::Comment | LexState::InParam | LexState::InQtParam |
                    LexState::InQtValue | LexState::InQtLex | LexState::InArrayVal => {
                        buffer[buf_fill] = c;
                        buf_fill += 1;
                    } ,
                    LexState::InParamBlank => {
                        state = LexState::InParam;
                        buffer[buf_fill] = c;
                        buf_fill += 1;
                    },
                    LexState::EscapeValue => {
                        buffer[buf_fill] = '\\';
                        buf_fill += 1;
                        buffer[buf_fill] = c;
                        buf_fill += 1;
                        state = LexState::InQtValue
                    }
                    LexState::EscapeBreakValue => {
                        buffer[buf_fill] = '\\';
                        buf_fill += 1;
                        state = LexState::Begin;
                        return (Lexem::Value(buffer[0..buf_fill].iter().collect()), state, reader.line);
                    },
                    LexState::InValue | LexState::InBreak | LexState::InLex => {
                        state = LexState::Begin;
                        return (Lexem::Value(buffer[0..buf_fill].iter().collect()), state, reader.line);
                    },
                    LexState::EscapeEndArray => {
                        buffer[buf_fill] = '\\';
                        buf_fill += 1;
                        buffer[buf_fill] = c;
                        buf_fill += 1;
                        state = LexState::InArrayVal
                    },
                    _ => todo!("state: {:?} at {}", state, reader.line)
                }
            },

            ':' => {
                match state {
                    LexState::BlankOrEnd | LexState::Begin => {
                        buffer[buf_fill] = c;
                        buf_fill += 1;
                        state = LexState::InLex;
                    },
                    LexState::InValue | LexState::BlankInValue => {
                        state = LexState::InType;
                        last_nb = buf_fill;
                        return (Lexem::Value(buffer[0..last_nb].iter().collect()), state, reader.line);
                    },
                    LexState::InParam | LexState::InLex | LexState::Comment | LexState::InQtParam |
                    LexState::InQtValue | LexState::InQtLex | LexState::InArrayVal => {
                        buffer[buf_fill] = c;
                        buf_fill += 1;
                    },
                    LexState::InParamBlank => {
                        state = LexState::InParam;
                        buffer[buf_fill] = c;
                        buf_fill += 1;
                    },
                    LexState::EscapeValue => {
                        buffer[buf_fill] = '\\';
                        buf_fill += 1;
                        buffer[buf_fill] = c;
                        buf_fill += 1;
                        state = LexState::InQtValue
                    }
                    LexState::EscapeBreakValue => {
                        buffer[buf_fill] = '\\';
                        buf_fill += 1;
                        buffer[buf_fill] = c;
                        buf_fill += 1;
                        state = LexState::InValue;
                    },
                    LexState::InBreak => {
                        buffer[buf_fill] = c;
                        buf_fill += 1; 
                        state = LexState::InValue;
                    },
                    LexState::EscapeEndArray => {
                        buffer[buf_fill] = '\\';
                        buf_fill += 1;
                        buffer[buf_fill] = c;
                        buf_fill += 1;
                        state = LexState::InArrayVal
                    },
                    _ => todo!("state: {:?} at {}", state, reader.line)
                }
            },
            '=' => {
                match state {
                    LexState::BlankOrEnd | LexState::IgnoredBlankToEnd => {
                        state = LexState::StartValue; 
                        return (Lexem::Variable(buffer[0..last_nb].iter().collect()), state, reader.line);
                    },
                    LexState::InLex => {
                        state = LexState::StartValue; 
                        return (Lexem::Variable(buffer[0..buf_fill].iter().collect()), state, reader.line);
                    },
                    LexState::Comment | LexState::InParam | LexState::InQtParam |
                    LexState::InQtValue | LexState::InQtLex | LexState::InArrayVal |
                    LexState::InValue => {
                        buffer[buf_fill] = c;
                        buf_fill += 1;
                    } ,
                    LexState::InParamBlank => {
                        state = LexState::InParam;
                        buffer[buf_fill] = c;
                        buf_fill += 1;
                    },
                    LexState::EscapeValue => {
                        buffer[buf_fill] = '\\';
                        buf_fill += 1;
                        buffer[buf_fill] = c;
                        buf_fill += 1;
                        state = LexState::InQtValue
                    }
                    LexState::EscapeEndArray => {
                        buffer[buf_fill] = '\\';
                        buf_fill += 1;
                        buffer[buf_fill] = c;
                        buf_fill += 1;
                        state = LexState::InArrayVal
                    },
                    _ => todo!("state: {:?} at {}", state, reader.line)
                }
            },
            '(' => { 
                match state {
                    LexState::InLex => {
                        state = LexState::StartParam; 
                        return (Lexem::Function(buffer[0..buf_fill].iter().collect()), state, reader.line);
                    },
                    LexState::BlankOrEnd => {
                        state = LexState::StartParam; 
                        return (Lexem::Function(buffer[0..buf_fill].iter().collect()), state, reader.line);
                    },
                    LexState::InValue | LexState::InParam | LexState::InQtParam | LexState::Comment | 
                    LexState::InQtValue | LexState::InQtLex | LexState::InArrayVal => {
                        buffer[buf_fill] = c;
                        buf_fill += 1;
                    },
                    LexState::StartParam => {
                        state= LexState::InParam;
                        buffer[buf_fill] = c;
                        buf_fill += 1;
                    },
                    LexState::Begin => {
                        state = LexState::InLex;
                        buffer[buf_fill] = c;
                        buf_fill += 1;
                    },
                    LexState::EscapeBreakValue => {
                        buffer[buf_fill] = '\\';
                        buf_fill += 1;
                        buffer[buf_fill] = c;
                        buf_fill += 1;
                        state = LexState::InValue;
                    },
                    LexState::EscapeValue => {
                        buffer[buf_fill] = '\\';
                        buf_fill += 1;
                        buffer[buf_fill] = c;
                        buf_fill += 1;
                        state = LexState::InQtValue
                    }
                    LexState::InBreak => {
                        buffer[buf_fill] = c;
                        buf_fill += 1; 
                        state = LexState::InValue;
                    },
                    LexState::EscapeEndArray => {
                        buffer[buf_fill] = '\\';
                        buf_fill += 1;
                        buffer[buf_fill] = c;
                        buf_fill += 1;
                        state = LexState::InArrayVal
                    },
                    _ => todo!("state: {:?} at {}", state, reader.line)
                }
            },
            ')' => {
                match state {
                    LexState::InParam  => {
                        state = LexState::EndFunction; 
                        return (Lexem::Parameter(buffer[0..buf_fill].iter().collect()), state, reader.line)
                    }
                    LexState::InParamBlank  => {
                        state = LexState::EndFunction; 
                        return (Lexem::Parameter(buffer[0..last_nb].iter().collect()), state, reader.line)
                    }
                    LexState::StartParam => {
                        state = LexState::EndFunction; 
                        return (Lexem::Parameter(buffer[0..buf_fill].iter().collect()), state, reader.line)
                    }
                    LexState::InValue | LexState::InQtParam | LexState::Comment |
                    LexState::InQtValue | LexState::InQtLex | LexState::InArrayVal 
                    | LexState::InLex => { // using ) has to be prohibited in lex
                        buffer[buf_fill] = c;
                        buf_fill += 1
                    }
                    LexState::Begin => {
                        state = LexState::InLex;
                        buffer[buf_fill] = c;
                        buf_fill += 1
                    }
                    LexState::EscapeBreakValue => {
                        buffer[buf_fill] = '\\';
                        buf_fill += 1;
                        buffer[buf_fill] = c;
                        buf_fill += 1;
                        state = LexState::InValue
                    }
                    LexState::EscapeValue => {
                        buffer[buf_fill] = '\\';
                        buf_fill += 1;
                        buffer[buf_fill] = c;
                        buf_fill += 1;
                        state = LexState::InQtValue
                    }
                    LexState::InBreak => {
                        buffer[buf_fill] = c;
                        buf_fill += 1; 
                        state = LexState::InValue
                    }
                    LexState::EscapeEndArray => {
                        buffer[buf_fill] = '\\';
                        buf_fill += 1;
                        buffer[buf_fill] = c;
                        buf_fill += 1;
                        state = LexState::InArrayVal
                    },
                    _ => todo!("state: {:?} at {}", state, reader.line)
                }
            },
            dig @ '0' ..= '9' => {
                match state {
                 LexState::InParam |LexState::InValue | LexState::Comment | LexState::InLex | LexState::InQtParam |
                 LexState::InQtValue | LexState::InQtLex | LexState::InArrayVal => {
                        buffer[buf_fill] = c;
                        buf_fill += 1;
                    },
                    LexState::StartParam | LexState::InParamBlank => {
                        state = LexState::InParam;
                        buffer[buf_fill] = dig;
                        buf_fill += 1;
                    },
                    LexState::StartValue | LexState::InBreak=> {
                        state = LexState::InValue;
                        buffer[buf_fill] = dig;
                        buf_fill += 1;
                    },
                    LexState::Begin | LexState::BlankOrEnd=> {
                        state = LexState::InLex;
                        buffer[buf_fill] = dig;
                        buf_fill += 1;
                    },
                    LexState::EscapeBreakValue => {
                        buffer[buf_fill] = '\\';
                        buf_fill += 1;
                        buffer[buf_fill] = c;
                        buf_fill += 1;
                        state = LexState::InValue;
                    },
                    LexState::EscapeValue => {
                        buffer[buf_fill] = '\\';
                        buf_fill += 1;
                        buffer[buf_fill] = c;
                        buf_fill += 1;
                        state = LexState::InQtValue
                    }
                    LexState::EscapeQtParam => {
                        buffer[buf_fill] = '\\';
                        buf_fill += 1;
                        buffer[buf_fill] = c;
                        buf_fill += 1;
                        state = LexState::InQtParam
                    }
                    LexState::EscapeEndArray => {
                        buffer[buf_fill] = '\\';
                        buf_fill += 1;
                        buffer[buf_fill] = c;
                        buf_fill += 1;
                        state = LexState::InArrayVal
                    },
                    LexState::EscapeParam => {
                        buffer[buf_fill] = '\\';
                        buf_fill += 1;
                        buffer[buf_fill] = c;
                        buf_fill += 1;
                        state = LexState::InParam
                    }
                    _ => todo!("state: {:?} at {}", state, reader.line)
                }
            },
            ',' => {
                match state {
                    LexState::InParam | LexState::InParamBlank => {                    
                        state = LexState::StartParam; 
                        return (Lexem::Parameter(buffer[0..buf_fill].iter().collect()), state, reader.line);
                    },
                    LexState::StartParam => {
                        state = LexState::StartParam; 
                        return (Lexem::Parameter("".to_string() /* EMPTY */), state, reader.line);
                    },
                    LexState::InValue | LexState::InQtParam | LexState::Comment |
                    LexState::InQtValue | LexState::InQtLex | LexState::InArrayVal |
                     LexState::BlankInValue | LexState::InLex => {
                        buffer[buf_fill] = c;
                        buf_fill += 1;
                    },
                    LexState::Begin => {
                        state = LexState::InLex;
                        buffer[buf_fill] = c;
                        buf_fill += 1;
                    },
                    LexState::EscapeBreakValue => {
                        buffer[buf_fill] = '\\';
                        buf_fill += 1;
                        buffer[buf_fill] = c;
                        buf_fill += 1;
                        state = LexState::InValue;
                    },
                    LexState::EscapeValue => {
                        buffer[buf_fill] = '\\';
                        buf_fill += 1;
                        buffer[buf_fill] = c;
                        buf_fill += 1;
                        state = LexState::InQtValue
                    }
                    LexState::InBreak | LexState::StartValue => {
                        buffer[buf_fill] = c;
                        buf_fill += 1; 
                        state = LexState::InValue;
                    },
                    LexState::EscapeEndArray => {
                        buffer[buf_fill] = '\\';
                        buf_fill += 1;
                        buffer[buf_fill] = c;
                        buf_fill += 1;
                        state = LexState::InArrayVal
                    },
                    _ => todo!("state: {:?} at {}", state, reader.line)
                }
            },
            '.' => {
                //println!("{:?}", state);
                match state {
                    LexState::InValue | LexState::InLex | LexState::InParam | LexState::Comment | LexState::InQtParam |
                    LexState::InQtValue | LexState::InQtLex | LexState::InArrayVal  => {
                        buffer[buf_fill] = c;
                        buf_fill += 1;
                    },
                    LexState::BlankOrEnd | LexState::Begin => {
                        buffer[buf_fill] = c;
                        buf_fill += 1;
                        state = LexState::InLex;
                    },
                    LexState::StartValue | LexState::InBreak => {
                        state = LexState::InValue;
                        buffer[buf_fill] = c;
                        buf_fill += 1;
                    },
                    LexState::StartParam | LexState::InParamBlank=> {
                        state = LexState::InParam;
                        buffer[buf_fill] = c;
                        buf_fill += 1;
                    },
                    LexState::EscapeBreakValue => {
                        buffer[buf_fill] = '\\';
                        buf_fill += 1;
                        buffer[buf_fill] = c;
                        buf_fill += 1;
                        state = LexState::InValue;
                    },
                    LexState::EscapeValue => {
                        buffer[buf_fill] = '\\';
                        buf_fill += 1;
                        buffer[buf_fill] = c;
                        buf_fill += 1;
                        state = LexState::InQtValue
                    }
                    LexState::EscapeEndArray => {
                        buffer[buf_fill] = '\\';
                        buf_fill += 1;
                        buffer[buf_fill] = c;
                        buf_fill += 1;
                        state = LexState::InArrayVal
                    },
                    _ => todo!("state: {:?} at {}", state, reader.line)
                }

            },
            _ => {
                match state {
                    LexState::InQtLex | LexState::InQtParam |
                    LexState::InQtValue | LexState::InArrayVal | LexState::InLex |
                    LexState::InValue | LexState::InParam | LexState::InType | LexState::Comment => {
                        buffer[buf_fill] = c;
                        buf_fill += 1;
                    },
                    LexState::Begin | LexState::BlockStart | LexState::BlankOrEnd | LexState::BlockEnd => {
                        state = LexState::InLex;
                        buffer[buf_fill] = c;
                        buf_fill += 1;
                    },
                    LexState::QuotedStart => {
                        state = LexState::InQtLex;
                        buffer[buf_fill] = c;
                        buf_fill += 1;
                    },
                    LexState::StartParam | LexState::InParamBlank => {
                        state = LexState::InParam;
                        buffer[buf_fill] = c;
                        buf_fill += 1;
                    }
                    LexState::BlankInValue |  LexState::InBreak | LexState::StartValue => {
                        buffer[buf_fill] = c;
                        buf_fill += 1;
                        state = LexState::InValue;
                    },
                    LexState::EscapeParam => {
                        buffer[buf_fill] = '\\';
                        buf_fill += 1; 
                        buffer[buf_fill] = c;
                        buf_fill += 1; 
                        state = LexState::InParam;
                    },
                    LexState::EscapeValue  => {
                        buffer[buf_fill] = '\\';
                        buf_fill += 1; 
                        buffer[buf_fill] = c;
                        buf_fill += 1; 
                        state = LexState::InQtValue;
                    },
                    LexState::EscapeQtParam => {
                        buffer[buf_fill] = '\\';
                        buf_fill += 1; 
                        buffer[buf_fill] = c;
                        buf_fill += 1; 
                        state = LexState::InQtParam;
                    },
                    LexState::EscapeBreakValue => {
                        buffer[buf_fill] = '\\';
                        buf_fill += 1;
                        buffer[buf_fill] = c;
                        buf_fill += 1;
                        state = LexState::InValue;
                    },
                    LexState::EscapeEndArray => {
                        buffer[buf_fill] = '\\';
                        buf_fill += 1;
                        buffer[buf_fill] = c;
                        buf_fill += 1;
                        state = LexState::InArrayVal
                    },
                    LexState::EndFunction  => {
                        state = LexState::Begin; 
                        log.error(&format!{"Expected ';' or a new line at {}:{}", reader.line, reader.line_offset});
                        return (Lexem::EOF, state, reader.line)
                    },
                    _ => todo!("state: {:?} at {}", state, reader.line)
                }
            }
        }
        c1 = reader.next()
    }
    match state {
        LexState::InQtLex => {
            log.error(&format!{"Unexpected ending of the script file in quoted token at {}:{}", reader.line, reader.line_offset});
            return (Lexem::EOF, state, reader.line);
        },
        LexState::EndFunction | LexState::InParam => {
            //state = 
            return (Lexem::EOF, state, reader.line);
        },
        LexState::InLex => {
            
        },
        LexState::InValue  => {
            state = LexState::Begin;
            return (Lexem::Value(buffer[0..buf_fill].iter().collect()), state, reader.line); 
        }, 
        LexState::Begin | LexState::End | LexState::BlockEnd => {
            return (Lexem::EOF, state, reader.line);
        },
        LexState::InType => {
            state = LexState::End;
            return (Lexem::Type(buffer[0..buf_fill].iter().collect()), state, reader.line);
        },
        LexState::Comment => {
            state = LexState::End;
            return (Lexem::Comment(buffer[0..buf_fill].iter().collect()), state, reader.line);
        },
        _ => todo!("state: {:?} at {}", state, reader.line)
    }
    (Lexem::Variable(buffer[0..buf_fill].iter().collect()), state, reader.line)
}

fn process_lex_header(_log: &Log, value : &str, _vars: &HashMap<String, VarVal>) -> Box<(String, String, String, String)> {
    let mut buf = Vec::with_capacity(4096);

    let chars = value.chars();
    let mut state = HdrState::InType;
    let mut last_blank = 0;
    let mut name : String = "".to_string();
    let mut lex_type : String = "".to_string();
    let mut work_dir : String = "".to_string();
    let mut path : String = "".to_string();
    for c in chars {
        match c {
            ' ' | '\t' => {
                match state {
                    HdrState::InType => {
                        state = HdrState::NameStart;
                        lex_type = buf.clone().into_iter().collect();
                        buf.clear();
                    },
                    HdrState::PathDiv | HdrState::WorkDiv => {
                    },
                    HdrState::NameStart => (),
                    HdrState::InName => {
                        state = HdrState::InNameBlank;
                        last_blank = buf.len();
                        buf.push(c)
                    },
                    HdrState::InNameBlank | HdrState::InWorkBlank | HdrState::InPathBlank => {
                        buf.push(c)
                    },
                   // HdrState::WorkDiv | HdrState::PathDiv => {},
                    HdrState::InWork => {
                        state = HdrState::InWorkBlank;
                        last_blank = buf.len();
                        buf.push(c)
                    },
                    HdrState::InPath => {
                        state = HdrState::InPathBlank;
                        last_blank = buf.len();
                        buf.push(c)
                    },
                     HdrState::InNameQt 
                    | HdrState::InPathQt | HdrState::InWorkQt => {
                        buf.push(c)
                    },
                   // _ => todo!("state: {:?}", state)
                }

            },
            ':' => {
                match state {
                    HdrState::InType => {
                        state = HdrState::WorkDiv;
                        lex_type = buf.clone().into_iter().collect();
                        buf.clear();
                    },
                    HdrState::WorkDiv => {
                        state = HdrState::PathDiv;
                    },
                    HdrState::NameStart => {
                        state = HdrState::WorkDiv;
                    },
                    HdrState::InName => {
                        state = HdrState::WorkDiv;
                        name = buf.clone().into_iter().collect();
                        buf.clear();
                    },
                    HdrState::InWork => {
                        state = HdrState::PathDiv;
                        work_dir = buf.clone().into_iter().collect();
                        buf.clear();
                    },
                    HdrState::InNameBlank => {
                        name = buf[0..last_blank].into_iter().collect();
                        buf.clear();
                        state = HdrState::WorkDiv;
                    },
                    HdrState::InNameQt | HdrState::InPathQt | HdrState::InWorkQt => {
                        buf.push(c)
                    },
                    _ => todo!("state: {:?}", state)
                }

            },
            '"' => {
                match state {
                    HdrState::NameStart => {
                        state = HdrState::InNameQt;
                    },
                    HdrState::InNameQt => {
                        state = HdrState::InName;
                    },
                    HdrState::InNameBlank => {
                        state = HdrState::InNameQt;
                    },
                    HdrState::WorkDiv => {
                        state = HdrState::InWorkQt;
                    },
                    HdrState::InWorkQt => {
                        state = HdrState::InWork;
                    }
                    HdrState::PathDiv => {
                        state = HdrState::InPathQt;
                    },
                    HdrState::InPathQt => {
                        state = HdrState::InPath;
                    },
                    HdrState::InName => {
                        state = HdrState::InNameQt;
                    }
                    _ => todo!("header state: {:?}", state)
                }
            },
            _ => {
                match state {
                    HdrState::WorkDiv => {
                        state = HdrState::InWork;
                        buf.push(c)
                    },
                    HdrState::PathDiv => {
                        state = HdrState::InPath;
                        buf.push(c)
                    },
                    HdrState::NameStart | HdrState::InName => {
                        state = HdrState::InName;
                        buf.push(c)
                    },
                    
                    HdrState::InNameBlank => {
                        state = HdrState::InName;
                        buf.push(c)
                    },
                    HdrState::InWorkBlank => {
                        state = HdrState::InWork;
                        buf.push(c)
                    },
                    HdrState::InPathBlank => {
                        state = HdrState::InPath;
                        buf.push(c)
                    },
                    HdrState::InWork | HdrState::InPath | HdrState::InNameQt | HdrState::InType 
                    | HdrState::InPathQt | HdrState::InWorkQt => {
                        buf.push(c)
                    },
                    //_ => todo!("state: {:?}", state)
                }
            }
        }
    }
    match state {
        HdrState::InType => {
            lex_type = buf.into_iter().collect();
        },
        HdrState::InName => {
            name = buf.into_iter().collect();
        },
        HdrState::InNameBlank => {
            name = buf[0..last_blank].into_iter().collect();
        },
        HdrState::InWork => {
            work_dir = buf.into_iter().collect();
        },
        HdrState::InWorkBlank => {
            work_dir = buf[0..last_blank].into_iter().collect();
        },
        HdrState::InPath => {
            path = buf.into_iter().collect();
        },
        HdrState::InPathBlank=> {
            path = buf[0..last_blank].into_iter().collect();
        },
        HdrState::NameStart | HdrState::WorkDiv | HdrState::PathDiv=> (),
        _ => todo!("state: {:?}", state)
    }
    //println!{"=>{lex_type} {name} '{work_dir}' '{path}'"};
    Box::new((lex_type.to_string(), name.to_string(), work_dir.to_string(), path.to_string()))
}

pub fn process_template_value(log: &Log, value : &str, vars: &GenBlock, res_prev: &Option<VarVal>) -> Box<String> {
    // String interpolation
    let mut buf = Vec::with_capacity(4096);
    let mut buf_var = Vec::with_capacity(256); // buf for var name
    let chars = value.chars();
    let mut state = TemplateState::InVal;
    let mut was_replacement = false;
    for c in chars {
        match c {
            '$' => {
                match state {
                    TemplateState::InVal  => state = TemplateState::VarStart,
                    TemplateState::VarStart => {
                        buf.push(c);
                    },
                    TemplateState::InVar => {buf_var.push(c)},
                    _ => todo!()
                }
            },
            '{' => {
                match state {
                    TemplateState::VarStart => state = TemplateState::InVar,
                    TemplateState::InVal  => {
                        buf.push(c);
                    },
                    TemplateState::InVar => buf_var.push(c),
                    _ => todo!()
                }
            },
            '}' => {
                match state {
                    TemplateState::VarStart => {
                        state = TemplateState::InVal;
                        buf.push('$');
                        buf.push(c);
                    },
                    TemplateState::InVal  => {
                        buf.push(c);
                    },
                    TemplateState::InVar => {
                        state = TemplateState::InVal;
                        let var : String = buf_var.clone().into_iter().collect();
                       // println!("looking {:?}", buf_var);
                        // check name for ~~ and then use global thread local
                        let res = if var == PREV_VAL {
                            match res_prev {
                                None => None,
                                Some(prev) => Some(prev.clone())
                            }
                        } else {vars.search_up( &var)};
                        match res {
                            Some(var) => {
                               // println!("found {:?}", var);
                               // TODO avoid replacement in an infinitive loop
                               match var.val_type {
                                    VarType::Environment  => {
                                      //  println!("looking for {} in env", var.value);
                                        let _env = match env::var(var.value.to_string()) {
                                            Ok(val) => {
                                                for vc in val.chars() {
                                                    buf.push(vc);
                                                }
                                            },
                                            Err(_e) => {
                                                for vc in var.value.chars() {
                                                    buf.push(vc);
                                                } 
                                            },
                                        };
                                    },
                                    VarType::Array => {
                                        let chars = vec_to_str(&var.values);
                                        for vc in chars.chars() {
                                            buf.push(vc);
                                        }
                                    },
                                    VarType::Property => {
                                        if let Some(val) = get_property(&var.value) {
                                            for vc in val.chars() {
                                                buf.push(vc);
                                            }
                                        } else {
                                            for vc in var.value.chars() {
                                                buf.push(vc);
                                            }
                                        }
                                    },
                                    _ => {
                                        for vc in var.value.chars() {
                                            buf.push(vc);
                                        }
                                    }
                               }
                               was_replacement = true;
                            },
                            None => {
                               // println!("restoring {:?}", buf_var);
                                buf.push('$');
                                buf.push('{');
                                buf.append(&mut buf_var);
                                buf.push('}');
                            }
                        }
                        buf_var.clear();
                    },
                    _ => todo!()
                }
            },
            _ => {
                match state {
                    TemplateState::InVal => {
                        buf.push(c);
                    },
                    TemplateState::InVar => buf_var.push(c),
                    TemplateState::VarStart => {
                        buf.push('$');
                        buf.push(c);
                        state = TemplateState::InVal;
                    },
                    _ => todo!()
                }
            }
        }
    }
    // temporay hack (no loop detection )
    let expanded_val:String = buf.into_iter().collect();
    if was_replacement {
        log.debug(&format!{"expanding {}", &expanded_val});
        return process_template_value(&log, &expanded_val, &vars, &res_prev)
    }
    Box::new(expanded_val)
}

fn process_array_value(_log: &Log, value : &str) -> Result<Vec<String>, String> {
    let mut buf = vec![' ';value.len()];
    let mut state: LexState = LexState::Begin;
    let chars = value.chars();
    let mut res : Vec<_> = Vec::new();
    let mut blank_pos = 0;
    let mut pos = 0;
    let mut array_line = 1;
    let mut array_pos = 0;
    for c in chars {
        array_pos += 1;
        match c {
            '[' => {
                match state {
                    LexState::Begin  => state = LexState::RangeStart,
                    LexState::InParam | LexState::InQtParam | LexState::RangeStart => {
                        buf[pos] = c;
                        pos += 1;
                    },
                    _ => todo!("state: {:?} at {array_line}:{array_pos}", state)
                }
            },
            ']' => {
                match state {
                    LexState::InQtParam  => {
                        buf[pos] = c;
                        pos += 1;
                    },
                    LexState::InParam => {
                        let param = buf[0..pos].iter().collect();
                        res.push(param);
                        return Ok(res)
                    },
                    LexState::BlankOrEnd => {
                        let param = buf[0..blank_pos].iter().collect();
                        res.push(param);
                        return Ok(res)
                    },
                    LexState::EndQtParam => {
                        return Ok(res)
                    } ,
                    LexState::RangeStart => {
                        return Ok(res)
                    },
                    _ => todo!("state: {:?} at {array_line}:{array_pos}", state)
                }
            },
            '"' => {
                match state {
                    LexState::RangeStart => {
                        state = LexState::InQtParam;
                    },
                    LexState::InParam   => {
                        buf[pos] = c;
                        pos += 1;
                    },
                    LexState::StartParam => {  buf[pos] = c;
                        pos += 1; state = LexState::InParam }
                    LexState::InQtParam => {
                        state = LexState::EndQtParam;
                        let param = buf[0..pos].iter().collect();
                        res.push(param);
                        pos = 0;
                    },
                    LexState::EscapeParam => {
                        buf[pos] = c;
                        pos += 1;
                        state = LexState::InQtParam;
                    },
                    _ => todo!("state: {:?} at {array_line}:{array_pos}", state)
                }
            },
            '\\' => {
                match state {
                    LexState::StartParam => { state = LexState::InParam },
                    _ => ()
                }
                match state {
                    LexState::InParam  => {
                        buf[pos] = c;
                        pos += 1;
                    },
                    LexState::InQtParam  => {
                        state = LexState::EscapeParam;
                    },
                    _ => todo!("state: {:?} at {array_line}:{array_pos}", state)
                }
            },
            ',' => {
                match state {
                    LexState::InParam  => {
                        let param = buf[0..pos].iter().collect();
                       // log.log(&format!{"param: {}", &param});
                        res.push(param);
                        pos = 0;
                        state = LexState:: StartParam;
                    },
                    LexState::BlankOrEnd => {
                        let param = buf[0..blank_pos].iter().collect();
                        res.push(param);
                        pos = 0;
                        state = LexState:: StartParam;
                    },
                    LexState::InQtParam  => {
                        buf[pos] = c;
                        pos += 1;
                    },
                    LexState::EndQtParam => {
                        state = LexState:: StartParam;
                    }, 
                    _ => todo!("state: {:?} at {array_line}:{array_pos}", state)
                }
            },
            ' ' | '\t' | '\n' | '\r' => {
                if c == '\n' {
                    array_pos = 0;
                    array_line += 1
                }
                match state {
                    LexState::InQtParam  => {
                        buf[pos] = c;
                        pos += 1;
                    },
                    LexState::InParam  => {
                        blank_pos = pos;
                        buf[pos] = c;
                        pos += 1;
                        state = LexState::BlankOrEnd;
                    },
                    LexState::BlankOrEnd => { 
                        buf[pos] = c;
                        pos += 1;
                    },
                    LexState::EndQtParam => { },
                    LexState::RangeStart | LexState:: StartParam => {

                    },
                    _ => todo!("state: {:?} at {array_line}:{array_pos}", state)
                }
            },
            //':' =>
            _ => {
                match state {
                    LexState::InParam | LexState::InQtParam  => {
                        buf[pos] = c;
                        pos += 1;
                    },
                    LexState::EscapeParam => {
                        buf[pos] = '\\';
                        pos += 1;
                        buf[pos] = c;
                        pos += 1;
                    },
                    LexState::BlankOrEnd => {
                        buf[pos] = c;
                        pos += 1;
                        state = LexState::InParam;
                    },
                    LexState::RangeStart | LexState::StartParam => {
                        state = LexState::InParam;
                        buf[pos] = c;
                        pos += 1;
                    },
                    _ => todo!("state: {:?} at {array_line}:{array_pos}", state)
                }
            }
        }
    }
    Err(value.to_string())
}

pub fn process(log: &Log, file: & str, block: GenBlockTup) -> io::Result<()> {
    let mut all_chars =  match  open(file) {
        Err(e) => return Err(e),
        Ok(r) => r,
    };
    
    //let mut func_stack = Vec::new();
    //let mut block_stack : Vec<&mut GenBlock> = Vec::new();
    let mut state = LexState::Begin;
    // current block
    let mut scoped_block = block; 
    let mut current_name = "".to_string();
    while state != LexState::End {
        // consider returning a partial lexem for example, interrupted by a comment
        let ( lex, mut state2, line) = read_lex(log, &mut all_chars, state);
        log.debug(&format!("Lex: {:?}, line: {}/{}, state: {:?}", lex, all_chars.line, all_chars.line_offset, state2));
        match lex {
            Lexem::EOF => {
                state2 = LexState::End;
            },
            Lexem::Variable(name) => {
                current_name = name.to_string();
            },
            Lexem::Value(value) => {
               // consider it can be an array in form [v1,v2,...vn]
               let c_b = 
                if value.starts_with("[") && value.ends_with("]") {
                    let res = process_array_value(&log, &value);
                    if res.is_ok() {
                        VarVal::from_vec(&res.unwrap())
                    } else {
                        log.error(&format!{"The array isn't well defined: {} at {}:{}", &value, all_chars.line, all_chars.line_offset});
                        VarVal::from_string(&value)
                    }
                } else {VarVal::from_string(&value)}
                ;
                if current_name.is_empty() {
                    let mut scoped_block = scoped_block.borrow_mut();
                    scoped_block.out = Some(value);
                    log.warning(&format!{"The value {c_b:?} can't be set to no name at {}:{}", all_chars.line, all_chars.line_offset} )
                } else {
                    scoped_block.borrow_mut().vars.insert(current_name.to_owned(), c_b);
                }
            },
            Lexem::Function(name) => {
                // name can be function + main argument
                let (type_hdr,name,work,path) = *process_lex_header(&log, &name, &scoped_block.0.as_ref().borrow_mut().vars) ;
                let mut func = GenBlock::new(BlockType::Function);
                //fun::GenBlockTup(Rc::new(RefCell::new(GenBlock::new(BlockType::Function))));
                func.name = Some(type_hdr);
                func.flex = if name.is_empty() {None} else { Some(name)};
                func.dir = if work.is_empty() {None} else { Some(work)};
                func.out = if path.is_empty() {None} else { Some(path)};

                func.script_line = line;
                scoped_block = scoped_block.add(GenBlockTup(Rc::new(RefCell::new(func))));
            },
            Lexem::Type(var_type) => {
                let mut bl = scoped_block.borrow_mut();
                //log.debug(&format!("type {} in block {:?}", &current_name, bl.block_type));
                match bl.vars.get(&current_name.to_string()) {
                    Some(var) => { 
                        match var_type.as_str() {
                            "file" => {
                                let c_b = VarVal{val_type:VarType::File, value:var.value.clone(), values: Vec::new()};
                                bl.vars.insert(current_name.to_string(), c_b);
                            },
                            "prop" => {
                                //  println!("prop {} in {:?}", var.value, bl.block_type);
                                  let c_b = VarVal{val_type:VarType::Property, value:var.value.clone(), values: Vec::new()};
                                  bl.vars.insert(current_name.to_string(), c_b);
                              },
                            "env" => {
                              //  println!("env {} in {:?}", var.value, bl.block_type);
                                let c_b = VarVal{val_type:VarType::Environment, value:var.value.clone(), values: Vec::new()};
                                bl.vars.insert(current_name.to_string(), c_b);
                            },
                            "rep-rust" | "rep-crate"=> {
                                //let at_pos = 
                                //  println!("env {} in {:?}", var.value, bl.block_type);
                                  let c_b = VarVal{val_type:VarType::RepositoryRust, value:var.value.clone(), values: Vec::new()};
                                  bl.vars.insert(current_name.to_string(), c_b);
                              },
                            "rep-maven" => {
                                //let at_pos = 
                                //  println!("env {} in {:?}", var.value, bl.block_type);
                                  let c_b = VarVal{val_type:VarType::RepositoryMaven, value:var.value.clone(), values: Vec::new()};
                                  bl.vars.insert(current_name.to_string(), c_b);
                              },
                            _ => log.error(&format!("Unknown type '{}' ignored at {}:{}", &var_type, all_chars.line, all_chars.line_offset))
                        }
                        
                    },
                    _ => ()
                }
            },
            Lexem::Parameter(value) => { // collect all parameters and then process function call
                let name = {
                   let mut rl_block = scoped_block.borrow_mut();
                   rl_block.params.push(value.to_owned());
                   rl_block.name.to_owned()
                };
               
                if state2 == LexState::EndFunction {
                    log.debug(&format!("end func for {:?}", name));
                    if let Some(name) = name {
                        match name.as_str() {
                            "include" => {
                                //println!{"search {:?}", &value};
                                match scoped_block.search_up(&value) {
                                    Some(var) => {
                                      // println!("found {:?}", var);
                                        match var.val_type {
                                            VarType::File => {
                                                let mut clone_var = *process_template_value(&log, &var.value, &scoped_block.0.as_ref().borrow_mut(), &None);
                                                let parent_scoped_block = scoped_block.parent();
                                                if let Some(block) = parent_scoped_block {
                                                    if !has_root(&clone_var) {
                                                    // TODO consider not CWD but the current script directory
                                                        let cwd = scoped_block.search_up(&::CWD.to_string());
                                                        if let Some(cwd) = cwd {
                                                            clone_var = cwd.value + std::path::MAIN_SEPARATOR_STR + &clone_var
                                                        }
                                                    }
                                                    match process(&log, clone_var.as_str(), block.clone()) {
                                                        Err(e) => {
                                                            log.error(&format!("Can't process an include script {clone_var} at {0}, problem: {e}", all_chars.line));
                                                            return Err(e)
                                                        },
                                                        _ => ()
                                                    }
                                                }
                                            },
                                            _ => log.error(&format!("The include location variable {} isn't type file , the include is ignored at {}", &value, all_chars.line)),
                                        }
                                    },
                                    None => {
                                        let mut temp_expand = *process_template_value(&log, &value, &scoped_block.0.as_ref().borrow_mut(), &None);
                                        log.debug(&format!{"Expand an include template {}", temp_expand});
                                        let parent_scoped_block = scoped_block.parent();
                                        if let Some(block) = parent_scoped_block {
                                            // TODO fn expand_path(path: impl AsRef<str>, block: GenBlockTup) -> String 
                                            if !has_root(&temp_expand) {
                                                let cwd = scoped_block.search_up(&::CWD.to_string());
                                                if let Some(cwd) = cwd {
                                                    temp_expand = cwd.value + std::path::MAIN_SEPARATOR_STR + &temp_expand
                                                }
                                            }
                                            if let Err(e) = 
                                                process(&log, &temp_expand, block.clone()) {
                                                    log.error(&format!("Can't process an include script {} at {}, problem: {}", temp_expand, all_chars.line, e));
                                                    return Err(e)
                                                }
                                        }
                                    }
                                }
                            },
                            _ => ()
                        }
                    } 
                    scoped_block = scoped_block.parent().clone().unwrap();
                }
 
            },
            Lexem::BlockHdr(value) => { 
                // parse header and push in block stack
               // let mut test_block = GenBlock::new(BlockType::Target);
                let parent_type = scoped_block.borrow().block_type.clone();
                 /*  if let Some(parent) = &scoped_block.borrow().parent {
                       let parent_type = Rc::clone(&parent).borrow().block_type.clone();
                       parent_type
                   } else {
                       BlockType::Main
                   };*/
                current_name.clear();
                let (type_hdr,name,work,path) = *process_lex_header(&log, &value, &scoped_block.0.as_ref().borrow_mut().vars) ;
                log.debug(&format!("Type: {}, name: {}, work dir: '{}', path; '{}'", type_hdr,name,work,path));
                match type_hdr.as_str() {
                    "target" => {
                        // check if a target with the name exists
                        let target = scoped_block.get_target(&name);
                        if target.is_none() {
                            let mut inner_block = GenBlock::new(BlockType::Target);
                            inner_block.name = Some(name);
                            inner_block.dir = Some(work);
                            inner_block.flex = Some(path);
                            //println!{"name {:?} dir {:?} flex {:?}", inner_block.name, inner_block.dir, inner_block.flex}
                            scoped_block =  scoped_block.add(GenBlockTup(Rc::new(RefCell::new(inner_block))));
                        } else {
                            log.error(&format!("Target {} is already exists", &name));
                        }
                    },
                    "eq" => {
                        let  inner_block = GenBlock::new(BlockType::Eq);
        
                        scoped_block =  scoped_block.add(GenBlockTup(Rc::new(RefCell::new(inner_block))));
                    },
                    "if" => {
                        let inner_block = GenBlock::new(BlockType::If);
        
                        scoped_block =  scoped_block.add(GenBlockTup(Rc::new(RefCell::new(inner_block))));
                    },
                    "then" => {
                        let inner_block = GenBlock::new(BlockType::Then);
        
                        scoped_block =  scoped_block.add(GenBlockTup(Rc::new(RefCell::new(inner_block))));
                    },
                    "neq" => {
                        let inner_block = GenBlock::new(BlockType::Neq);
        
                        scoped_block =  scoped_block.add(GenBlockTup(Rc::new(RefCell::new(inner_block))));
                    },
                    "else" => {
                        let inner_block = GenBlock::new(BlockType::Else);
        
                        scoped_block =  scoped_block.add(GenBlockTup(Rc::new(RefCell::new(inner_block))));
                    },
                    "or" => {
                        let inner_block = GenBlock::new(BlockType::Or);
        
                        scoped_block =  scoped_block.add(GenBlockTup(Rc::new(RefCell::new(inner_block))));
                    },
                    "and" => {
                        let inner_block = GenBlock::new(BlockType::And);
        
                        scoped_block =  scoped_block.add(GenBlockTup(Rc::new(RefCell::new(inner_block))));
                    },
                    "not" => {
                        let inner_block = GenBlock::new(BlockType::Not);
        
                        scoped_block =  scoped_block.add(GenBlockTup(Rc::new(RefCell::new(inner_block))));
                    },
                    "for" => {
                        let mut inner_block = GenBlock::new(BlockType::For);
                        inner_block.name = Some(name);
                        inner_block.dir = Some(work);
                        inner_block.flex = Some(path);
                        scoped_block =  scoped_block.add(GenBlockTup(Rc::new(RefCell::new(inner_block))));
                    },
                    "" => {
                        let inner_block = GenBlock::new(BlockType::Scope);
        
                        scoped_block =  scoped_block.add(GenBlockTup(Rc::new(RefCell::new(inner_block))));// *scoped_block = GenBlock::new(BlockType::Scope);
                    },
                    "dependency" => {
                        let inner_block = GenBlock::new(BlockType::Dependency);
        
                        scoped_block =  scoped_block.add_dep(GenBlockTup(Rc::new(RefCell::new(inner_block))));
                    },
                    "while" => {
                        let mut inner_block = GenBlock::new(BlockType::While);
                        inner_block.name = Some(name);
                        scoped_block =  scoped_block.add(GenBlockTup(Rc::new(RefCell::new(inner_block))));
                    },
                    "case" => {
                        let mut inner_block = GenBlock::new(BlockType::Case);
                        inner_block.name = Some(name); // var holding analyzed pattern
                        scoped_block =  scoped_block.add(GenBlockTup(Rc::new(RefCell::new(inner_block))));
                    },
                    "choice" if parent_type == BlockType::Case => {
                        let mut inner_block = GenBlock::new(BlockType::Choice);
                       // println!{"added choice {type_hdr} -> {name}"};
                        inner_block.name = Some(name); 
                        scoped_block =  scoped_block.add(GenBlockTup(Rc::new(RefCell::new(inner_block))))
                    },
                    _ => log.error(&format!("unknown block {} of {:?} at {}:{}", type_hdr, &parent_type, all_chars.line, all_chars.line_offset))
                }
                
            },
            Lexem::BlockEnd(value) => {
                //println!(" current {:?}", scoped_block.0.borrow_mut().block_type);
                let mut rl_block = scoped_block.borrow_mut();
                if rl_block.out == None {
                    rl_block.out = value
                }
                drop(rl_block);
                let parent = scoped_block.parent();
                match parent {
                    None => log.error(&format!("Unmatched block {:?} closing found at {}:{}", scoped_block.borrow().block_type, all_chars.line, all_chars.line_offset)),
                    Some(parent) => scoped_block = parent.clone()
                }
            },
            Lexem::Comment(value) => {
                log.debug(&format!("Commentary: {}, line: {}/{}", value, all_chars.line, all_chars.line_offset));
            },
            _ => todo!("unprocessed lexem {:?}", lex)
        }
        state = state2;
    }
    
    Ok(())
}