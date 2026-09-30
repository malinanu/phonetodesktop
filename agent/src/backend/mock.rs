//! Simulated player for development and tests on machines without a supported OS layer.

use super::{Backend, Input, Key, Transport};
use crate::protocol::PlayerInfo;
use anyhow::Result;
use std::sync::Mutex;
use std::time::Instant;

struct Inner {
    playing: bool,
    pos_ms: i64,
    since: Instant,
    /// When set, seek_abs returns true but does nothing (like Spotify's SMTC session).
    lying_seek: bool,
    volume: u8,
    muted: bool,
    log: Vec<String>,
}

pub struct MockBackend {
    inner: Mutex<Inner>,
}

impl MockBackend {
    pub fn new() -> Self {
        Self::with_lying_seek(false)
    }

    pub fn with_lying_seek(lying: bool) -> Self {
        MockBackend {
            inner: Mutex::new(Inner {
                playing: true,
                pos_ms: 60_000,
                since: Instant::now(),
                lying_seek: lying,
                volume: 50,
                muted: false,
                log: vec![],
            }),
        }
    }

    #[cfg(test)]
    pub fn log(&self) -> Vec<String> {
        self.inner.lock().unwrap().log.clone()
    }
}

const DUR_MS: i64 = 213_000;

impl Inner {
    fn pos(&self) -> i64 {
        let p = if self.playing {
            self.pos_ms + self.since.elapsed().as_millis() as i64
        } else {
            self.pos_ms
        };
        p.clamp(0, DUR_MS)
    }
    fn set_pos(&mut self, p: i64) {
        self.pos_ms = p.clamp(0, DUR_MS);
        self.since = Instant::now();
    }
}

impl Backend for MockBackend {
    fn name(&self) -> &'static str {
        "mock"
    }

    fn snapshot(&self) -> Result<Vec<PlayerInfo>> {
        let s = self.inner.lock().unwrap();
        Ok(vec![PlayerInfo {
            id: "mock.player".into(),
            app: "Mock Player".into(),
            title: "Demo Track".into(),
            artist: "Phone Remote".into(),
            playing: s.playing,
            pos_ms: s.pos(),
            dur_ms: DUR_MS,
            can_seek: true,
        }])
    }

    fn transport(&self, _id: &str, what: Transport) -> Result<bool> {
        let mut s = self.inner.lock().unwrap();
        s.log.push(format!("{what:?}"));
        match what {
            Transport::PlayPause => {
                let p = s.pos();
                s.playing = !s.playing;
                s.set_pos(p);
            }
            Transport::Next | Transport::Prev => s.set_pos(0),
        }
        Ok(true)
    }

    fn seek_abs(&self, _id: &str, pos_ms: i64) -> Result<bool> {
        let mut s = self.inner.lock().unwrap();
        s.log.push(format!("seek_abs {pos_ms}"));
        if !s.lying_seek {
            s.set_pos(pos_ms);
        }
        Ok(true)
    }

    fn media_key(&self, key: Key) -> Result<()> {
        self.inner.lock().unwrap().log.push(format!("key {key:?}"));
        Ok(())
    }

    fn volume(&self) -> Option<(u8, bool)> {
        let s = self.inner.lock().unwrap();
        Some((s.volume, s.muted))
    }

    fn set_volume(&self, level: u8) -> Result<()> {
        let mut s = self.inner.lock().unwrap();
        s.log.push(format!("volume {level}"));
        s.volume = level;
        s.muted = false;
        Ok(())
    }

    fn set_mute(&self, muted: bool) -> Result<()> {
        let mut s = self.inner.lock().unwrap();
        s.log.push(format!("mute {muted}"));
        s.muted = muted;
        Ok(())
    }

    fn input(&self, input: Input) -> Result<()> {
        self.inner.lock().unwrap().log.push(format!("input {input:?}"));
        Ok(())
    }

    fn focused_seek(&self, secs: i64) -> Result<i64> {
        let mut s = self.inner.lock().unwrap();
        s.log.push(format!("focused_seek {secs}"));
        let p = s.pos();
        s.set_pos(p + secs * 1000);
        Ok(secs)
    }
}
