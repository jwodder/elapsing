#![cfg(unix)]
use std::io::{Seek, Write};
use std::process::ExitStatus;
use std::time::Duration;
use tokio::{
    io::AsyncReadExt,
    time::{Instant, timeout_at},
};

static SCRIPTS_DIR: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/scripts");

const SCREEN_WIDTH: u16 = 24;
const SCREEN_HEIGHT: u16 = 80;

const STARTUP_WAIT: Duration = Duration::from_millis(100);
const LAX_SECOND: Duration = Duration::from_millis(1500);

struct TestScreen {
    parser: vt100::Parser,
    p: tokio::process::Child,
    pty: pty_process::Pty,
}

impl TestScreen {
    fn spawn(cmd: pty_process::Command) -> Result<TestScreen, pty_process::Error> {
        let (pty, pts) = pty_process::open()?;
        pty.resize(pty_process::Size::new(SCREEN_WIDTH, SCREEN_HEIGHT))?;
        let p = cmd.spawn(pts)?;
        let parser = vt100::Parser::new(SCREEN_WIDTH, SCREEN_HEIGHT, 0);
        Ok(TestScreen { pty, p, parser })
    }

    fn contents(&self) -> String {
        self.parser.screen().contents()
    }

    async fn read(&mut self) -> std::io::Result<Option<Vec<u8>>> {
        let mut buf = vec![0u8; 2048];
        match self.pty.read(&mut buf).await {
            Ok(0) => Ok(None),
            #[cfg(target_os = "linux")]
            Err(e) if e.raw_os_error() == Some(5) => {
                // On Linux, attempting to read from a pty master after the
                // slave closes (due, e.g., to the child process exiting)
                // results in EIO (which Rust currently represents with the
                // undocumented ErrorKind::Uncategorized).
                Ok(None)
            }
            Ok(n) => {
                buf.truncate(n);
                Ok(Some(buf))
            }
            Err(e) => Err(e),
        }
    }

    #[allow(clippy::match_wild_err_arm)]
    async fn wait_for_contents(
        &mut self,
        expected: &str,
        timeout: Duration,
    ) -> std::io::Result<()> {
        let deadline = Instant::now() + timeout;
        loop {
            match timeout_at(deadline, self.read()).await {
                Ok(Ok(Some(buf))) => {
                    self.parser.process(&buf);
                    if self.contents() == expected {
                        return Ok(());
                    }
                }
                Ok(Ok(None)) => {
                    panic!(
                        "Reached EOF while waiting for screen contents {expected:?}; final content = {:?}",
                        self.contents()
                    );
                }
                Ok(Err(e)) => return Err(e),
                Err(_) => {
                    panic!(
                        "Timed out while waiting for screen contents {expected:?}; final content = {:?}",
                        self.contents()
                    );
                }
            }
        }
    }

    async fn wait_for_exit(&mut self) -> std::io::Result<ExitStatus> {
        while let Some(buf) = self.read().await? {
            self.parser.process(&buf);
        }
        self.p.wait().await
    }
}

#[tokio::test]
async fn sleepy() {
    let mut screen = TestScreen::spawn(
        pty_process::Command::new(env!("CARGO_BIN_EXE_elapsed"))
            .arg("python3")
            .arg(format!("{SCRIPTS_DIR}/sleepy.py")),
    )
    .unwrap();
    screen
        .wait_for_contents("Elapsed: 00:00:00", STARTUP_WAIT)
        .await
        .unwrap();
    screen
        .wait_for_contents("Starting...\nElapsed: 00:00:01", LAX_SECOND)
        .await
        .unwrap();
    screen
        .wait_for_contents("Starting...\nElapsed: 00:00:02", LAX_SECOND)
        .await
        .unwrap();
    screen
        .wait_for_contents(
            "Starting...\nWorking...\nStdout is not a tty\nElapsed: 00:00:03",
            LAX_SECOND,
        )
        .await
        .unwrap();
    screen
        .wait_for_contents(
            "Starting...\nWorking...\nStdout is not a tty\nElapsed: 00:00:04",
            LAX_SECOND,
        )
        .await
        .unwrap();
    screen
        .wait_for_contents(
            "Starting...\nWorking...\nStdout is not a tty\nShutting down...\nElapsed: 00:00:05",
            LAX_SECOND,
        )
        .await
        .unwrap();
    let r = screen.wait_for_exit().await.unwrap();
    assert!(r.success());
    assert_eq!(
        screen.contents(),
        "Starting...\nWorking...\nStdout is not a tty\nShutting down..."
    );
}

#[tokio::test]
async fn sleepy_total() {
    let mut screen = TestScreen::spawn(
        pty_process::Command::new(env!("CARGO_BIN_EXE_elapsed"))
            .arg("--total")
            .arg("python3")
            .arg(format!("{SCRIPTS_DIR}/sleepy.py")),
    )
    .unwrap();
    screen
        .wait_for_contents("Elapsed: 00:00:00", STARTUP_WAIT)
        .await
        .unwrap();
    screen
        .wait_for_contents("Starting...\nElapsed: 00:00:01", LAX_SECOND)
        .await
        .unwrap();
    screen
        .wait_for_contents("Starting...\nElapsed: 00:00:02", LAX_SECOND)
        .await
        .unwrap();
    screen
        .wait_for_contents(
            "Starting...\nWorking...\nStdout is not a tty\nElapsed: 00:00:03",
            LAX_SECOND,
        )
        .await
        .unwrap();
    screen
        .wait_for_contents(
            "Starting...\nWorking...\nStdout is not a tty\nElapsed: 00:00:04",
            LAX_SECOND,
        )
        .await
        .unwrap();
    screen
        .wait_for_contents(
            "Starting...\nWorking...\nStdout is not a tty\nShutting down...\nElapsed: 00:00:05",
            LAX_SECOND,
        )
        .await
        .unwrap();
    let r = screen.wait_for_exit().await.unwrap();
    assert!(r.success());
    assert_eq!(
        screen.contents(),
        "Starting...\nWorking...\nStdout is not a tty\nShutting down...\nElapsed: 00:00:06"
    );
}

#[tokio::test]
async fn read_stdin() {
    let mut infile = tempfile::tempfile().unwrap();
    infile.write_all(b"Apple\nBanana\nCoconut\n").unwrap();
    infile.flush().unwrap();
    infile.rewind().unwrap();
    let mut screen = TestScreen::spawn(
        pty_process::Command::new(env!("CARGO_BIN_EXE_elapsed"))
            .arg("python3")
            .arg(format!("{SCRIPTS_DIR}/read-stdin.py"))
            .stdin(infile),
    )
    .unwrap();
    screen
        .wait_for_contents("Line 1: Apple\nElapsed: 00:00:01", LAX_SECOND)
        .await
        .unwrap();

    screen
        .wait_for_contents(
            "Line 1: Apple\nLine 2: Banana\nElapsed: 00:00:02",
            LAX_SECOND,
        )
        .await
        .unwrap();
    screen
        .wait_for_contents(
            "Line 1: Apple\nLine 2: Banana\nLine 3: Coconut\nElapsed: 00:00:03",
            LAX_SECOND,
        )
        .await
        .unwrap();
    let r = screen.wait_for_exit().await.unwrap();
    assert!(r.success());
    assert_eq!(
        screen.contents(),
        "Line 1: Apple\nLine 2: Banana\nLine 3: Coconut",
    );
}
