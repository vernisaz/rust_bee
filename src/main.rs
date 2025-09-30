extern crate simtime as time;
use std::{fs::{self,File},env,
    path::{Path,PathBuf},
    io::{self, Write, BufRead, Error, ErrorKind},
    cell::RefCell,
    rc::{Rc},
    time::{SystemTime},
    sync::RwLock,
    collections::HashMap, ops::ControlFlow};
#[cfg(feature = "release")]
use std::panic;

mod help;
mod ver;
mod log;
mod lex;
mod fun;
mod util;

use log::Log;

#[derive(Debug, PartialEq)]
enum CmdOption {
     Help,
     ScriptFile(String),
     Version,
     Verbose,
     SearchUp(Option<String>),
     PropertyFile(String),
     Diagnostics,
     ForceRebuild,
     DryRun,
     Quiet,
     TargetHelp
}


static SYSTEM_PROPERTIES: RwLock<Option<HashMap<String, String>>> = RwLock::new(None);

const SCRIPT_EXT: &str = ".7b";
pub const CWD : &str = "~cwd~";

pub fn set_property(name: &String, value: &String) {
     if SYSTEM_PROPERTIES.read().unwrap().is_none() {
          *SYSTEM_PROPERTIES.write().unwrap() = Some(HashMap::new());
     }
     let mut props = SYSTEM_PROPERTIES.write().unwrap();
     let map = props.as_mut().unwrap();
     map.insert(name.to_string(), value.to_string());
}

pub fn get_property(name: &String) -> Option<String> {
     if SYSTEM_PROPERTIES.read().unwrap().is_none() {
          return None
     }
     let props = SYSTEM_PROPERTIES.read().unwrap();
     let map = props.as_ref().unwrap();
     let ret = map.get(name);
     if let Some(val) = ret {
          Some(val.to_string())
     } else {
          None
     }
}

pub fn get_properties() -> impl IntoIterator <Item = (String, String)> {
    match SYSTEM_PROPERTIES.read() {
        Ok(props) if props.is_some() => {
            let ret = props.clone().unwrap();
            ret
        }
        _ => {let ret: HashMap<String, String> = HashMap::new(); ret}
    }
}

fn parse_command<'a>(log: &'a Log, args: &'a Vec<String>) -> (Vec<CmdOption>, Vec<&'a String>, Vec<String>) {
     let (mut options, mut targets, mut run_args) = (Vec::new(), Vec::new(), Vec::new());
     let mut arg_n = 0;
     while arg_n < args.len() {
         let arg = &args[arg_n] ;
         let len = args.len();
         //println!("analizing {}", arg);
          if arg.starts_with("-h") {
              options.push(CmdOption::Help)
          } else if arg == &"-f" || arg.starts_with("-file") || arg.starts_with("-build") {
               arg_n += 1;
               if arg_n < len {
                    options.push(CmdOption::ScriptFile(args[arg_n].to_string()))
               } else {
                    log.error(&format!("No file path specified in -file option"))
               }
          } else if arg.starts_with("-s") || arg.starts_with("-find") {
               arg_n += 1;
               if arg_n < len {
                    if args[arg_n].starts_with("-") {
                         options.push(CmdOption::SearchUp(None));
                         arg_n -= 1
                    } else {
                         options.push(CmdOption::SearchUp(Some(args[arg_n].to_string())))
                    }
               } else {
                    options.push(CmdOption::SearchUp(None));
                    break
               }
          } else if arg.starts_with("-version") || arg == &"-V" {
               options.push(CmdOption::Version)
          } else if arg.starts_with("-v") || arg.starts_with("-verbose") {
               options.push(CmdOption::Verbose)
          } else if arg.starts_with("-dry")  {
               options.push(CmdOption::DryRun)
          } else if arg.starts_with("-d") || arg.starts_with("-diagnostic") {
               options.push(CmdOption::Diagnostics);
               //unsafe {env::set_var("RUST_BACKTRACE", "1") }
               set_property(&"RUST_BACKTRACE".to_string(), &"1".to_string())
          } else if arg.starts_with("-r")  {
               options.push(CmdOption::ForceRebuild);
          } else if arg.starts_with("-D")  {
               if let Some((name,val)) = &arg[2..].split_once('=') {
                    set_property(&name.to_string(), &val.to_string());
                    //unsafe { env::set_var(name, val) }
               } else {
                    log.error(&format!("Invalid property definition: {}", &arg))
               }
          } else if arg.starts_with("-xprop") || arg.starts_with("-prop") {
               arg_n += 1;
               if arg_n < len {
                    if args[arg_n].starts_with("-") {
                         log.error(&"No property file specified");
                         arg_n -= 1;
                         continue
                    }
                    options.push(CmdOption::PropertyFile(args[arg_n].to_string()))
               } else {
                    log.error(&"Property file isn't specified".to_string());
                    break
               }
          } else if arg.starts_with("-q") {
               options.push(CmdOption::Quiet)
          } else if arg.starts_with("-th") || arg.starts_with("-targethelp") {
               options.push(CmdOption::TargetHelp)
          } else if arg == "--" { 
               arg_n += 1;
               if arg_n < len {
                    run_args.extend_from_slice( &args[arg_n..]);
                    break
               }
          } else if arg.starts_with("-")  {
               log.error(&format!("Not supported option: {}", &arg))
          } else if arg_n > 0 {
               targets.push(arg)
          }
         
         arg_n += 1
     }
     if options.contains(&CmdOption::DryRun) && (!options.contains(&CmdOption::Diagnostics) &&
           !options.contains(&CmdOption::Verbose) ) {
               options.push(CmdOption::Verbose)
     }
     (options, targets, run_args)
}

fn is_bee_scrpt(file_path: &str) -> bool {
     file_path.starts_with("bee") && (file_path.ends_with(".rb") || file_path.ends_with(SCRIPT_EXT))
}

fn find_script(dir: &Path, name: &Option<String>) -> Option<String> {
     let binding = fs::canonicalize(&dir.to_path_buf()).ok()?; 
     let mut curr_dir = binding.as_path();
     while curr_dir.is_dir() {
          match  name  {
               None =>  for entry in fs::read_dir(curr_dir).unwrap() {
                         let path = entry.ok()?.path();
                         if path.is_file() {
                              if is_bee_scrpt(path.file_name()?.to_str()?) {
                                   return Some(path.to_str().unwrap().to_string())
                              }
                         }
                    }
               Some(name) => {
                    let mut path_buf = curr_dir.to_path_buf();
                    path_buf.push(name);
                    let script_path = path_buf.as_path();
                    //println!{"-> {:?}", script_path};
                    if script_path.exists() {
                         return Some(script_path.to_str().unwrap().to_string())
                    }
               }
          }
          curr_dir = curr_dir.parent()?
     }
     None
}

fn main() -> io::Result<()> {
    #[cfg(feature = "release")]
    panic::set_hook(Box::new(|panic_info| {
          if let Some(s) = panic_info.payload().downcast_ref::<&str>() {
               eprintln!("Abnormal RustBee termination: {s:?}")
          } else if let Some(s) = panic_info.payload().downcast_ref::<String>() {
               eprintln!("Abnormal RustBee termination: {s:?}")
          } else {
               eprintln!("Abnormal RustBee termination")
          }
     }));
     let mut log = Log {debug : false, verbose : false, quiet : false};
     *SYSTEM_PROPERTIES.write().unwrap() = Some(HashMap::new());
     let mut path: Option<String> = None;
     let args: Vec<String> = env::args().collect();
     let (options, targets, run_args) = parse_command( &log, &args);

     let lex_tree = fun::GenBlockTup(Rc::new(RefCell::new(fun::GenBlock::new(fun::BlockType::Main))));
     let mut real_targets: Vec<String> = Vec::new();
     for target in targets {
          real_targets.push(target.to_string())
     }
     let _ = &lex_tree.add_var(String::from("~args~"), lex::VarVal::from_vec(&run_args));
     let _ = &lex_tree.add_var(String::from("~os~"),  lex::VarVal::from_string(std::env::consts::OS));
     let _ = &lex_tree.add_var(String::from("~separator~"),  lex::VarVal::from_string(std::path::MAIN_SEPARATOR_STR));
     let _ = &lex_tree.add_var(String::from("~/~"), lex::VarVal::from_string(std::path::MAIN_SEPARATOR_STR));
     let _ = &lex_tree.add_var(String::from("~path_separator~"), if std::env::consts::OS == "windows" {
          lex::VarVal::from_string(";") } else {lex::VarVal::from_string(":")});
     
     let cwd = env::current_dir()?.into_os_string().into_string().unwrap();
     lex_tree.add_var(String::from(CWD),  lex::VarVal::from_string(&cwd));
     //println!("additional ars {:?}", lex_tree.search_up(&String::from("~args~")));
     let mut target_help = false;
     if options.iter().position(|x| *x == CmdOption::Quiet).is_some() {
          log.quiet = true
     }
     if !log.quiet {
        // TODO get year from time::
          log.message(&format!("RustBee (\x1b[0;36mrb\x1b[0m) v {} © {} D. Rogatkin", ver::version().0, util::year_now()));
          if options.contains(&CmdOption::Version) {
               let (ver, build, date) = ver::version();
               log.message(&format!("RB Version: {}, build: {} on {}", ver, build, date))
          }
     }
     for opt in &options {
          //println!("{:?}", opt);
          match opt {
               CmdOption::Version => (),
               CmdOption::Help => { log.message(&format!("{}", help::get_help())); return Ok(())},
               CmdOption::Verbose => log.verbose = true,
               CmdOption::Diagnostics => log.debug = true,
               CmdOption::Quiet => log.quiet = true,
               CmdOption::ScriptFile(file) => {
                    log.log(&format!("Script: {}", file));
                    
                    path = Some(file.to_string())
                    // TODO decide if cwd has to be set in the file.parent()
                    // probably not, because it can be some common place for a global script
                    // to do a build in different directories
               },
               CmdOption::SearchUp(file) => {
                    log.log(&format!("Search: {:?}", file));
                    path = find_script(&Path::new("."), &file);
                    if path.is_some() {
                         let path1 = &path.clone().unwrap();
                         let path1 = Path::new(path1);
                         let cwd = path1.parent().unwrap().to_str().unwrap();
                         unsafe { env::set_var("PWD", &cwd) }
                         lex_tree.add_var(String::from(CWD), lex::VarVal::from_string(cwd));
                    } else {
                         let err = format!("Script {} not found", file.clone().unwrap_or("*".to_string()));
                         log.error(&err);
                         return Err(Error::new(ErrorKind::Other, err))
                    }
               },
               CmdOption::ForceRebuild => {
                    let fb = lex::VarVal{val_type:lex::VarType::Bool, value: String::from("true"), values: Vec::new()};
                    let _ = &lex_tree.add_var(String::from("~force-build-target~"), fb);
               },
               CmdOption::DryRun => {
                    let dr = lex::VarVal{val_type:lex::VarType::Bool, value: String::from("true"), values: Vec::new()};
                    let _ = &lex_tree.add_var(String::from("~dry-run~"), dr);
               }
               CmdOption::PropertyFile(filename) => {
                    let file = File::open(filename)?;
                    let lines = io::BufReader::new(file).lines();
                    for line in lines {
                        if let Ok(prop_def) = line {
                            if let Some((name,val)) = prop_def.split_once('=') {
                                set_property(&name.to_string(), &val.to_string());
                                   //unsafe { env::set_var(name, val) }
                            } else {
                                log.error(&format!("Invalid property definition: {}", &prop_def))
                            }    
                        }
                    }
               }
               CmdOption::TargetHelp => target_help = true
          }
     }
     
     if path.is_none() {
          let mut paths = fs::read_dir(&"./").unwrap();
          //let re = Regex::new(r"bee.*\.rb|.7b").unwrap(); if re.is_match(file_path)
          let _ = paths.try_for_each(|each| {
               if let Ok(p) = each {
                    let p = p.path();
                    if p.is_file() {
                         if let Some(file_name) = p.file_name() {
                              if let Some(file_name) = file_name.to_str() {
                                   if is_bee_scrpt(&file_name) {
                                        path = Some(file_name.to_string());
                                        return ControlFlow::Break(())
                                   }
                              }
                         }
                    }
               }
               ControlFlow::Continue(())
          });
     }
     let Some(mut path) = path else {
          log.error(&format!{"No script file found in {:?}", env::current_dir()});
          return Ok(())
     };
     if !Path::new(&path).exists() {
          path += SCRIPT_EXT;
          if !Path::new(&path).is_file() {
              log.error(&format!{"Script file {path:#} not found"});
              return Ok(())
          }
          
     }
     let _ = &lex_tree.add_var(String::from("~script~"), lex::VarVal::from_string(&path));
     
     let sys_time = SystemTime::now();
     
     let lex_res = lex::process(&log, &PathBuf::from(path), lex_tree.clone());
      if target_help {
          let tree = lex_tree.0.borrow();
          log.message("Targets");
         for child_tree in &tree.children {
                let child = child_tree.0.borrow();
               if child .block_type == fun::BlockType::Target {
                    if let Some(name) = &child.name {
                         log.message(&format!("{name} - {}", &child.flex.clone().unwrap_or("".to_string())))
                    }
               }
         }
      } else {
          if lex_res.is_ok() {
               fun::run(&log, lex_tree, &mut real_targets)?
          }
      }
     
     if let Ok(elapsed) = sys_time.elapsed() {
               log.log(&format!("Finished in {}.{:<03} sec(s)", elapsed.as_secs(), elapsed.subsec_millis()))
     }
     io::stdout().flush()?;
     Ok(())
}