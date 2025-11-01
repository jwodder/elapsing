#![cfg(unix)]
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
const LAX_SECOND: Duration = Duration::from_millis(1300);

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

    async fn wait_for_contents(
        &mut self,
        expected: &str,
        timeout: Duration,
    ) -> std::io::Result<()> {
        let deadline = Instant::now() + timeout;
        loop {
            let mut buf = vec![0u8; 2048];
            if let Ok(r) = timeout_at(deadline, self.pty.read(&mut buf)).await {
                #[allow(clippy::manual_assert)]
                if r? == 0 {
                    panic!(
                        "Reached EOF while waiting for screen contents {expected:?}; final content = {:?}",
                        self.contents()
                    );
                }
                self.parser.process(&buf);
                if self.contents() == expected {
                    return Ok(());
                }
            } else {
                panic!(
                    "Timed out while waiting for screen contents {expected:?}; final content = {:?}",
                    self.contents()
                );
            }
        }
    }

    async fn wait_for_exit(&mut self) -> std::io::Result<ExitStatus> {
        let mut buf = Vec::new();
        self.pty.read_to_end(&mut buf).await?;
        self.parser.process(&buf);
        self.p.wait().await
    }
}

#[tokio::test]
async fn sleepy() {
    let mut screen = TestScreen::spawn(
        pty_process::Command::new(env!("CARGO_BIN_EXE_elapsing"))
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
        .wait_for_contents("Starting...\nWorking...\nElapsed: 00:00:03", LAX_SECOND)
        .await
        .unwrap();
    screen
        .wait_for_contents("Starting...\nWorking...\nElapsed: 00:00:04", LAX_SECOND)
        .await
        .unwrap();
    screen
        .wait_for_contents(
            "Starting...\nWorking...\nShutting down...\nElapsed: 00:00:05",
            LAX_SECOND,
        )
        .await
        .unwrap();
    let r = screen.wait_for_exit().await.unwrap();
    assert!(r.success());
    assert_eq!(
        screen.contents(),
        "Starting...\nWorking...\nShutting down..."
    );
}
