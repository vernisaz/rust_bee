# RustBee

## Purpose
RustBee is a lightweight version of 7Bee build tool written in Rust. RB has several
advantages over 7Bee as:
1. more concise and clear syntax of scripts
2. footprint is under 1Mb
3. more friendly to non Java builds
4. can work on systems where Java isn't supported

## Syntax highlights
RB build script defines at least one build target. Several build
targets can be dependent.

A script variable can be defined in the form:

    name=value[:type]

Name and value can be everything, but if a name includes spaces or symbols like `=, ;, {,( ` then the
name has to be quoted. If a name should include a quote, then use \ for escaping it.
The same rule is applied for a value. If one of the following characters `:, ;, [` is included in 
a value  then the value has to be quoted, for example:

    json lib="org.glassfish:javax.json:1.1.4":rep-maven
Although any name is allowed, all names starting with
*~* and ending with *~* are reserved.

- The name as ~~ is reserved for a result previous operation, it is similar to $? of a Bash script
- The separator for parts of a path is \~separator\~ or \~/\~
- The paths separator is \~path_separator\~
- The array of a command line arguments is \~args\~
- The string representing the current OS is \~os\~
- The current working directory is \~cwd\~
- The index of the current loop iteration is \~index\~
- The script file name is ~script~
- The current path to the current script \~script_path\~ (useful to specify path to an include script)

You can break a line by adding \ at the end.

A function call is defined as:

    function_name [name](parameter1,....)

If a function parameter includes one of the following symbols `,, ;, )` then it has to be escaped as described above.

A target is defined as :

 ```   
     target name:[work dir]:[description] {
        dependency {...}
         ...
        dependency {...}
        ....
        a target function or an operator
             ....
     }
```

A dependency can be:

- **anynewer**, function with two parameters, path to a file, second file has to be newer, use \* to compare an entire directory content
- **eq** block, specifies that all arguments must be equal, if only one argument specified, then the second considered as *none*
- **or** block, one of the arguments has to be true
- **target**, for dependency on a target
- **true**, for unconditional execution of the target

A body of a target contains a sequence of operators and functions. 
Currently *if*, *while*, *case*, and *for*  operators are supported. More details on syntax of them:

### if
```
     if {
       a condition function or a condition block
       then {
       }
      [ else {
      } ]
     }
```
### while
```
    while control_variable {
         # the loop body
    }
```
### for
```
    for var_name:array[:array elements separator if array defined as a scalar value] {
      # loop actions
    }
```
### case
```
    case var {
       choice pat1: {
           # do something
       }
       choice "pat2" | "pat3": {
           # do something
        }
       [ else {
           # when nothing matches
         }
    }
```
Ifs,  fors, cases, and whiles  can be nested.

A function can be one of the following:
- **and**, considers parameters as boolean values and returns true if all parameters are true
- **anynewer**, compares the modification time of a file specified by first parameter with
the second one. Use \* to consider a file with the latest modification time in the specified directory
- **array**, converts a list of parameters to an array, which can be consumed as the function result
- **as_jar**, returns jar file name for given Maven description - groupId:artifactId:version
- **as_url**, returns a download URL of an artifact specified by a parameter
- **ask**, prompts a console using first parameter, and then read a user input, second parameter is used for the default answer, when a user press the enter
- **assign**, first parameter is a *name* of variable, the second is a value, the function returns a previous value under the name, if any,
no value parameter means cleaning the variable parameter
- **calc**, a calculator function, it uses one parameter specifying an expression, **float** values are used and four operations accordingly their priority, parenthesis are acknowledged
- **canonicalize** | **absolute**,  converts a path if a relative to an absolute form in the current directory context
- **cfg**, return the common path using for storing app config data
- **contains** | **find**, check if first parameter contains a content of the second. Returns value of true if contains
- **cp**, file copy command similar used for Unix. Pairs of parameter are not limited
- **cropname**, cut a part of the name specified by fist parameter by a matching second one (\* means a variable part and can be ommited at the end) 
and replace it with 3rd parameter when it's specified
- **display** - display a message specified by a parameter
- **element**, set/get an element of an array, first parameter specifies an array, second an index, and optional 3rd, when a value has to be set
- **eq**, compares two parameters and returns true if they are equal, only one parameter compares it with *None*
- **exec**, executes a process on the underline OS, a name of a process separated by a blank from *exec*, 
parameters are parameters of the process, a current directory, and a variable to keep the process stdout can be
specified after a process name separated by ':', otherwise stdout will appear on screen
- **filename**, returns a filename of a parameter, no extension
- **files**, return an array of file paths matching patterns specified by parameters
- **file_filter** | **filter** , shrink an array specified my first parameters by filter values specified by extra parameters
- **gt** , first argument is greater than second one
- **include**, includes a file content pointed by a parameter as a part of the script 
- **lt** , first argument is littler than second one
- **mkd**, creates directories from the list of parameters. It returns an array of successfully created directories. Directories get created from current work directory unless a fully qualified name is specified
- **mv**, similar to cp, but does a move
- **newerthan**, compares a timestamp of files specified with the pattern path/.ext with a timestamp of files specified using the path/.ext and
returns an array of files which have the later date
- **neq**,  compares two parameters and returns true if they are not equal, only one parameter compares with *None*
- **not** , invert boolean value of the expression of the parameter 
- **now**, shows the current time and date in ISO 8601, or in a format specified by a parameter, the following letters are allowed in the format: W, MMM-DD-YY hh:mm:ss Z
- **number**, converts an argument in a number and returns as a result  
- **or**, considers parameters as boolean values and returns true of first true parameter,
otherwise returns false
- *panic*, a parameter specifies a panic message, and stops the script execution
- **range**, returns a range of first parameter specified by a start by second parameter and an end specified by third parameter, when presented
- **read**, reads a file content specified by a parameter
- **rm**, removes files defined in parameters
- **rmdir**, **rmdira** removes an empty directory (rmdir), or a directory with all content (rmdira) specified in parameters
- **scalar** | **join** , if a parameter is an array, then concatenates all elements using a separator specified by second parameter
- **set_env**, set the environment key specified by first parameter to the value specified by the second one
- **timestamp**, returns a timestamp of a file specified by a parameter
- **write**, writes to the file specified by first parameter, content of the rest parameters
- **writea**, writes to the file specified by first parameter, content of the rest parameters. It doesn't create a new file if it already exists,
just append content
- **zip**, write a zip file, a name is specified by the first parameter and a content is specified by the following parameter pairs. A pair can be:

    * -\<A|E\> zip dir/name, content (when E specified, the content gets the execute permission under UNIX)
    * -C zip dir, dir with a possible file wildcard name (all directories below are processed)
    * -B zip dir, dir with a possible file wildcard name (no traverse of directories). There's a possibility
of using an array of file paths, however to prevent an array flatten, use a name a var storing a name of the array of paths, like:
```
assign(+TJWS libs, TJWS libs);
zip(${distro dir}${~/~}rds-${version}.zip,
  -B lib,
  +TJWS libs) # it's a reference to an array of component paths
```
The function returns the stored zip path, or nothing in  a case of errors.

A result of a function or a block is stored in a temporary variable **\~\~** and can be consumed in the next operation. 

### String interpolation
It allows to extend any value by processing template variables  like:

       ${name}

The name is a name of some variable. Since a substituted value has to be interpolated as well,
the process is recursive. It doesn't do check for looping though, and you need to prevent of it happens.

### name or value?
Rustbee resolves this ambiguity in the following manner. First it considers the value as a name of a variable and is looking for it.
If the variable with such name wasn't found, then the value is considered as a literal value. 
A string interpolation is applied at the end of any variant. 

## Examples

An example of a script for building a Java project, can be found [there](https://github.com/drogatkin/JustDSD/blob/master/bee-java.rb
Another example is [common Rust scriptd](https://github.com/vernisaz/simscript).
