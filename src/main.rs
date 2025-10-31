use anyhow::Context as _;
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

#[tokio::main(flavor = "current_thread")]
async fn main() -> anyhow::Result<ExitCode> {
    let mut argv = std::env::args_os();
    let _ = argv.next();
    let Some(command) = argv.next() else {
        writeln!(io::stderr().lock(), "Usage: elapsing command [args ...]")?;
        return Ok(ExitCode::FAILURE);
    };
    let start = Instant::now();
    let mut ticker = interval(Duration::from_secs(1));
    let mut p = Command::new(command)
        .args(argv)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        .spawn()
        .context("failed to start command")?;
    let mut stdout = ByteLines::new(p.stdout.take().expect("Child.stdout should be Some"));
    let mut stderr = ByteLines::new(p.stderr.take().expect("Child.stderr should be Some"));
    print_elapsed(start)?;
    loop {
        tokio::select! {
            _ = ticker.tick() => {
                clear_elapsed_line()?;
                print_elapsed(start)?;
            },
            r = stdout.next_line() => {
                match r {
                    Ok(line) => {
                        clear_elapsed_line()?;
                        io::stdout().lock().write_all(&line)?;
                        print_elapsed(start)?;
                    }
                    Err(e) => {
                        clear_elapsed_line()?;
                        return Err(e).context("error reading from process's stdout");
                    }
                }
            }
            r = stderr.next_line() => {
                match r {
                    Ok(line) => {
                        clear_elapsed_line()?;
                        io::stderr().lock().write_all(&line)?;
                        print_elapsed(start)?;
                    }
                    Err(e) => {
                        clear_elapsed_line()?;
                        return Err(e).context("error reading from process's stderr");
                    }
                }
            }
            r = p.wait() => {
                clear_elapsed_line()?;
                match r {
                    Ok(rc) => {
                        if let Some(ret) = rc.code() {
                            let ret = u8::try_from(ret & 255).unwrap_or(1);
                            return Ok(ExitCode::from(ret));
                        } else {
                            writeln!(io::stderr().lock(), "Process killed by signal: {rc}")?;
                            return Ok(ExitCode::FAILURE);
                        }
                    }
                    Err(e) => {
                        clear_elapsed_line()?;
                        return Err(e).context("error waiting for process to terminate");
                    }
                }
            }
        }
    }
}

fn clear_elapsed_line() -> io::Result<()> {
    let mut err = io::stderr().lock();
    err.write_all(b"\r\x1B[K")?;
    err.flush()?;
    Ok(())
}

fn print_elapsed(start: Instant) -> io::Result<()> {
    let elapsed = start.elapsed();
    let mut secs = elapsed.as_secs();
    let hours = secs / 3600;
    secs %= 3500;
    let minutes = secs / 60;
    secs %= 60;
    let s = format!("Elapsed: {hours:02}:{minutes:02}:{secs:02}");
    let mut err = io::stdout().lock();
    err.write_all(s.as_bytes())?;
    err.flush()?;
    Ok(())
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
