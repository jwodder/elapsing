use cfg_if::cfg_if;
use lexopt::{Arg, Parser};
use std::ffi::OsString;
use std::future::Future;
use std::io::{self, IsTerminal, Write};
use std::pin::{Pin, pin};
use std::process::{ExitCode, ExitStatus, Stdio};
use std::task::{Context, Poll, ready};
use std::time::{Duration, Instant};
use thiserror::Error;
use tokio::{
    io::{AsyncRead, ReadBuf},
    process::{Child, ChildStderr, ChildStdout, Command},
    time::{Interval, interval},
};

const READ_BUFFER_SIZE: usize = 2048;

#[derive(Clone, Debug, Eq, PartialEq)]
enum Arguments {
    Run(Elapsed),
    Help,
    Version,
}

impl Arguments {
    fn from_parser(mut parser: Parser) -> Result<Arguments, lexopt::Error> {
        let mut total = false;
        #[cfg(unix)]
        let mut tty = false;
        #[cfg(unix)]
        let mut split_stderr = false;
        while let Some(arg) = parser.next()? {
            match arg {
                #[cfg(unix)]
                Arg::Short('S') | Arg::Long("split-stderr") => split_stderr = true,
                #[cfg(not(unix))]
                Arg::Short('S') | Arg::Long("split-stderr") => {
                    return Err("--split-stderr is not supported on this system".into());
                }
                Arg::Short('t') | Arg::Long("total") => total = true,
                #[cfg(unix)]
                Arg::Short('T') | Arg::Long("tty") => tty = true,
                #[cfg(not(unix))]
                Arg::Short('T') | Arg::Long("tty") => {
                    return Err("--tty is not supported on this system".into());
                }
                Arg::Short('h') | Arg::Long("help") => return Ok(Arguments::Help),
                Arg::Short('V') | Arg::Long("version") => return Ok(Arguments::Version),
                Arg::Value(cmd) => {
                    let args = parser.raw_args()?.collect::<Vec<_>>();
                    cfg_if! {
                        if #[cfg(unix)] {
                            return Ok(Arguments::Run(Elapsed { cmd, args, total, tty, split_stderr }));
                        } else {
                            return Ok(Arguments::Run(Elapsed { cmd, args, total }));
                        }
                    }
                }
                _ => return Err(arg.unexpected()),
            }
        }
        Err("no command supplied".into())
    }

    fn run(self) -> Result<ExitCode, Error> {
        match self {
            Arguments::Run(elapsed) => run(elapsed),
            Arguments::Help => {
                write!(
                    io::stdout().lock(),
                    concat!(
                        "Usage: elapsed [<options>] <command> [<arg> ...]\n",
                        "\n",
                        "Show runtime while a command runs\n",
                        "\n",
                        "Visit <https://github.com/jwodder/elapsed> for more information.\n",
                        "\n",
                        "Options:\n",
                        "  -t, --total       Leave total elapsed time behind after command finishes\n",
                        "\n",
                        "  -T, --tty         Run command via a pseudo-terminal [Unix only]\n",
                        "\n",
                        "  -S, --split-stderr\n",
                        "                    When used with --tty, send the command's stderr directly to\n",
                        "                    elapsed's stderr instead of unifying with stdout via the\n",
                        "                    pseudo-terminal [Unix only]\n",
                        "\n",
                        "  -h, --help        Display this help message and exit\n",
                        "  -V, --version     Show the program version and exit\n",
                    )
                )
                .map_err(Error::Write)?;
                Ok(ExitCode::SUCCESS)
            }
            Arguments::Version => {
                writeln!(
                    io::stdout().lock(),
                    "{} {}",
                    env!("CARGO_PKG_NAME"),
                    env!("CARGO_PKG_VERSION")
                )
                .map_err(Error::Write)?;
                Ok(ExitCode::SUCCESS)
            }
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct Elapsed {
    cmd: OsString,
    args: Vec<OsString>,
    total: bool,
    #[cfg(unix)]
    tty: bool,
    #[cfg(unix)]
    split_stderr: bool,
}

impl Elapsed {
    fn spawn(&self) -> Result<(Child, ByteLines<ChildOutput>, ByteLines<ChildOutput>), Error> {
        cfg_if! {
            if #[cfg(unix)] {
                if self.tty {
                    self.spawn_tty()
                } else {
                    self.spawn_plain()
                }
            } else {
                self.spawn_plain()
            }
        }
    }

    fn spawn_plain(
        &self,
    ) -> Result<(Child, ByteLines<ChildOutput>, ByteLines<ChildOutput>), Error> {
        let mut p = Command::new(&self.cmd)
            .args(&self.args)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true)
            .spawn()
            .map_err(Error::Spawn)?;
        let pout = ByteLines::new(ChildOutput::Stdout(
            p.stdout.take().expect("Child.stdout should be Some"),
        ));
        let perr = ByteLines::new(ChildOutput::Stderr(
            p.stderr.take().expect("Child.stderr should be Some"),
        ));
        Ok((p, pout, perr))
    }

    fn spawn_tty(&self) -> Result<(Child, ByteLines<ChildOutput>, ByteLines<ChildOutput>), Error> {
        let (pty, pts) = pty_process::open().map_err(Error::InitPty)?;
        if let Some((width, height)) = terminal_size::terminal_size() {
            pty.resize(pty_process::Size::new(width.0, height.0))
                .map_err(Error::InitPty)?;
        }
        let mut cmd = pty_process::Command::new(&self.cmd)
            .args(&self.args)
            .stdin(Stdio::inherit())
            .kill_on_drop(true);
        if self.split_stderr {
            cmd = cmd.stderr(Stdio::piped());
        }
        let mut p = cmd.spawn(pts).map_err(Error::SpawnPty)?;
        let perr = if self.split_stderr {
            ChildOutput::Stderr(p.stderr.take().expect("Child.stderr should be Some"))
        } else {
            ChildOutput::Null
        };
        Ok((
            p,
            ByteLines::new(ChildOutput::Pty(pty)),
            ByteLines::new(perr),
        ))
    }
}

fn main() -> ExitCode {
    match Arguments::from_parser(Parser::from_env())
        .map_err(Error::Usage)
        .and_then(Arguments::run)
    {
        Ok(code) => code,
        Err(e) if e.is_epipe_write() => ExitCode::SUCCESS,
        Err(e) => {
            let _ = writeln!(io::stderr().lock(), "elapsed: {e}");
            ExitCode::FAILURE
        }
    }
}

#[tokio::main(flavor = "current_thread")]
async fn run(app: Elapsed) -> Result<ExitCode, Error> {
    let statline = StatusLine::new();
    let stdout = io::stdout();
    let stderr = io::stderr();
    let stdout_is_tty = stdout.is_terminal();
    let ticker = interval(Duration::from_secs(1));
    let (p, pout, perr) = app.spawn()?;
    let mut elapsing = Elapsing {
        statline,
        p,
        pout,
        perr,
        stdout,
        stderr,
        stdout_is_tty,
        ticker,
    };
    elapsing.statline.print()?;
    let r = elapsing.event_loop().await;
    if app.total {
        elapsing.statline.print_total()?;
    }
    r
}

struct Elapsing {
    statline: StatusLine,
    p: Child,
    pout: ByteLines<ChildOutput>,
    perr: ByteLines<ChildOutput>,
    stdout: io::Stdout,
    stderr: io::Stderr,
    stdout_is_tty: bool,
    ticker: Interval,
}

impl Elapsing {
    async fn event_loop(&mut self) -> Result<ExitCode, Error> {
        loop {
            tokio::select! {
                _ = self.ticker.tick() => {
                    self.statline.clear()?;
                    self.statline.print()?;
                },
                r = self.pout.next_line() => {
                    if self.stdout_is_tty {
                        self.statline.clear()?;
                    }
                    let line = r.map_err(Error::ReadStdout)?;
                    self.stdout.lock().write_all(&line).map_err(Error::Write)?;
                    if self.stdout_is_tty {
                        self.statline.print()?;
                    }
                }
                r = self.perr.next_line() => {
                    self.statline.clear()?;
                    let line = r.map_err(Error::ReadStderr)?;
                    self.stderr.lock().write_all(&line).map_err(Error::Write)?;
                    self.statline.print()?;
                }
                r = self.p.wait() => {
                    self.statline.clear()?;
                    let rc = r.map_err(Error::Wait)?;
                    if let Some(ret) = rc.code() {
                        let ret = u8::try_from(ret & 255).unwrap_or(1);
                        return Ok(ExitCode::from(ret));
                    } else {
                        return Err(Error::Signal(rc));
                    }
                }
                r = tokio::signal::ctrl_c() => {
                    if r.is_ok() {
                        self.statline.clear()?;
                        return Ok(ExitCode::FAILURE);
                    } // Else: Keep your mouth shut?
                }
            }
        }
    }
}

#[derive(Debug)]
enum StatusLine {
    Active { start: Instant, err: io::Stderr },
    Inactive,
}

impl StatusLine {
    fn new() -> StatusLine {
        let err = io::stderr();
        if err.is_terminal() {
            StatusLine::Active {
                start: Instant::now(),
                err,
            }
        } else {
            StatusLine::Inactive
        }
    }

    fn clear(&self) -> Result<(), Error> {
        if let StatusLine::Active { err, .. } = self {
            let mut err = err.lock();
            err.write_all(b"\r\x1B[K").map_err(Error::Write)?;
            err.flush().map_err(Error::Write)?;
        }
        Ok(())
    }

    fn print(&self) -> Result<(), Error> {
        self.inner_print(false)
    }

    fn print_total(&self) -> Result<(), Error> {
        self.inner_print(true)
    }

    fn inner_print(&self, nl: bool) -> Result<(), Error> {
        if let StatusLine::Active { start, err } = self {
            let elapsed = start.elapsed();
            let mut secs = elapsed.as_secs();
            let hours = secs / 3600;
            secs %= 3500;
            let minutes = secs / 60;
            secs %= 60;
            let mut s = format!("Elapsed: {hours:02}:{minutes:02}:{secs:02}");
            if nl {
                s.push('\n');
            }
            let mut err = err.lock();
            err.write_all(s.as_bytes()).map_err(Error::Write)?;
            err.flush().map_err(Error::Write)?;
        }
        Ok(())
    }
}

enum ChildOutput {
    Stdout(ChildStdout),
    Stderr(ChildStderr),
    #[cfg(unix)]
    Pty(pty_process::Pty),
    #[cfg(unix)]
    Null,
}

impl AsyncRead for ChildOutput {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        match &mut *self {
            ChildOutput::Stdout(out) => {
                let out = pin!(out);
                out.poll_read(cx, buf)
            }
            ChildOutput::Stderr(err) => {
                let err = pin!(err);
                err.poll_read(cx, buf)
            }
            #[cfg(unix)]
            ChildOutput::Pty(pty) => {
                let pty = pin!(pty);
                // On Linux, attempting to read from a pty master after the
                // slave closes (due, e.g., to the child process exiting)
                // results in EIO (which Rust currently represents with the
                // undocumented ErrorKind::Uncategorized).
                match ready!(pty.poll_read(cx, buf)) {
                    Err(e) if e.raw_os_error() == Some(5) => Ok(()).into(),
                    r => r.into(),
                }
            }
            #[cfg(unix)]
            ChildOutput::Null => Ok(()).into(),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct ByteLines<R> {
    reader: R,
    buffer: Vec<u8>,
    next_index: usize,
    eof: bool,
}

impl<R> ByteLines<R> {
    fn new(reader: R) -> Self {
        ByteLines {
            reader,
            buffer: Vec::new(),
            next_index: 0,
            eof: false,
        }
    }

    fn get_line(&mut self) -> Option<Vec<u8>> {
        if let Some(i) = self.buffer[self.next_index..]
            .iter()
            .position(|&b| b == b'\n')
        {
            let newline_pos = self.next_index + i;
            self.next_index = 0;
            Some(self.buffer.drain(0..=newline_pos).collect())
        } else if self.eof {
            let r = (!self.buffer.is_empty()).then(|| std::mem::take(&mut self.buffer));
            self.next_index = 0;
            r
        } else {
            self.next_index = self.buffer.len();
            None
        }
    }

    fn next_line<'a>(&'a mut self) -> NextLine<'a, R> {
        NextLine { inner: self }
    }
}

#[derive(Debug, Eq, PartialEq)]
struct NextLine<'a, R> {
    inner: &'a mut ByteLines<R>,
}

impl<R: AsyncRead + Unpin> Future for NextLine<'_, R> {
    type Output = io::Result<Vec<u8>>;

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        loop {
            if let Some(ln) = self.inner.get_line() {
                return Poll::Ready(Ok(ln));
            } else if self.inner.eof {
                return Poll::Pending;
            } else {
                let mut buf0 = vec![0u8; READ_BUFFER_SIZE];
                let mut buf = ReadBuf::new(&mut buf0);
                let reader = pin!(&mut self.inner.reader);
                match ready!(reader.poll_read(cx, &mut buf)) {
                    Ok(()) => {
                        if buf.filled().is_empty() {
                            self.inner.eof = true;
                        } else {
                            self.inner.buffer.extend_from_slice(buf.filled());
                        }
                    }
                    Err(e) => return Err(e).into(),
                }
            }
        }
    }
}

#[derive(Debug, Error)]
enum Error {
    #[error(transparent)]
    Usage(lexopt::Error),
    #[error("failed to spawn child process: {0}")]
    Spawn(io::Error),
    #[error(transparent)]
    Write(io::Error),
    #[error("error reading from child process's stdout: {0}")]
    ReadStdout(io::Error),
    #[error("error reading from child process's stderr: {0}")]
    ReadStderr(io::Error),
    #[error("error waiting for child process to terminate: {0}")]
    Wait(io::Error),
    #[error("child process killed by signal: {0}")]
    Signal(ExitStatus),
    #[cfg(unix)]
    #[error("error initializing pty: {0}")]
    InitPty(pty_process::Error),
    #[cfg(unix)]
    #[error("failed to spawn child process on pty: {0}")]
    SpawnPty(pty_process::Error),
}

impl Error {
    fn is_epipe_write(&self) -> bool {
        matches!(self, Error::Write(e) if e.kind() == io::ErrorKind::BrokenPipe)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    mod parse_args {
        use super::*;
        use assert_matches::assert_matches;

        #[test]
        fn command_only() {
            let parser = Parser::from_iter(["elapsed", "foo"]);
            assert_matches!(Arguments::from_parser(parser).unwrap(), Arguments::Run(app) => {
                assert_eq!(app.cmd, "foo");
                assert!(app.args.is_empty());
                assert!(!app.total);
            });
        }

        #[test]
        fn command_with_arg() {
            let parser = Parser::from_iter(["elapsed", "foo", "bar"]);
            assert_matches!(Arguments::from_parser(parser).unwrap(), Arguments::Run(app) => {
                assert_eq!(app.cmd, "foo");
                assert_eq!(app.args, ["bar"]);
            });
        }

        #[test]
        fn command_with_opt() {
            let parser = Parser::from_iter(["elapsed", "foo", "--bar"]);
            assert_matches!(Arguments::from_parser(parser).unwrap(), Arguments::Run(app) => {
                assert_eq!(app.cmd, "foo");
                assert_eq!(app.args, ["--bar"]);
            });
        }

        #[test]
        fn command_with_my_opt() {
            let parser = Parser::from_iter(["elapsed", "foo", "--help"]);
            assert_matches!(Arguments::from_parser(parser).unwrap(), Arguments::Run(app) => {
                assert_eq!(app.cmd, "foo");
                assert_eq!(app.args, ["--help"]);
            });
        }

        #[test]
        fn help() {
            let parser = Parser::from_iter(["elapsed", "--help"]);
            assert_eq!(Arguments::from_parser(parser).unwrap(), Arguments::Help);
        }

        #[test]
        fn version() {
            let parser = Parser::from_iter(["elapsed", "--version"]);
            assert_eq!(Arguments::from_parser(parser).unwrap(), Arguments::Version);
        }

        #[test]
        fn help_and_command() {
            let parser = Parser::from_iter(["elapsed", "--help", "foo"]);
            assert_eq!(Arguments::from_parser(parser).unwrap(), Arguments::Help);
        }

        #[test]
        fn total() {
            let parser = Parser::from_iter(["elapsed", "--total", "foo"]);
            assert_matches!(Arguments::from_parser(parser).unwrap(), Arguments::Run(app) => {
                assert_eq!(app.cmd, "foo");
                assert!(app.args.is_empty());
                assert!(app.total);
            });
        }

        #[test]
        fn double_dash_command() {
            let parser = Parser::from_iter(["elapsed", "--", "foo"]);
            assert_matches!(Arguments::from_parser(parser).unwrap(), Arguments::Run(app) => {
                assert_eq!(app.cmd, "foo");
                assert!(app.args.is_empty());
            });
        }

        #[test]
        fn double_dash_opt() {
            let parser = Parser::from_iter(["elapsed", "--", "--help"]);
            assert_matches!(Arguments::from_parser(parser).unwrap(), Arguments::Run(app) => {
                assert_eq!(app.cmd, "--help");
                assert!(app.args.is_empty());
            });
        }

        #[test]
        fn command_double_dash() {
            let parser = Parser::from_iter(["elapsed", "foo", "--", "bar"]);
            assert_matches!(Arguments::from_parser(parser).unwrap(), Arguments::Run(app) => {
                assert_eq!(app.cmd, "foo");
                assert_eq!(app.args, ["--", "bar"]);
            });
        }
    }

    mod byte_lines {
        use super::*;
        use std::io::Cursor;
        use tokio_test::{assert_pending, io::Builder, task::spawn};

        #[tokio::test]
        async fn many_short_lines() {
            let reader = Cursor::new(b"Hello!\nI like your code.\nGoodbye!\n");
            let mut lines = ByteLines::new(reader);
            assert_eq!(lines.next_line().await.unwrap(), b"Hello!\n");
            assert_eq!(lines.next_line().await.unwrap(), b"I like your code.\n");
            assert_eq!(lines.next_line().await.unwrap(), b"Goodbye!\n");
            let mut fut = spawn(lines.next_line());
            assert_pending!(fut.poll());
        }

        #[tokio::test]
        async fn many_short_lines_no_final_newline() {
            let reader = Cursor::new(b"Hello!\nI like your code.\nGoodbye!");
            let mut lines = ByteLines::new(reader);
            assert_eq!(lines.next_line().await.unwrap(), b"Hello!\n");
            assert_eq!(lines.next_line().await.unwrap(), b"I like your code.\n");
            assert_eq!(lines.next_line().await.unwrap(), b"Goodbye!");
            let mut fut = spawn(lines.next_line());
            assert_pending!(fut.poll());
        }

        #[tokio::test]
        async fn split_line() {
            let reader = Builder::new()
                .read(b"Hello, ")
                .read(b"World!\n")
                .read(b"Bye now!\n")
                .build();
            let mut lines = ByteLines::new(reader);
            assert_eq!(lines.next_line().await.unwrap(), b"Hello, World!\n");
            assert_eq!(lines.next_line().await.unwrap(), b"Bye now!\n");
            let mut fut = spawn(lines.next_line());
            assert_pending!(fut.poll());
        }

        #[tokio::test]
        async fn non_utf8() {
            let reader = Cursor::new(b"Hell\xF6!\nI like your code.\nGoodbye!\n");
            let mut lines = ByteLines::new(reader);
            assert_eq!(lines.next_line().await.unwrap(), b"Hell\xF6!\n");
            assert_eq!(lines.next_line().await.unwrap(), b"I like your code.\n");
            assert_eq!(lines.next_line().await.unwrap(), b"Goodbye!\n");
            let mut fut = spawn(lines.next_line());
            assert_pending!(fut.poll());
        }
    }
}
