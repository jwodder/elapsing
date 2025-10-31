[![Project Status: WIP – Initial development is in progress, but there has not yet been a stable, usable release suitable for the public.](https://www.repostatus.org/badges/latest/wip.svg)](https://www.repostatus.org/#wip)
[![CI Status](https://github.com/jwodder/elapsing/actions/workflows/test.yml/badge.svg)](https://github.com/jwodder/elapsing/actions/workflows/test.yml)
[![codecov.io](https://codecov.io/gh/jwodder/elapsing/branch/main/graph/badge.svg)](https://codecov.io/gh/jwodder/elapsing)
[![Minimum Supported Rust Version](https://img.shields.io/badge/MSRV-1.90-orange)](https://www.rust-lang.org)
[![MIT License](https://img.shields.io/github/license/jwodder/elapsing.svg)](https://opensource.org/licenses/MIT)

[GitHub](https://github.com/jwodder/elapsing) | [Issues](https://github.com/jwodder/elapsing/issues)

`elapsing` is a simple utility program that runs a given command and, while
it's running, displays & updates the elapsed time below the command's output.

![Recording of an example invocation](https://github.com/jwodder/elapsing/raw/main/example.gif)


Installation
============

In order to install `elapsing`, you first need to have [Rust and Cargo
installed](https://www.rust-lang.org/tools/install).  You can then build the
latest version of `elapsing` and install it in `~/.cargo/bin` by running:

    cargo install --git https://github.com/jwodder/elapsing


Restrictions
============

`elapsing` is intended for use with commands with line-oriented output.  If it
is used with a command that outputs a large amount of data between newlines or
that manipulates the cursor, you'll have a bad experience.
