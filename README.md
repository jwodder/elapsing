[![Project Status: Active – The project has reached a stable, usable state and is being actively developed.](https://www.repostatus.org/badges/latest/active.svg)](https://www.repostatus.org/#active)
[![CI Status](https://github.com/jwodder/elapsed/actions/workflows/test.yml/badge.svg)](https://github.com/jwodder/elapsed/actions/workflows/test.yml)
[![codecov.io](https://codecov.io/gh/jwodder/elapsed/branch/main/graph/badge.svg)](https://codecov.io/gh/jwodder/elapsed)
[![Minimum Supported Rust Version](https://img.shields.io/badge/MSRV-1.85-orange)](https://www.rust-lang.org)
[![MIT License](https://img.shields.io/github/license/jwodder/elapsed.svg)](https://opensource.org/licenses/MIT)

[GitHub](https://github.com/jwodder/elapsed) | [crates.io](https://crates.io/crates/elapsed-cmd) | [Issues](https://github.com/jwodder/elapsed/issues) | [Changelog](https://github.com/jwodder/elapsed/blob/main/CHANGELOG.md)

`elapsed` is a simple utility program that runs a given command and, while
it's running, displays & updates the elapsed time below the command's output.

![Recording of an example invocation](https://github.com/jwodder/elapsed/raw/main/example.gif)


Installation
============

In order to install `elapsed`, you first need to have [Rust and Cargo
installed](https://www.rust-lang.org/tools/install).  You can then build the
latest release of `elapsed` and install it in `~/.cargo/bin` by running:

    cargo install elapsed-cmd

(Note that the name of the package on crates.io is "elapsed-cmd"; just
"elapsed" was already taken.)


Usage
=====

    elapsed [<options>] <command> [<arg> ...]

`elapsed` takes the name of a command to run plus any arguments to that
command.  While the command is running, the elapsed time is displayed in a
status line written to standard error below the command's output and updated
once per second.  If `elapsed`'s standard error is redirected, the status line
will not be shown.

When the command exits, the status line is erased (unless the `-t`/`--total`
option was given), and `elapsed` exits with the same return code as the
command; if the command was killed by a signal, a message is printed to stderr
(after any `--total` status line), and `elapsed` exits with return code 1
instead.

Options
-------

- `-f <TEMPLATE>`, `--format <TEMPLATE>` — Use the given format template for
  rendering the time in the status line.  Templates may contain the following
  specifiers & escape sequences:

    - `%H` - number of hours as a two-digit number
    - `%M` - number of minutes within the hour as a two-digit number
    - `%S` - number of seconds within the minute as a two-digit number
    - `%s` - total number of seconds
    - `%f` - subseconds; defaults to six digits (microseconds) but can also
      take a decimal precision (e.g., `%9f` to show nanoseconds)
    - `%n` or `\n` - newline
    - `%t` or `\t` - tab
    - `%e` or `\e` - escape character
    - `%%` - percent sign
    - `\\` - backslash

    The default template is "`Elapsed: %H:%M:%S`."

- `-r <INT>`, `--refresh <INT>` — Update the status line after every `<INT>`
  milliseconds [default: 1000 (once per second)]

- `-t`, `--total` — Leave the total elapsed time behind after the command finishes

- `-T`, `--tty` — Run the command via a pseudo-terminal.  This is useful if the
  command's behavior (e.g., whether it buffers output or emits ANSI color
  sequences) changes depending on whether it's directly connected to a
  terminal.

  This option is only available on Unix.

- `-S`, `--split-stderr` — When the `--tty` option is given, by default, the
  command's stdout and stderr will be merged together into a single stream that
  is ultimately printed on `elapsed`'s stdout.  If `--split-stderr` is given,
  the command's stderr will not be connected to the pseudo-terminal and will
  instead be sent to `elapsed`'s stderr so that stdout & stderr can then be
  redirected separately.

  This option is only available on Unix and only has an effect when `--tty` is
  also given.

- `-h`, `--help` — Show command-line usage

- `-V`, `--version` — Show current program version


Restrictions
============

`elapsed` is intended for use with commands with line-oriented output.  If it
is used with a command that outputs a large amount of data between newlines or
that manipulates the cursor, you'll have a bad experience.
