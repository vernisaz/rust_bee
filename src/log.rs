//use std::io::{self, BufRead, Write};
pub struct Log {
    pub verbose: bool,
    pub debug: bool,
    pub quiet: bool,
   // writer: Write
}

impl Log {
    pub fn log(&self, msg: &str) {
        if self.verbose && !self.quiet {
            println!("{}", msg); // write!(&mut writer, "{}", msg);
        }
    }
    
    pub fn debug(&self, msg: &str) {
        if self.debug && !self.quiet {
            println!("{}", msg);
        }
    }

    pub fn error(&self, msg: &str) {
        if !self.quiet {
             eprintln!("\x1b[0;31mError: {}\x1b[0m", msg);
        }
    }   
    
    pub fn warning(&self, msg: &str) {
        if self.verbose && !self.quiet {
             println!("\x1b[0;33mWarning: {}\x1b[0m", msg);
        }
    }

    pub fn message(&self, msg: &str) {
        if !self.quiet {
            println!("{}", msg);
        }
    }
}