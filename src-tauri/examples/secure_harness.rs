//! SPEC-29 device harness: runs the real `run_poll` against a phone over plain
//! adb, without the app. Prints one line per event and per tick (no pixels,
//! only window metadata). Used for the FPS acceptance and the device tests.
//!
//! usage: secure_harness <adb.exe> <serial> <mirror|flex> <seconds> [scrcpy_pid] [virtual_display] [screen_off 0|1]

use anyleap_lib::secure_watch::*;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;
use std::time::{Duration, Instant};

struct Env {
    adb: String,
    serial: String,
    pid: Option<u32>,
    vd: Option<u32>,
    wake: Mutex<SessionWake>,
    t0: Instant,
    calls: AtomicU64,
}

impl Env {
    fn say(&self, m: &str) {
        println!("[{:>7.2}s] {m}", self.t0.elapsed().as_secs_f64());
    }
}

impl PollEnv for Env {
    fn view(&self) -> Option<SessionView> {
        Some(SessionView {
            wake: self.wake.lock().unwrap().clone(),
            hidden: false,
            virtual_display: self.vd,
        })
    }
    fn mutate(&self, f: &mut dyn FnMut(&mut SessionWake)) {
        f(&mut self.wake.lock().unwrap());
    }
    async fn adb_shell(&self, script: &str) -> Result<String, String> {
        self.calls.fetch_add(1, Ordering::Relaxed);
        let (adb, serial, script) = (self.adb.clone(), self.serial.clone(), script.to_string());
        let out = tokio::task::spawn_blocking(move || {
            std::process::Command::new(adb)
                .args(["-s", &serial, "shell", &script])
                .output()
        })
        .await
        .map_err(|e| e.to_string())?
        .map_err(|e| e.to_string())?;
        Ok(String::from_utf8_lossy(&out.stdout).to_string())
    }
    async fn screen_power(&self, off: bool, awake: Option<bool>) -> Result<(), String> {
        self.say(&format!("ACTION screen_power off={off} awake={awake:?}"));
        match self.pid {
            Some(pid) if inject_screen_power(&self.serial, pid, off) => Ok(()),
            _ if off => Err("no scrcpy window".into()),
            _ => {
                std::process::Command::new(&self.adb)
                    .args([
                        "-s",
                        &self.serial,
                        "shell",
                        "input",
                        "keyevent",
                        "KEYCODE_WAKEUP",
                    ])
                    .output()
                    .map_err(|e| e.to_string())?;
                Ok(())
            }
        }
    }
    fn emit_state(&self, s: &SecureState) {
        self.say(&format!("STATE {s:?}"));
    }
    fn emit_notice(&self, text: &str) {
        self.say(&format!("NOTICE {text}"));
    }
    fn emit_screen_power(&self, off: bool) {
        self.say(&format!("EVENT screen-power off={off}"));
    }
    fn log(&self, msg: &str) {
        self.say(msg);
    }
    fn tick_stats(&self, ewma: Option<f64>, interval: Duration) {
        self.say(&format!(
            "TICK ewma_ms={:?} interval_ms={} adb_calls={}",
            ewma.map(|e| e.round()),
            interval.as_millis(),
            self.calls.load(Ordering::Relaxed)
        ));
    }
}

#[tokio::main(flavor = "current_thread")]
async fn main() {
    let a: Vec<String> = std::env::args().collect();
    let kind = if a[3] == "flex" {
        PollKind::Flex
    } else {
        PollKind::Screen
    };
    let secs: u64 = a[4].parse().unwrap();
    let env = Env {
        adb: a[1].clone(),
        serial: a[2].clone(),
        pid: a.get(5).and_then(|s| s.parse().ok()).filter(|p| *p != 0),
        vd: a.get(6).and_then(|s| s.parse().ok()).filter(|p| *p != 0),
        wake: Mutex::new(SessionWake::new(
            a.get(7).map(|s| s == "1").unwrap_or(true),
            true,
            true,
        )),
        t0: Instant::now(),
        calls: AtomicU64::new(0),
    };
    let _ = tokio::time::timeout(
        Duration::from_secs(secs),
        run_poll(&env, kind, Tuning::default()),
    )
    .await;
    env.say(&format!(
        "END adb_calls={}",
        env.calls.load(Ordering::Relaxed)
    ));
}
