#!/usr/bin/env python3
"""A fake MPRIS media player for the live D-Bus test (see docs/DESKTOP-TESTING.md).
Run it on a session bus, then: cargo test live_player_roundtrip -- --ignored"""
import asyncio, sys
from dbus_next import Variant
from dbus_next.aio import MessageBus
from dbus_next.service import ServiceInterface, method, dbus_property, PropertyAccess

LOG = sys.argv[1] if len(sys.argv) > 1 else "/dev/null"

def log(s):
    with open(LOG, "a") as f:
        f.write(s + "\n")

class Root(ServiceInterface):
    def __init__(self):
        super().__init__("org.mpris.MediaPlayer2")
    @dbus_property(access=PropertyAccess.READ)
    def Identity(self) -> "s":
        return "Fake Player"

class Player(ServiceInterface):
    def __init__(self):
        super().__init__("org.mpris.MediaPlayer2.Player")
        self.status, self.pos = "Playing", 60_000_000
    @method()
    def PlayPause(self):
        self.status = "Paused" if self.status == "Playing" else "Playing"
        log("PlayPause -> " + self.status)
    @method()
    def Next(self):
        log("Next")
    @method()
    def Previous(self):
        log("Previous")
    @method()
    def SetPosition(self, track_id: "o", position: "x"):
        self.pos = position
        log(f"SetPosition {track_id} {position}")
    @dbus_property(access=PropertyAccess.READ)
    def PlaybackStatus(self) -> "s":
        return self.status
    @dbus_property(access=PropertyAccess.READ)
    def Position(self) -> "x":
        return self.pos
    @dbus_property(access=PropertyAccess.READ)
    def CanSeek(self) -> "b":
        return True
    @dbus_property(access=PropertyAccess.READ)
    def Metadata(self) -> "a{sv}":
        return {
            "xesam:title": Variant("s", "Song"),
            "xesam:artist": Variant("as", ["A", "B"]),
            "mpris:length": Variant("x", 213_000_000),
            "mpris:trackid": Variant("o", "/org/fake/track/1"),
        }

async def main():
    bus = await MessageBus().connect()
    bus.export("/org/mpris/MediaPlayer2", Root())
    bus.export("/org/mpris/MediaPlayer2", Player())
    await bus.request_name("org.mpris.MediaPlayer2.fake")
    log("ready")
    await asyncio.get_event_loop().create_future()

asyncio.run(main())
