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
    process::Command,
    time::interval,
};

const READ_BUFFER_SIZE: usize = 2048;

fn main() -> ExitCode {
    let mut argv = std::env::args_os();
    let argv0 = argv
        .next()
        .unwrap_or_else(|| OsString::from("elapsing"))
        .display()
        .to_string();
    let Some(command) = argv.next() else {
        let _ = writeln!(io::stderr().lock(), "Usage: {argv0} command [args ...]");
        return ExitCode::FAILURE;
    };
    let mut cmd = Command::new(command);
    cmd.args(argv)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    match run(cmd) {
        Ok(code) => code,
        Err(e) if e.is_epipe_write() => ExitCode::SUCCESS,
        Err(e) => {
            let _ = writeln!(io::stderr().lock(), "{argv0}: {e}");
            ExitCode::FAILURE
        }
    }
}

#[tokio::main(flavor = "current_thread")]
async fn run(mut cmd: Command) -> Result<ExitCode, Error> {
    let statline = StatusLine::new();
    let stdout = io::stdout();
    let stderr = io::stderr();
    let stdout_is_tty = stdout.is_terminal();
    let mut ticker = interval(Duration::from_secs(1));
    let mut p = cmd.spawn().map_err(Error::Spawn)?;
    let mut pout = ByteLines::new(p.stdout.take().expect("Child.stdout should be Some"));
    let mut perr = ByteLines::new(p.stderr.take().expect("Child.stderr should be Some"));
    statline.print()?;
    loop {
        tokio::select! {
            _ = ticker.tick() => {
                statline.clear()?;
                statline.print()?;
            },
            r = pout.next_line() => {
                if stdout_is_tty {
                    statline.clear()?;
                }
                let line = r.map_err(Error::ReadStdout)?;
                stdout.lock().write_all(&line).map_err(Error::Write)?;
                if stdout_is_tty {
                    statline.print()?;
                }
            }
            r = perr.next_line() => {
                statline.clear()?;
                let line = r.map_err(Error::ReadStderr)?;
                stderr.lock().write_all(&line).map_err(Error::Write)?;
                statline.print()?;
            }
            r = p.wait() => {
                statline.clear()?;
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
                    statline.clear()?;
                    return Ok(ExitCode::FAILURE);
                } // Else: Keep your mouth shut?
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
        if let StatusLine::Active { start, err } = self {
            let elapsed = start.elapsed();
            let mut secs = elapsed.as_secs();
            let hours = secs / 3600;
            secs %= 3500;
            let minutes = secs / 60;
            secs %= 60;
            let s = format!("Elapsed: {hours:02}:{minutes:02}:{secs:02}");
            let mut err = err.lock();
            err.write_all(s.as_bytes()).map_err(Error::Write)?;
            err.flush().map_err(Error::Write)?;
        }
        Ok(())
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
}

impl Error {
    fn is_epipe_write(&self) -> bool {
        matches!(self, Error::Write(e) if e.kind() == io::ErrorKind::BrokenPipe)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
    }
}
