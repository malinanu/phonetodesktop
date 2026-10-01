//! macOS and Linux backend.
//!
//! - Media keys, mouse, keyboard and text: `enigo` (X11 on Linux, CGEvent on macOS).
//! - Players: Linux reads every MPRIS session over D-Bus (title, progress, exact seek). macOS has no public
//!   now-playing API, so it reports no sessions and the controller falls back to media keys (play/pause, next,
//!   previous, volume) without a title or progress bar.
//! - Volume: `wpctl`/`pactl` on Linux, `osascript` on macOS.

use super::{keymap, sysvol, Backend, ButtonAction, Input, Key, MouseButton, Transport};
use crate::protocol::PlayerInfo;
use anyhow::{anyhow, Result};
use enigo::{Axis, Button, Coordinate, Direction, Enigo, Keyboard, Mouse, Settings};
use std::sync::Mutex;

/// Wheel units from the phone: 120 = one notch (same as Windows).
const NOTCH: i32 = 120;

pub struct DesktopBackend {
    /// Created on first use and dropped after an error, so a lost display or a revoked permission recovers.
    enigo: Mutex<Option<Enigo>>,
    /// Scroll amounts smaller than one notch are carried over instead of being lost.
    scroll_rem: Mutex<(i32, i32)>,
    last_error: Mutex<String>,
    #[cfg(target_os = "linux")]
    mpris: super::mpris::Mpris,
}

impl DesktopBackend {
    pub fn new() -> Self {
        DesktopBackend {
            enigo: Mutex::new(None),
            scroll_rem: Mutex::new((0, 0)),
            last_error: Mutex::new(String::new()),
            #[cfg(target_os = "linux")]
            mpris: super::mpris::Mpris::new(),
        }
    }

    fn with_enigo<R>(&self, f: impl FnOnce(&mut Enigo) -> enigo::InputResult<R>) -> Result<R> {
        let mut g = self.enigo.lock().unwrap();
        if g.is_none() {
            *g = Some(Enigo::new(&Settings::default()).map_err(|e| anyhow!("cannot send keyboard/mouse input: {e}{}", hint()))?);
        }
        match f(g.as_mut().unwrap()) {
            Ok(r) => Ok(r),
            Err(e) => {
                *g = None;
                Err(anyhow!("input failed: {e}{}", hint()))
            }
        }
    }

    #[cfg_attr(not(target_os = "linux"), allow(dead_code))]
    fn note_error(&self, e: &anyhow::Error) {
        *self.last_error.lock().unwrap() = format!("{e:#}");
    }
}

#[cfg(target_os = "macos")]
fn hint() -> &'static str {
    " (allow Phone Remote under System Settings > Privacy & Security > Accessibility)"
}

#[cfg(not(target_os = "macos"))]
fn hint() -> &'static str {
    " (needs an X11 session or XWayland; Wayland-only desktops may block it)"
}

/// How many whole notches a wheel amount makes, keeping the remainder for next time.
fn take_notches(rem: &mut i32, add: i32) -> i32 {
    *rem += add;
    let n = *rem / NOTCH;
    *rem -= n * NOTCH;
    n
}

fn enigo_media_key(key: Key) -> enigo::Key {
    match key {
        Key::PlayPause => enigo::Key::MediaPlayPause,
        Key::Next => enigo::Key::MediaNextTrack,
        Key::Prev => enigo::Key::MediaPrevTrack,
        Key::VolUp => enigo::Key::VolumeUp,
        Key::VolDown => enigo::Key::VolumeDown,
        Key::Mute => enigo::Key::VolumeMute,
    }
}

impl Backend for DesktopBackend {
    fn name(&self) -> &'static str {
        if cfg!(target_os = "macos") {
            "macos"
        } else {
            "linux-mpris"
        }
    }

    fn snapshot(&self) -> Result<Vec<PlayerInfo>> {
        #[cfg(target_os = "linux")]
        {
            return match self.mpris.snapshot() {
                Ok(p) => Ok(p),
                // No bus (headless, or started before the session): behave like "nothing playing"; media keys still work.
                Err(e) => {
                    self.note_error(&e);
                    Ok(vec![])
                }
            };
        }
        #[allow(unreachable_code)]
        Ok(vec![])
    }

    fn transport(&self, id: &str, what: Transport) -> Result<bool> {
        #[cfg(target_os = "linux")]
        {
            let method = match what {
                Transport::PlayPause => "PlayPause",
                Transport::Next => "Next",
                Transport::Prev => "Previous",
            };
            return self.mpris.call(id, method);
        }
        #[allow(unreachable_code)]
        {
            let _ = id;
            self.media_key(match what {
                Transport::PlayPause => Key::PlayPause,
                Transport::Next => Key::Next,
                Transport::Prev => Key::Prev,
            })?;
            Ok(true)
        }
    }

    fn seek_abs(&self, id: &str, pos_ms: i64) -> Result<bool> {
        #[cfg(target_os = "linux")]
        return self.mpris.seek_abs(id, pos_ms);
        #[allow(unreachable_code)]
        {
            let _ = (id, pos_ms);
            Ok(false)
        }
    }

    fn media_key(&self, key: Key) -> Result<()> {
        self.with_enigo(|e| e.key(enigo_media_key(key), Direction::Click))
    }

    /// Without a way to tell which window is a player, arrow keys would go to whatever has focus: send nothing.
    fn focused_seek(&self, _secs: i64) -> Result<i64> {
        Ok(0)
    }

    fn volume(&self) -> Option<(u8, bool)> {
        sysvol::get()
    }

    fn set_volume(&self, level: u8) -> Result<()> {
        sysvol::set_level(level)
    }

    fn set_mute(&self, muted: bool) -> Result<()> {
        sysvol::set_mute(muted)
    }

    fn input(&self, input: Input) -> Result<()> {
        match input {
            Input::Move(dx, dy) => self.with_enigo(|e| e.move_mouse(dx, dy, Coordinate::Rel)),
            Input::Button(b, action) => {
                let button = match b {
                    MouseButton::Left => Button::Left,
                    MouseButton::Right => Button::Right,
                    MouseButton::Middle => Button::Middle,
                };
                let dir = match action {
                    ButtonAction::Click => Direction::Click,
                    ButtonAction::Down => Direction::Press,
                    ButtonAction::Up => Direction::Release,
                };
                self.with_enigo(|e| e.button(button, dir))
            }
            Input::Scroll(dx, dy) => {
                let (sx, sy) = {
                    let mut r = self.scroll_rem.lock().unwrap();
                    (take_notches(&mut r.0, dx), take_notches(&mut r.1, dy))
                };
                // Wheel units here are Windows-style (positive = up/right); enigo's vertical axis is positive = down.
                self.with_enigo(|e| {
                    if sy != 0 {
                        e.scroll(-sy, Axis::Vertical)?;
                    }
                    if sx != 0 {
                        e.scroll(sx, Axis::Horizontal)?;
                    }
                    Ok(())
                })
            }
            Input::Text(text) => self.with_enigo(|e| e.text(&text)),
            Input::Key { name, mods } => {
                let key = keymap::named_key(&name).ok_or_else(|| anyhow!("unknown key {name:?}"))?;
                let mods: Vec<enigo::Key> = mods.iter().filter_map(|m| keymap::modifier(m)).collect();
                self.with_enigo(|e| {
                    let mut pressed = vec![];
                    let mut result = Ok(());
                    for m in &mods {
                        match e.key(*m, Direction::Press) {
                            Ok(()) => pressed.push(*m),
                            Err(err) => {
                                result = Err(err);
                                break;
                            }
                        }
                    }
                    if result.is_ok() {
                        result = e.key(key, Direction::Click);
                    }
                    // Never leave a modifier stuck down, even when the key itself failed.
                    for m in pressed.into_iter().rev() {
                        let _ = e.key(m, Direction::Release);
                    }
                    result
                })
            }
        }
    }

    fn debug(&self) -> String {
        format!("backend: {}\nlast error: {}\n{:#?}", self.name(), self.last_error.lock().unwrap(), self.snapshot())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn small_scrolls_accumulate_into_notches() {
        let mut rem = 0;
        assert_eq!(take_notches(&mut rem, 50), 0);
        assert_eq!(take_notches(&mut rem, 50), 0);
        assert_eq!(take_notches(&mut rem, 40), 1); // 140 -> one notch, 20 left
        assert_eq!(rem, 20);
        assert_eq!(take_notches(&mut rem, -260), -2); // -240 -> two notches back, 0 left... plus
        assert_eq!(rem, 0);
    }

    #[test]
    fn every_media_key_maps() {
        for k in [Key::PlayPause, Key::Next, Key::Prev, Key::VolUp, Key::VolDown, Key::Mute] {
            let _ = enigo_media_key(k);
        }
    }

    #[test]
    fn unknown_key_is_rejected_before_touching_the_display() {
        let b = DesktopBackend::new();
        let err = b.input(Input::Key { name: "nonsense".into(), mods: vec![] }).unwrap_err();
        assert!(err.to_string().contains("unknown key"));
    }

    /// Needs an X server and tests-support/x11_probe.py logging its events:
    ///   xvfb-run -a sh -c 'python3 tests-support/x11_probe.py /tmp/x.log 8 & sleep 2; cargo test live_x11_input -- --ignored'
    #[cfg(target_os = "linux")]
    #[test]
    #[ignore = "needs an X display with tests-support/x11_probe.py running"]
    fn live_x11_input() {
        let b = DesktopBackend::new();
        b.input(Input::Move(40, 25)).unwrap();
        b.input(Input::Button(MouseButton::Left, ButtonAction::Click)).unwrap();
        b.input(Input::Button(MouseButton::Right, ButtonAction::Click)).unwrap();
        b.input(Input::Scroll(0, 240)).unwrap(); // two notches up = X button 4, twice
        b.input(Input::Scroll(0, -120)).unwrap(); // one notch down = X button 5
        b.input(Input::Text("hi".into())).unwrap();
        b.input(Input::Key { name: "a".into(), mods: vec!["ctrl".into()] }).unwrap();
        b.input(Input::Key { name: "enter".into(), mods: vec![] }).unwrap();
        b.media_key(Key::PlayPause).unwrap();
    }
}
