//! Linux media sessions: every MPRIS player on the user's D-Bus session bus (Spotify, VLC, mpv with the
//! mpris script, Firefox, Chromium, Rhythmbox, ...). Needs no extra permissions.

use crate::protocol::PlayerInfo;
use anyhow::{anyhow, Result};
use std::collections::HashMap;
use std::sync::Mutex;
use zbus::blocking::fdo::{DBusProxy, PropertiesProxy};
use zbus::blocking::{Connection, Proxy};
use zbus::names::InterfaceName;
use zbus::zvariant::{ObjectPath, OwnedValue};

const PREFIX: &str = "org.mpris.MediaPlayer2.";
const PATH: &str = "/org/mpris/MediaPlayer2";
const IFACE_PLAYER: &str = "org.mpris.MediaPlayer2.Player";
const IFACE_ROOT: &str = "org.mpris.MediaPlayer2";

type Props = HashMap<String, OwnedValue>;

pub struct Mpris {
    conn: Mutex<Option<Connection>>,
}

impl Mpris {
    pub fn new() -> Self {
        Mpris { conn: Mutex::new(None) }
    }

    /// The session-bus connection, opened on first use and reopened after an error
    /// (the agent may start before the desktop session's bus is ready).
    fn conn(&self) -> Result<Connection> {
        let mut g = self.conn.lock().unwrap();
        if g.is_none() {
            *g = Some(Connection::session().map_err(|e| anyhow!("no D-Bus session bus: {e}"))?);
        }
        Ok(g.as_ref().unwrap().clone())
    }

    fn forget_conn(&self) {
        *self.conn.lock().unwrap() = None;
    }

    pub fn snapshot(&self) -> Result<Vec<PlayerInfo>> {
        let conn = self.conn()?;
        let names = match DBusProxy::new(&conn).map_err(anyhow::Error::from).and_then(|d| d.list_names().map_err(anyhow::Error::from)) {
            Ok(n) => n,
            Err(e) => {
                self.forget_conn();
                return Err(anyhow!("D-Bus: {e}"));
            }
        };
        let mut players = vec![];
        for name in names.iter().map(|n| n.to_string()).filter(|n| n.starts_with(PREFIX)) {
            // A player that vanishes or misbehaves must not hide the others.
            if let Ok(p) = read_player(&conn, &name) {
                players.push(p);
            }
        }
        // Playing sessions first, so the controller's default target is the one making sound.
        players.sort_by_key(|p| !p.playing);
        Ok(players)
    }

    fn player_proxy<'a>(&self, conn: &'a Connection, id: &'a str) -> Result<Proxy<'a>> {
        check_id(id)?;
        Ok(Proxy::new(conn, id, PATH, IFACE_PLAYER)?)
    }

    pub fn call(&self, id: &str, method: &'static str) -> Result<bool> {
        let conn = self.conn()?;
        let p = self.player_proxy(&conn, id)?;
        Ok(p.call::<_, _, ()>(method, &()).is_ok())
    }

    pub fn seek_abs(&self, id: &str, pos_ms: i64) -> Result<bool> {
        let conn = self.conn()?;
        let props = get_all(&conn, id, IFACE_PLAYER)?;
        let meta = metadata(&props);
        let Some(track) = meta.get("mpris:trackid").and_then(track_path) else { return Ok(false) };
        let p = self.player_proxy(&conn, id)?;
        Ok(p.call::<_, _, ()>("SetPosition", &(track, pos_ms.saturating_mul(1000))).is_ok())
    }
}

/// Only genuine MPRIS bus names may be addressed: the id comes from the phone's command via the controller.
fn check_id(id: &str) -> Result<()> {
    if id.starts_with(PREFIX) && id.len() < 255 && id.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'.' || b == b'_' || b == b'-' || b == b':') {
        Ok(())
    } else {
        Err(anyhow!("not an MPRIS player: {id}"))
    }
}

fn get_all(conn: &Connection, bus_name: &str, iface: &'static str) -> Result<Props> {
    check_id(bus_name)?;
    let proxy = PropertiesProxy::builder(conn).destination(bus_name.to_string())?.path(PATH)?.build()?;
    Ok(proxy.get_all(InterfaceName::from_static_str_unchecked(iface))?)
}

fn read_player(conn: &Connection, name: &str) -> Result<PlayerInfo> {
    let player = get_all(conn, name, IFACE_PLAYER)?;
    let identity = get_all(conn, name, IFACE_ROOT)
        .ok()
        .and_then(|r| r.get("Identity").and_then(|v| String::try_from(v.clone()).ok()))
        .unwrap_or_default();
    Ok(player_info(name, &identity, &player))
}

fn metadata(props: &Props) -> Props {
    props.get("Metadata").and_then(|v| Props::try_from(v.clone()).ok()).unwrap_or_default()
}

fn track_path(v: &OwnedValue) -> Option<ObjectPath<'static>> {
    ObjectPath::try_from(v.clone()).ok().map(|p| p.into_owned()).or_else(|| {
        String::try_from(v.clone()).ok().and_then(|s| ObjectPath::try_from(s).ok()).map(|p| p.into_owned())
    })
}

fn int(v: &OwnedValue) -> Option<i64> {
    i64::try_from(v.clone()).ok().or_else(|| u64::try_from(v.clone()).ok().map(|n| n.min(i64::MAX as u64) as i64))
}

/// Turn a player's raw MPRIS properties into the app's player record. Pure, so it is unit-tested.
pub fn player_info(bus_name: &str, identity: &str, player: &Props) -> PlayerInfo {
    let meta = metadata(player);
    let text = |k: &str| meta.get(k).and_then(|v| String::try_from(v.clone()).ok()).unwrap_or_default();
    let artist = meta
        .get("xesam:artist")
        .and_then(|v| Vec::<String>::try_from(v.clone()).ok().map(|a| a.join(", ")).or_else(|| String::try_from(v.clone()).ok()))
        .unwrap_or_default();
    let app = if identity.is_empty() { bus_name.trim_start_matches(PREFIX).split('.').next().unwrap_or("player").to_string() } else { identity.to_string() };
    let playing = player.get("PlaybackStatus").and_then(|v| String::try_from(v.clone()).ok()).map(|s| s == "Playing").unwrap_or(false);
    let pos_us = player.get("Position").and_then(int).unwrap_or(0).max(0);
    let dur_us = meta.get("mpris:length").and_then(int).unwrap_or(0).max(0);
    let can_seek = player.get("CanSeek").and_then(|v| bool::try_from(v.clone()).ok()).unwrap_or(false);
    PlayerInfo {
        id: bus_name.to_string(),
        app,
        title: text("xesam:title"),
        artist,
        playing,
        pos_ms: pos_us / 1000,
        dur_ms: dur_us / 1000,
        can_seek,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use zbus::zvariant::Value;

    fn ov<'a, T: Into<Value<'a>>>(v: T) -> OwnedValue {
        OwnedValue::try_from(v.into()).unwrap()
    }

    #[test]
    fn maps_properties_to_a_player() {
        let mut meta = Props::new();
        meta.insert("xesam:title".into(), ov("Song"));
        meta.insert("xesam:artist".into(), ov(vec!["A".to_string(), "B".to_string()]));
        meta.insert("mpris:length".into(), ov(213_000_000i64));
        let mut p = Props::new();
        p.insert("PlaybackStatus".into(), ov("Playing"));
        p.insert("Position".into(), ov(60_500_000i64));
        p.insert("CanSeek".into(), ov(true));
        p.insert("Metadata".into(), ov(Value::from(zbus::zvariant::Dict::from(meta))));
        let i = player_info("org.mpris.MediaPlayer2.spotify", "Spotify", &p);
        assert_eq!((i.app.as_str(), i.title.as_str(), i.artist.as_str()), ("Spotify", "Song", "A, B"));
        assert!(i.playing && i.can_seek);
        assert_eq!((i.pos_ms, i.dur_ms), (60_500, 213_000));
    }

    #[test]
    fn missing_properties_are_harmless() {
        let i = player_info("org.mpris.MediaPlayer2.vlc.instance123", "", &Props::new());
        assert_eq!(i.app, "vlc");
        assert!(!i.playing && !i.can_seek);
        assert_eq!((i.pos_ms, i.dur_ms), (0, 0));
    }

    #[test]
    fn only_mpris_names_may_be_addressed() {
        assert!(check_id("org.mpris.MediaPlayer2.spotify").is_ok());
        for bad in ["", "org.freedesktop.DBus", "org.mpris.MediaPlayer2.a b", "org.mpris.MediaPlayer2.x;y", ":1.42"] {
            assert!(check_id(bad).is_err(), "{bad:?}");
        }
    }

    /// Needs a session bus with the fake player from tests-support/fake_mpris.py:
    ///   dbus-run-session -- sh -c 'python3 tests-support/fake_mpris.py /tmp/fake.log & sleep 2; cargo test live_player_roundtrip -- --ignored'
    #[test]
    #[ignore = "needs a D-Bus session with tests-support/fake_mpris.py running"]
    fn live_player_roundtrip() {
        let m = Mpris::new();
        let find = |m: &Mpris| m.snapshot().unwrap().into_iter().find(|p| p.app == "Fake Player").expect("fake player on the bus");
        let p = find(&m);
        assert_eq!((p.title.as_str(), p.artist.as_str()), ("Song", "A, B"));
        assert!(p.playing && p.can_seek);
        assert_eq!((p.pos_ms, p.dur_ms), (60_000, 213_000));

        assert!(m.call(&p.id, "PlayPause").unwrap());
        assert!(!find(&m).playing, "PlayPause should pause");
        assert!(m.call(&p.id, "Next").unwrap());
        assert!(m.call(&p.id, "Previous").unwrap());

        assert!(m.seek_abs(&p.id, 90_000).unwrap());
        assert_eq!(find(&m).pos_ms, 90_000);

        // Names that are not MPRIS players are refused before any D-Bus call.
        assert!(m.call("org.freedesktop.DBus", "PlayPause").is_err());
    }
}
