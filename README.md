# rrecycle (rust+recycle)

A cross-platform terminal file manager with recycle bin support.

![Basic usage](https://vhs.charm.sh/vhs-311lSVb9LGhZ7euYbrX615.gif)

## Features

- Delete files permanently
- Move files to the recycle bin
- Restore from the recycle bin
- List files in the recycle bin
- "Shred" files - securely delete them by overwriting them first. 


## Installation
To install the project, use Cargo or download a prebuilt binary from GitHub.

```bash
  cargo install rrecycle
  rrc --help
```

Please note that due to limitations in the underlying library which handles interactions with the recycle bin, the project will not compile on MacOS. 

## Usage
```bash
rrc [OPTIONS] <COMMAND>

Subcommands
  trash, -t    Move files to the recycle bin
  restore, -r  Restore files from the recycle bin
  purge, -p    Remove files from the recycle bin
  delete, -d   Delete files permanently
  shred, -s    Securely delete files by overwriting them first
  list, -l     List files in the recycle bin
  help         Print this message or the help of the given subcommand(s)
Options
  -R, --recurse  Run delete and shred on directories without a prompt
  -h, --help     Print help
  -V, --version  Print version
```
    
## Contributing

Any contributions are very welcome! However, this project uses some rules for code
structure which you must follow.

### Project structure
This is a split library and binary project. 

The library contains most of what the application actually *does* to your filesystem, as well as utility functions. Functions and types that go in the library are independent from the binary, testable, and generic if possible. 

The binary should control application flow, handle user input, and display output. 
e.g it should not:

- Check if files exist
- Recurse over directories
- Contain overly generic functions

However, things should also not be removed from the binary for the sake of it. For example, calls to functions defined in the underlying trash library (trash-rs) are fine. Wrapper functions should be avoided. 

This helps with strictly seperating concerns between application flow and what the application actually does (which terminal tools of this kind often struggle with).

