pub fn get_help() -> String {
    let help = r#"
rb [target [target2 [target3] ...]] [options] [-- parameter1 [...parameter2..]]
Options: 
  -help, -h              print this message
  -version, -V           print a version information
  -quiet, -q             be extra quiet
  -verbose, -v           be extra verbose
  -debug, -d             print a debugging information
  -logfile <file>        use given file for log
  -l       <file>          ''
  -noinput               do not allow interactive input
  -buildfile <file>      use given buildfile
  -file    <file>        ''
  -f       <file>        ''
  -keep-going, -k        execute all targets that do not depend
                         on failed target(s)
  -dry-run               do not launch any executable, but show all run parameters
  -r                     execute all targets accordingly dependencies anyway
  -D<property>=<value>   use a value for a given property name
  -propertyfile <name>   load all properties from file with -D
                         properties taking precedence
  -input                 input responses read from a pipe instead of a console
  -find [<file>]         (s)earch for buildfile towards the root of 
  -s    [<file>]         the filesystem and use it 
  -targethelp            print all target names in a build file with descriptions/comments
  -th                    ''
  --                     a separator of argumets passed to the build target executable
Examples: rb jar -d
          rb compile -s
          rb clean compile -r
          rb run -- arg1
"#;
   help.to_string()
}