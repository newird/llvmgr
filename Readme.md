# llvmgr

Helps you to download, compile and install LLVM.
Specially tailored for LLVM development with https://gitlab.com/taricorp/llvm-sys.rs

## Install

```
cargo install --git https://github.com/milkyapps/llvmgr
```

## Usage

```
> llvmgr install --help
Usage: llvmgr install [-v] <name> <version>

Install LLVM tools

Arguments:
  name           Options: llvm
  version        Options: [10-19]

Options:
  -v, --verbose  Be verbose.
  -h, --help     Show this help message and exit.
```

## Config
The default config will save in `.cache/llvmgr/config.toml`
```
[cache]
delete_src = "false" # delete source code after compile
delete_xz = "false" # delete zip file after download

[compile_config]
# same as parameters when compile LLVM , remove '-D' prefix
LLVM_ENABLE_PROJECTS = '"clang;lld"'
CMAKE_BUILD_TYPE = "Release"
LLVM_TARGETS_TO_BUILD = "X86"
LLVM_INSTALL_PREFIX = "."

[cmake]
B = "build"
G = "Ninja" # depends on system , Ninja for Linux and Visual Studio for Windows
S = "llvm"
```
## Shell Integration at Linux

Suggestion is to source the output of `llvmgr env bash` at your `.bashrc`.

```
eval "$(llvmgr env bash)"
```
or for `.config/fish/config.fish`

```
llvmgr env fish | source
```

This will export all installed versions as `LLVM_SYS_*_PREFIX` environment variables.

```
> llvmgr env bash
export LLVM_SYS_170_PREFIX=/home/<user>/.cache/llvmgr/17.0.6
export LLVM_SYS_180_PREFIX=/home/<user>/.cache/llvmgr/18.1.2
export LLVM_SYS_160_PREFIX=/home/<user>/.cache/llvmgr/16.0.1
```

```
> llvmgr env fish
set LLVM_SYS_170_PREFIX /home/<user>/.cache/llvmgr/17.0.6
set LLVM_SYS_180_PREFIX=/home/<user>/.cache/llvmgr/18.1.2
set LLVM_SYS_160_PREFIX=/home/<user>/.cache/llvmgr/16.0.1
```








