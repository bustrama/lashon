//! Additive Discord Push-to-Mute, never a Toggle Mute or endpoint mute.
//! The user must reserve F24 for Discord's Push to Mute and verify it once.
use anyhow::{anyhow, Context, Result};
use std::io::{BufRead, BufReader};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

pub const HELPER_ARG: &str = "--discord-mute-guard";
static ACTIVE: AtomicBool = AtomicBool::new(false);

pub trait HeldKey {
    fn press(&mut self) -> Result<()>;
    fn release(&mut self) -> Result<()>;
}
pub struct Hold<K: HeldKey>(K);
impl<K: HeldKey> Hold<K> {
    pub fn acquire(mut key: K) -> Result<Self> {
        if let Err(err) = key.press() {
            let _ = key.release();
            return Err(err);
        }
        Ok(Self(key))
    }
}
impl<K: HeldKey> Drop for Hold<K> {
    fn drop(&mut self) {
        let _ = self.0.release();
    }
}

fn hold_until_release<K: HeldKey>(
    key: K,
    release: std::sync::mpsc::Receiver<()>,
    limit: Duration,
    ready: impl FnOnce() -> Result<()>,
) -> Result<()> {
    let _held = Hold::acquire(key)?;
    ready()?;
    let _ = release.recv_timeout(limit);
    Ok(())
}

pub struct Lease {
    child: Child,
    stdin: Option<ChildStdin>,
}
impl Lease {
    pub fn acquire() -> Result<Self> {
        if !cfg!(windows) {
            return Err(anyhow!("Discord Push to Mute is supported on Windows only"));
        }
        if ACTIVE.swap(true, Ordering::SeqCst) {
            return Err(anyhow!("Discord suppression is already in use"));
        }
        let result = Self::spawn();
        if result.is_err() {
            ACTIVE.store(false, Ordering::SeqCst);
        }
        result
    }
    fn spawn() -> Result<Self> {
        let mut command = Command::new(std::env::current_exe().context("locating Ottid")?);
        command
            .arg(HELPER_ARG)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null());
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            command.creation_flags(0x08000000); // CREATE_NO_WINDOW
        }
        let mut child = command.spawn().context("starting Discord release guard")?;
        let stdin = child.stdin.take();
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| anyhow!("guard stdout unavailable"))?;
        let mut lease = Self { child, stdin };
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let mut line = String::new();
            let success =
                BufReader::new(stdout).read_line(&mut line).is_ok() && line.trim() == "ready";
            let _ = tx.send(success);
        });
        if rx.recv_timeout(Duration::from_secs(3)).unwrap_or(false)
            && lease.child.try_wait()?.is_none()
        {
            Ok(lease)
        } else {
            Err(anyhow!("Discord release guard did not become ready"))
        }
    }
}
impl Drop for Lease {
    fn drop(&mut self) {
        self.stdin.take(); // EOF: helper releases F24, also when the parent aborts.
        let deadline = std::time::Instant::now() + Duration::from_secs(1);
        while self.child.try_wait().ok().flatten().is_none() && std::time::Instant::now() < deadline
        {
            std::thread::sleep(Duration::from_millis(10));
        }
        if self.child.try_wait().ok().flatten().is_none() {
            let _ = self.child.kill();
            let _ = self.child.wait();
        }
        // A fallback key-up only: it never toggles manual mute.
        #[cfg(windows)]
        {
            let _ = WindowsKey::new().and_then(|mut key| key.release());
        }
        ACTIVE.store(false, Ordering::SeqCst);
    }
}

#[cfg(windows)]
struct WindowsKey(enigo::Enigo);
#[cfg(windows)]
impl WindowsKey {
    fn new() -> Result<Self> {
        Ok(Self(enigo::Enigo::new(&enigo::Settings::default())?))
    }
}
#[cfg(windows)]
impl HeldKey for WindowsKey {
    fn press(&mut self) -> Result<()> {
        use enigo::Keyboard;
        self.0.key(enigo::Key::F24, enigo::Direction::Press)?;
        Ok(())
    }
    fn release(&mut self) -> Result<()> {
        use enigo::Keyboard;
        self.0.key(enigo::Key::F24, enigo::Direction::Release)?;
        Ok(())
    }
}

/// A minimal helper mode in the existing app executable, before Tauri starts.
pub fn helper_main() -> Result<()> {
    #[cfg(windows)]
    {
        use std::io::{Read, Write};
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let mut buffer = [0u8; 32];
            while std::io::stdin().read(&mut buffer).unwrap_or(0) > 0 {}
            let _ = tx.send(());
        });
        // Parent exit/abort closes its pipe; cap guards against a stuck parent.
        hold_until_release(WindowsKey::new()?, rx, Duration::from_secs(600), || {
            // Give Discord's global key handler time before capture starts.
            std::thread::sleep(Duration::from_millis(150));
            writeln!(std::io::stdout(), "ready")?;
            std::io::stdout().flush()?;
            Ok(())
        })
    }
    #[cfg(not(windows))]
    {
        Err(anyhow!("Windows only"))
    }
}

pub fn setup_key() -> Result<()> {
    if ACTIVE.load(Ordering::SeqCst) {
        return Err(anyhow!("Discord suppression is in use"));
    }
    #[cfg(windows)]
    {
        use enigo::Keyboard;
        let mut key = WindowsKey::new()?;
        key.0.key(enigo::Key::F24, enigo::Direction::Click)?;
        Ok(())
    }
    #[cfg(not(windows))]
    {
        Err(anyhow!("Windows only"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Mutex};
    struct Fake {
        events: Arc<Mutex<Vec<&'static str>>>,
        fails: bool,
    }
    impl HeldKey for Fake {
        fn press(&mut self) -> Result<()> {
            self.events.lock().unwrap().push("down");
            if self.fails {
                Err(anyhow!("failed"))
            } else {
                Ok(())
            }
        }
        fn release(&mut self) -> Result<()> {
            self.events.lock().unwrap().push("up");
            Ok(())
        }
    }
    #[test]
    fn watchdog_releases_on_parent_eof_timeout_and_failed_handshake() {
        for mode in 0..3 {
            let events = Arc::new(Mutex::new(Vec::new()));
            let (tx, rx) = std::sync::mpsc::channel();
            if mode == 0 {
                drop(tx);
            }
            let result = hold_until_release(
                Fake {
                    events: events.clone(),
                    fails: false,
                },
                rx,
                Duration::ZERO,
                || {
                    if mode == 2 {
                        Err(anyhow!("parent gone before ready"))
                    } else {
                        Ok(())
                    }
                },
            );
            assert_eq!(result.is_err(), mode == 2);
            assert_eq!(*events.lock().unwrap(), vec!["down", "up"]);
        }
    }
    #[test]
    fn releases_after_success_error_and_unwind_without_toggling() {
        for mode in 0..3 {
            let events = Arc::new(Mutex::new(Vec::new()));
            let copy = events.clone();
            let _ = std::panic::catch_unwind(move || {
                let held = Hold::acquire(Fake {
                    events: copy,
                    fails: mode == 1,
                });
                if mode == 2 {
                    let _held = held.unwrap();
                    panic!("cancelled");
                }
                drop(held);
            });
            assert_eq!(*events.lock().unwrap(), vec!["down", "up"]);
        }
    }
}
