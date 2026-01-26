use crate::SystemTime;
#[cfg(any(unix, target_os = "redox"))]
use std::path::{MAIN_SEPARATOR_STR, Path, PathBuf};
#[cfg(target_os = "windows")]
use std::path::{MAIN_SEPARATOR, Path, PathBuf};

#[derive(Default, PartialEq)]
enum EscState {
#[default]
    No,
    Suspect,
    Esc,
    Oct2
}

pub fn insert_ctrl_char(in_str:&String) -> String {
    let esc = in_str.find('\\');
    if esc.is_none() {
        return in_str.to_string() // maybe return &String to avoid any data manipulation
    }
    let mut chars = vec![' '; in_str.chars().count()];
    let mut next = 0usize;
    let mut state = EscState::No;
    let mut esc = 0u8;
    for c in in_str.chars() {
        match c {
            '\\' => {
                match state {
                    EscState::No => state = EscState::Suspect,
                    EscState::Suspect => {
                        state = EscState::No;
                        chars[next] = '\\';
                        next += 1;
                        chars[next] = c;
                        next += 1
                    },
                    _ => {
                        state = EscState::No;
                        chars[next] = c;
                        next += 1
                    }
                }
            },
            '0'..='7' => {
                match state {
                    EscState::No => {
                        chars[next] = c;
                        next += 1
                    },
                    EscState::Suspect => {
                        match c {
                            '0'..'4' => {
                                state = EscState::Esc;
                                esc = 64 * (c.to_digit(10).unwrap() as u8 - '0'.to_digit(10).unwrap() as u8)
                            },
                            _ => {
                                state = EscState::No;
                                chars[next] = '\\';
                                next += 1;
                                chars[next] = c;
                                next += 1
                            },
                        }
                    }    
                    EscState::Esc => {
                        state = EscState::Oct2;
                        esc += 8 * (c.to_digit(10).unwrap() as u8 - '0'.to_digit(10).unwrap() as u8)
                    },
                    EscState::Oct2 => {
                        state = EscState::No;
                        esc += c.to_digit(10).unwrap() as u8 - '0'.to_digit(10).unwrap() as u8;
                        chars[next] = char::from_u32(esc as u32).unwrap();
                        next += 1
                    }
                }
            },
            _ => {
                match state {
                    EscState::Suspect => {
                        chars[next] = '\\';
                        next += 1
                    }
                    EscState::Esc => {
                        chars[next] = '\\';
                        next += 1;
                        chars[next] = (esc / 64 + b'0') as _;
                        next += 1
                    }
                    EscState::No | EscState::Oct2 => ()
                }
                
                state = EscState::No;
                chars[next] = c;
                next += 1
            }
        }
        
    }
    match state {
        EscState::Suspect => {
            chars[next] = '\\';
        }
        EscState::No | EscState::Oct2 | EscState::Esc => ()
    }
                
    chars[0..next].iter().collect()
}

#[allow(non_camel_case_types)]
#[allow(dead_code)]
#[derive(Default, PartialEq)]
enum DateFmtState {
    #[default]
        Relax,
        M, MM, MMM, 
        W,
        Y, YY,
        D, DD,
        h, hh,
        m, mm,
        s, ss,
        Z, z,
        Esc,

}

pub const SHORT_MONTH: &[&str] = &[
"Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec", 
];

pub fn format_time(fmt_str: impl AsRef<str>,  time: SystemTime) -> String {
    let dur = time.duration_since(SystemTime::UNIX_EPOCH).unwrap();
    let tz = time::get_local_timezone_offset();
    let (y,m,d,h,min,s,w) = time:: get_datetime(1970, ((dur.as_secs() as i64) + (tz as i64)*60) as u64);
    let mut res = String::from("");
    let mut state = DateFmtState::default();
    let format_upon_mask = |state| {
        match state {
        DateFmtState::M => format!{"{m}"},
        DateFmtState::MM => format!{"{m:0>2}"},
        DateFmtState::MMM => SHORT_MONTH[(m-1) as usize].to_string(),
        DateFmtState::D => format!{"{d}"},
        DateFmtState::DD => format!{"{d:0>2}"},
        DateFmtState::Y => format!{"{y:0>2}"},
        DateFmtState::YY => format!{"{y:0>4}"},
        DateFmtState::h |  DateFmtState::hh => format!{"{h:0>2}"},
        DateFmtState::m | DateFmtState::mm => format!{"{min:0>2}"},
        DateFmtState::s | DateFmtState::ss => format!{"{s:0>2}"},
        DateFmtState::W => time::DAYS_OF_WEEK[w as usize].to_string(),
        DateFmtState::z | DateFmtState::Z => format!{"{:0>2}{:0>2}", tz/60, tz%60},
        _ => "".to_string()
       }
    };
    for c in fmt_str.as_ref().chars() {
        match c {
            '\\' => {
                match state {
                    DateFmtState::Esc => {
                        res.push('\\');
                        state = DateFmtState::Relax
                    }
                    _ => {
                        res.push_str(&format_upon_mask(state));
                        state = DateFmtState::Esc
                    }
                }
            }
            'M' => {
                match state {
                    DateFmtState::Esc => {
                        res.push('M');
                        state = DateFmtState::Relax
                    }
                    DateFmtState::Relax => state = DateFmtState::M,
                    DateFmtState::M => state = DateFmtState::MM,
                    DateFmtState::MM => state = DateFmtState::MMM,
                    _ => {
                        res.push_str(&format_upon_mask(state));
                        state = DateFmtState::M
                    }
                }
            }
            'D' => {
                match state {
                    DateFmtState::Esc => {
                        res.push('D');
                        state = DateFmtState::Relax
                    }
                    DateFmtState::Relax => state = DateFmtState::D,
                    DateFmtState::D => state = DateFmtState::DD,
                    _ => {
                        res.push_str(&format_upon_mask(state));
                        state = DateFmtState::D
                    }
                }
            }
            'Y' => {
                match state {
                    DateFmtState::Esc => {
                        res.push('Y');
                        state = DateFmtState::Relax
                    }
                    DateFmtState::Relax => state = DateFmtState::Y,
                    DateFmtState::Y => state = DateFmtState::YY,
                    _ => {
                        res.push_str(&format_upon_mask(state));
                        state = DateFmtState::Y
                    }
                }
            }
            'h' => {
                match state {
                    DateFmtState::Esc => {
                        res.push('h');
                        state = DateFmtState::Relax
                    }
                    DateFmtState::Relax => state = DateFmtState::h,
                    DateFmtState::h => state = DateFmtState::hh,
                    _ => {
                        res.push_str(&format_upon_mask(state));
                        state = DateFmtState::h
                    }
                }
            }
            'm' => {
                match state {
                    DateFmtState::Esc => {
                        res.push('m');
                        state = DateFmtState::Relax
                    }
                    DateFmtState::Relax => state = DateFmtState::m,
                    DateFmtState::m => state = DateFmtState::mm,
                    _ => {
                        res.push_str(&format_upon_mask(state));
                        state = DateFmtState::m
                    }
                }
            }
            's' => {
                match state {
                    DateFmtState::Esc => {
                        res.push('s');
                        state = DateFmtState::Relax
                    }
                    DateFmtState::Relax => state = DateFmtState::s,
                    DateFmtState::s => state = DateFmtState::ss,
                    _ => {
                        res.push_str(&format_upon_mask(state));
                        state = DateFmtState::s
                    }
                }
            }
            'W' => {
                match state {
                    DateFmtState::Esc => {
                        res.push('W');
                        state = DateFmtState::Relax
                    }
                    DateFmtState::Relax => state = DateFmtState::W,
                    _ => {
                        res.push_str(&format_upon_mask(state));
                        state = DateFmtState::W
                    }
                }
            }
            'Z' => {
                match state {
                    DateFmtState::Esc => {
                        res.push('Z');
                        state = DateFmtState::Relax
                    }
                    DateFmtState::Relax => state = DateFmtState::Z,
                    _ => {
                        res.push_str(&format_upon_mask(state));
                        state = DateFmtState::Z
                    }
                }
            }
             _ => {
                res.push_str(&format_upon_mask(state));
                state = DateFmtState::Relax;
                res.push(c)
            }
        }
    }
    res.push_str(&format_upon_mask(state));
    res
}

pub fn vec_to_str(arr: &[String]) -> String {
    match arr.iter().map(|current| current.to_string()).reduce(|first, current| first + "\t" + &current) {
        Some(val) => val.to_owned(),
        None =>  String::new()
    }
}

use std::time::UNIX_EPOCH;
#[inline]
pub fn year_now() -> u64 {
    SystemTime::now()
    .duration_since(UNIX_EPOCH).expect("Time went backwards")
    .as_secs() / 31556952 + 1970
}

#[cfg(target_os = "windows")]
pub fn has_root(path:  impl AsRef<str>) -> bool {
    let path = path.as_ref().as_bytes();
    !path.is_empty() && (path.len() > 3 && path[1] == b':' && path[2] == b'\\' || path[0] == MAIN_SEPARATOR as _)
}

#[cfg(any(unix, target_os = "redox"))]
#[inline]
pub fn has_root(path:  impl AsRef<str>) -> bool {
    path.as_ref().starts_with(MAIN_SEPARATOR_STR)
}

pub fn normalize_path(path: &Path) -> PathBuf {
    let mut normalized = PathBuf::new();
    
    for component in path.components() {
        match component {
            std::path::Component::ParentDir => {
                normalized.pop(); // Remove the last component
            }
            std::path::Component::CurDir => {
                continue; // Skip current directory
            }
            _ => {
                normalized.push(component); // Add other components
            }
        }
    }
    
    normalized
}