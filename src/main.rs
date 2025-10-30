use anyhow::Context;
use std::io::{self, Write};
use std::process::{ExitCode, Stdio};
use std::time::{Duration, Instant};
use tokio::{
    io::{AsyncBufReadExt, BufReader},
    process::Command,
    time::interval,
};

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
        .spawn()
        .context("failed to start command")?;
    let mut stdout = BufReader::new(p.stdout.take().expect("Child.stdout should be Some")).lines();
    let mut stderr = BufReader::new(p.stderr.take().expect("Child.stderr should be Some")).lines();
    print_elapsed(start)?;
    loop {
        tokio::select! {
            _ = ticker.tick() => {
                clear_elapsed_line()?;
                print_elapsed(start)?;
            },
            r = stdout.next_line() => {
                match r {
                    Ok(Some(line)) => {
                        clear_elapsed_line()?;
                        writeln!(io::stdout().lock(), "{line}")?;
                        print_elapsed(start)?;
                    }
                    Ok(None) => (),
                    Err(e) => {
                        clear_elapsed_line()?;
                        return Err(e).context("error reading from process's stdout");
                    }
                }
            }
            r = stderr.next_line() => {
                match r {
                    Ok(Some(line)) => {
                        clear_elapsed_line()?;
                        writeln!(io::stderr().lock(), "{line}")?;
                        print_elapsed(start)?;
                    }
                    Ok(None) => (),
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
