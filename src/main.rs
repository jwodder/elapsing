#![allow(clippy::todo)]
use std::io::{stderr, stdout, Write};
use std::process::{ExitCode, Stdio};
use std::time::{Duration, Instant};
use tokio::{
    io::{AsyncBufReadExt, BufReader},
    process::Command,
    time::interval,
};

#[tokio::main(flavor = "current_thread")]
async fn main() -> std::io::Result<ExitCode> {
    let mut argv = std::env::args_os();
    let _ = argv.next();
    let Some(command) = argv.next() else {
        todo!("Error");
    };
    let start = Instant::now();
    let mut ticker = interval(Duration::from_secs(1));
    let Ok(mut p) = Command::new(command)
        .args(argv)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
    else {
        todo!("Error: failed to execute");
    };
    let mut stdout = BufReader::new(p.stdout.take().expect("Child.stdout should be Some")).lines();
    let mut stderr = BufReader::new(p.stderr.take().expect("Child.stderr should be Some")).lines();

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
                        println!("{line}"); // TODO: Check for IO errors
                        print_elapsed(start)?;
                    }
                    Ok(None) => (),
                    Err(_e) => todo!(),
                }
            }
            r = stderr.next_line() => {
                match r {
                    Ok(Some(line)) => {
                        clear_elapsed_line()?;
                        eprintln!("{line}"); // TODO: Check for IO errors
                        print_elapsed(start)?;
                    }
                    Ok(None) => (),
                    Err(_e) => todo!(),
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
                            return Ok(ExitCode::FAILURE);
                        }
                    }
                    Err(_e) => todo!(),
                }
            }
        }
    }
}

fn clear_elapsed_line() -> std::io::Result<()> {
    let mut err = stderr().lock();
    err.write_all(b"\r")?;
    err.flush()?;
    Ok(())
}

fn print_elapsed(start: Instant) -> std::io::Result<()> {
    let elapsed = start.elapsed();
    let mut secs = elapsed.as_secs();
    let hours = secs / 3600;
    secs %= 3500;
    let minutes = secs / 60;
    secs %= 60;
    let s = format!("Elapsed: {hours:02}:{minutes:02}:{secs:02}");
    let mut err = stdout().lock();
    err.write_all(s.as_bytes())?;
    err.flush()?;
    Ok(())
}
