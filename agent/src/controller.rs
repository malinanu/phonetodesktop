//! Backend-independent policy: pick the target player, and make seeks reliable by
//! verifying the effect and escalating to keyboard fallback when a session lies.

use crate::backend::{Backend, Key, Transport};
use crate::protocol::{Command, PlayerInfo, State};
const STATE_TTL: Duration = Duration::from_millis(750);
use anyhow::{bail, Result};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

const BROKEN_TTL: Duration = Duration::from_secs(600);

pub struct Controller {
    backend: Arc<dyn Backend>,
    host: String,
    selected: Mutex<Option<String>>,
    /// Apps whose session seek returned true but did not move the position; retried after
    /// BROKEN_TTL so one transient miss (track change, buffering) is not permanent.
    seek_broken: Mutex<HashMap<String, Instant>>,
    verify_delay: Duration,
    /// Short-lived copy of the last `state()`, so the dashboard poll, phone poll and debug views
    /// share one expensive OS query instead of each making their own.
    cached: Mutex<Option<(Instant, State)>>,
}

impl Controller {
    pub fn new(backend: Arc<dyn Backend>, host: String) -> Self {
        Controller {
            backend,
            host,
            selected: Mutex::new(None),
            seek_broken: Mutex::new(HashMap::new()),
            verify_delay: Duration::from_millis(350),
            cached: Mutex::new(None),
        }
    }

    #[cfg(test)]
    fn with_verify_delay(mut self, d: Duration) -> Self {
        self.verify_delay = d;
        self
    }

    /// The selected player if it still exists, else the first playing one, else the first.
    fn pick<'a>(&self, players: &'a [PlayerInfo]) -> Option<&'a PlayerInfo> {
        let sel = self.selected.lock().unwrap().clone();
        sel.and_then(|s| players.iter().find(|p| p.id == s))
            .or_else(|| players.iter().find(|p| p.playing))
            .or_else(|| players.first())
    }

    pub fn debug(&self) -> String {
        self.backend.debug()
    }

    pub fn state(&self) -> Result<State> {
        if let Some((at, st)) = self.cached.lock().unwrap().as_ref() {
            if at.elapsed() < STATE_TTL {
                return Ok(st.clone());
            }
        }
        let st = self.fresh_state()?;
        *self.cached.lock().unwrap() = Some((Instant::now(), st.clone()));
        Ok(st)
    }

    fn fresh_state(&self) -> Result<State> {
        let players = self.backend.snapshot()?;
        let current = self.pick(&players).map(|p| p.id.clone());
        Ok(State {
            t: "state",
            host: self.host.clone(),
            backend: self.backend.name(),
            version: env!("CARGO_PKG_VERSION"),
            current,
            players,
        })
    }

    pub fn execute(&self, cmd: Command) -> Result<()> {
        // Whatever this command does, the next state must be read fresh.
        *self.cached.lock().unwrap() = None;
        match cmd {
            Command::Select { id } => {
                *self.selected.lock().unwrap() = Some(id);
                Ok(())
            }
            Command::Volume { d } => {
                let key = if d >= 0 { Key::VolUp } else { Key::VolDown };
                for _ in 0..d.unsigned_abs().min(25) {
                    self.backend.media_key(key)?;
                }
                Ok(())
            }
            Command::Mute => self.backend.media_key(Key::Mute),
            Command::PlayPause => self.transport(Transport::PlayPause, Key::PlayPause),
            Command::Next => self.transport(Transport::Next, Key::Next),
            Command::Prev => self.transport(Transport::Prev, Key::Prev),
            Command::SeekRel { d } => self.seek_rel(d.clamp(-600, 600)),
            Command::SeekAbs { pos_ms } => {
                let players = self.backend.snapshot()?;
                let Some(p) = self.pick(&players) else { bail!("no player") };
                if !p.can_seek {
                    bail!("{} does not support seeking", p.app);
                }
                self.backend.seek_abs(&p.id, pos_ms.clamp(0, p.dur_ms.max(0)))?;
                Ok(())
            }
        }
    }

    /// Session command first; synthetic media key if there is no session or it refuses.
    fn transport(&self, what: Transport, key: Key) -> Result<()> {
        let players = self.backend.snapshot()?;
        if let Some(p) = self.pick(&players) {
            if self.backend.transport(&p.id, what)? {
                return Ok(());
            }
        }
        self.backend.media_key(key)
    }

    fn seek_rel(&self, delta_s: i64) -> Result<()> {
        let players = self.backend.snapshot()?;
        let Some(p) = self.pick(&players) else {
            return self.keys_or_err(delta_s);
        };
        let broken = self
            .seek_broken
            .lock()
            .unwrap()
            .get(&p.app)
            .is_some_and(|t| t.elapsed() < BROKEN_TTL);
        if p.can_seek && !broken {
            let dur = p.dur_ms.max(0);
            let target = (p.pos_ms + delta_s * 1000).clamp(0, dur);
            let t0 = Instant::now();
            self.backend.seek_abs(&p.id, target)?;
            std::thread::sleep(self.verify_delay);
            // Verify: the position must now be near the target (allowing for playback drift).
            let after = self.backend.snapshot()?;
            if let Some(q) = after.iter().find(|q| q.id == p.id) {
                let drift = if q.playing { t0.elapsed().as_millis() as i64 } else { 0 };
                let expected = (target + drift).min(dur);
                let moved = (q.pos_ms - p.pos_ms).abs() as f64;
                let wanted = (target - p.pos_ms).abs() as f64;
                if (q.pos_ms - expected).abs() <= 2_000 || (wanted > 0.0 && moved >= wanted * 0.5) {
                    return Ok(());
                }
                // Clamped at the start/end is fine.
                if wanted == 0.0 {
                    return Ok(());
                }
            }
            self.seek_broken.lock().unwrap().insert(p.app.clone(), Instant::now());
        }
        self.keys_or_err(delta_s)
    }

    fn keys_or_err(&self, delta_s: i64) -> Result<()> {
        if self.backend.focused_seek(delta_s)? == 0 {
            bail!("seek not possible: player exposes no seekable session and is not focused");
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::backend::mock::MockBackend;

    fn ctl(lying: bool) -> (Controller, Arc<MockBackend>) {
        let b = Arc::new(MockBackend::with_lying_seek(lying));
        let c = Controller::new(b.clone(), "test".into()).with_verify_delay(Duration::from_millis(20));
        (c, b)
    }

    #[test]
    fn seek_forward_uses_session() {
        let (c, b) = ctl(false);
        c.execute(Command::SeekRel { d: 10 }).unwrap();
        let s = c.state().unwrap();
        assert!(s.players[0].pos_ms >= 70_000 && s.players[0].pos_ms < 71_000);
        assert!(!b.log().iter().any(|l| l.starts_with("focused_seek")));
    }

    #[test]
    fn seek_clamps_at_start() {
        let (c, _) = ctl(false);
        c.execute(Command::SeekRel { d: -300 }).unwrap();
        assert!(c.state().unwrap().players[0].pos_ms < 1_000);
    }

    #[test]
    fn lying_session_escalates_to_keys_and_is_remembered() {
        let (c, b) = ctl(true);
        c.execute(Command::SeekRel { d: 10 }).unwrap();
        assert!(b.log().contains(&"focused_seek 10".to_string()));
        // Second time: skip the broken session path entirely.
        c.execute(Command::SeekRel { d: 10 }).unwrap();
        let seeks = b.log().iter().filter(|l| l.starts_with("seek_abs")).count();
        assert_eq!(seeks, 1);
    }

    #[test]
    fn play_pause_toggles() {
        let (c, _) = ctl(false);
        assert!(c.state().unwrap().players[0].playing);
        c.execute(Command::PlayPause).unwrap();
        assert!(!c.state().unwrap().players[0].playing);
    }

    #[test]
    fn volume_sends_media_keys() {
        let (c, b) = ctl(false);
        c.execute(Command::Volume { d: -2 }).unwrap();
        assert_eq!(b.log().iter().filter(|l| *l == "key VolDown").count(), 2);
    }
}
