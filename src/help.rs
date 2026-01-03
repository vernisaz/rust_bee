pub fn get_help() -> String {
    r#"
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
  -buildfile <file>      use the given script file
  -file    <file>        ''
  -f       <file>        ''
  -keep-going, -k        execute all targets that do not depend
                         on failed target(s)
  -dry-run               do not launch any executable, but show their arguments
  -r                     execute all targets accordingly dependencies anyway
  -D<property>=<value>   use a value for a given property name
  -propertyfile <name>   load all properties from file with -D
                         properties taking precedence
  -find [<file>]         (s)earch for a script file towards the root of 
  -s    [<file>]         the filesystem and then use it 
  -targethelp            print all target names in the script file with descriptions/comments
  -th                    ''
  --                     a separator of argumets passed to the script target executable
Examples: rb jar -d
          rb compile -s
          rb clean compile -r
          rb run -- arg1
"#.to_string()
}