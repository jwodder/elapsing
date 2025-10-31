use anyhow::Context as _;
use std::ffi::OsString;
use std::future::Future;
use std::io::{self, Write};
use std::pin::{pin, Pin};
use std::process::{ExitCode, Stdio};
use std::task::{ready, Context, Poll};
use std::time::{Duration, Instant};
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
        Err(e) => {
            // Unlike most uses of this idiom, don't go through e.chain()
            // looking for a BrokenPipe, as we only want to check errors raised
            // by failure to write to stdout/stderr, which are those for which
            // the payload is io::Error.  If we were to go through e.chain(),
            // then we might get false positives if reading from the child
            // process failed with EPIPE (Can that even happen?  Not sure.)
            if let Some(ioerr) = e.downcast_ref::<io::Error>() {
                if ioerr.kind() == io::ErrorKind::BrokenPipe {
                    return ExitCode::SUCCESS;
                }
            }
            let _ = writeln!(io::stderr().lock(), "{argv0}: {e:?}");
            ExitCode::FAILURE
        }
    }
}

#[tokio::main(flavor = "current_thread")]
async fn run(mut cmd: Command) -> anyhow::Result<ExitCode> {
    let statline = StatusLine::new();
    let mut ticker = interval(Duration::from_secs(1));
    let mut p = cmd.spawn().context("failed to start command")?;
    let mut stdout = ByteLines::new(p.stdout.take().expect("Child.stdout should be Some"));
    let mut stderr = ByteLines::new(p.stderr.take().expect("Child.stderr should be Some"));
    statline.print()?;
    loop {
        tokio::select! {
            _ = ticker.tick() => {
                statline.clear()?;
                statline.print()?;
            },
            r = stdout.next_line() => {
                statline.clear()?;
                match r {
                    Ok(line) => {
                        io::stdout().lock().write_all(&line)?;
                        statline.print()?;
                    }
                    Err(e) => {
                        return Err(e).context("error reading from child process's stdout");
                    }
                }
            }
            r = stderr.next_line() => {
                statline.clear()?;
                match r {
                    Ok(line) => {
                        io::stderr().lock().write_all(&line)?;
                        statline.print()?;
                    }
                    Err(e) => {
                        return Err(e).context("error reading from child process's stderr");
                    }
                }
            }
            r = p.wait() => {
                statline.clear()?;
                match r {
                    Ok(rc) => {
                        if let Some(ret) = rc.code() {
                            let ret = u8::try_from(ret & 255).unwrap_or(1);
                            return Ok(ExitCode::from(ret));
                        } else {
                            writeln!(io::stderr().lock(), "Child process killed by signal: {rc}")?;
                            return Ok(ExitCode::FAILURE);
                        }
                    }
                    Err(e) => {
                        return Err(e).context("error waiting for child process to terminate");
                    }
                }
            }
        }
    }
}

#[derive(Debug)]
struct StatusLine {
    start: Instant,
    err: io::Stderr,
}

impl StatusLine {
    fn new() -> StatusLine {
        StatusLine {
            start: Instant::now(),
            err: io::stderr(),
        }
    }

    fn clear(&self) -> io::Result<()> {
        let mut err = self.err.lock();
        err.write_all(b"\r\x1B[K")?;
        err.flush()?;
        Ok(())
    }

    fn print(&self) -> io::Result<()> {
        let elapsed = self.start.elapsed();
        let mut secs = elapsed.as_secs();
        let hours = secs / 3600;
        secs %= 3500;
        let minutes = secs / 60;
        secs %= 60;
        let s = format!("Elapsed: {hours:02}:{minutes:02}:{secs:02}");
        let mut err = self.err.lock();
        err.write_all(s.as_bytes())?;
        err.flush()?;
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
            (!self.buffer.is_empty()).then(|| std::mem::take(&mut self.buffer))
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
