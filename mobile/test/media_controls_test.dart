// The notification / lock-screen controls: what they show for each connection, and what their buttons do.
import 'dart:async';

import 'package:flutter_test/flutter_test.dart';
import 'package:phoneremote/app/connection.dart';
import 'package:phoneremote/app/media_controls.dart';
import 'package:phoneremote/app/settings.dart';
import 'package:phoneremote/core/hid.dart';
import 'package:phoneremote/core/identity.dart';
import 'package:shared_preferences/shared_preferences.dart';

import 'bluetooth_test.dart' show FakeHid;
import 'support/fakes.dart';

class FakeMediaPlatform implements MediaPlatform {
  final shows = <MediaSnapshot>[];
  final options = <Map<String, Object>>[];
  final creds = <Map<String, Object>?>[];
  int hides = 0;
  int permissionAsks = 0;
  final taps = StreamController<(String, int?)>.broadcast();

  @override
  Stream<(String, int?)> get commands => taps.stream;
  @override
  Future<void> show(MediaSnapshot s, {required Map<String, Object> options}) async {
    shows.add(s);
    this.options.add(options);
  }

  @override
  Future<void> hide() async => hides++;
  @override
  Future<bool> requestPermission() async {
    permissionAsks++;
    return true;
  }

  @override
  Future<void> setCredentials(Map<String, Object>? credentials) async => creds.add(credentials);
}

class LinkWithIdentity extends FakeLink {
  LinkWithIdentity(this.id) : super(state: sampleState());
  final DeviceIdentity id;
  @override
  DeviceIdentity get identity => id;
}

void main() {
  late AppSettings settings;
  late FakeMediaPlatform platform;

  setUp(() async {
    SharedPreferences.setMockInitialValues({});
    settings = await AppSettings.load();
    platform = FakeMediaPlatform();
  });

  Future<void> settle() => Future<void>.delayed(Duration.zero);

  test('shows the song while connected, follows changes, and hides when the PC is forgotten', () async {
    final link = FakeLink(state: sampleState());
    final m = MediaControls(link: link, settings: settings, platform: platform, isIOS: false);
    addTearDown(m.dispose);
    expect(platform.shows.single.title, 'Everything In Its Right Place');
    expect(platform.shows.single.artist, 'Radiohead');
    expect(platform.shows.single.mode, 'wifi');
    expect(platform.shows.single.playing, isTrue);
    expect(platform.shows.single.canSeek, isTrue);
    expect(platform.permissionAsks, 1, reason: 'asked once, when there is first something to show');

    // the same picture is not sent again
    link.update(state: sampleState());
    expect(platform.shows, hasLength(1));

    link.update(state: sampleState(playing: false));
    expect(platform.shows.last.playing, isFalse);

    // while reconnecting the controls stay, so the system keeps the app alive and it can reconnect
    link.update(status: LinkStatus.offline);
    expect(platform.shows.last.connected, isFalse);
    expect(platform.shows.last.title, contains('Living room PC'));
    expect(platform.hides, 0);

    await link.forget('pc-1');
    link.update(status: LinkStatus.idle);
    expect(platform.hides, 1);
    expect(m.shown, isNull);
    expect(platform.permissionAsks, 1);
  });

  test('after the grace period without the PC the controls go away, and come back with the connection', () async {
    final link = FakeLink(state: sampleState());
    final m = MediaControls(link: link, settings: settings, platform: platform, isIOS: false, offlineGrace: const Duration(milliseconds: 50));
    addTearDown(m.dispose);
    link.update(status: LinkStatus.offline);
    expect(platform.hides, 0, reason: 'a short drop keeps them');
    link.update(status: LinkStatus.connecting);
    await Future<void>.delayed(const Duration(milliseconds: 120));
    expect(platform.hides, 1);
    expect(m.shown, isNull);
    link.update(status: LinkStatus.connected);
    expect(m.shown?.connected, isTrue);
    // a new drop starts a new grace period
    link.update(status: LinkStatus.offline);
    expect(m.shown, isNotNull);
  });

  test('buttons control the PC over Wi-Fi, with the user’s skip and volume steps', () async {
    await settings.set('skip', 15);
    await settings.set('volStep', 5);
    final link = FakeLink(state: sampleState());
    final m = MediaControls(link: link, settings: settings, platform: platform, isIOS: false);
    addTearDown(m.dispose);
    for (final c in [
      ('play_pause', null),
      ('next', null),
      ('prev', null),
      ('seek_to', 120000),
      ('seek_fwd', null),
      ('seek_back', null),
      ('vol_up', null),
      ('vol_down', null),
    ]) {
      platform.taps.add(c);
    }
    await settle();
    expect(link.calls, ['playPause', 'next', 'prev', 'seekAbs 120000', 'seekRel 15', 'seekRel -15', 'volume 5', 'volume -5']);
  });

  test('Bluetooth mode shows a remote without song details and sends media keys', () async {
    final link = FakeLink(status: LinkStatus.idle, pcs: const []);
    final hid = FakeHid();
    final bt = BluetoothController(hid);
    await bt.start();
    final m = MediaControls(link: link, bluetooth: bt, settings: settings, platform: platform, isIOS: false);
    addTearDown(m.dispose);
    expect(platform.shows, isEmpty, reason: 'nothing is connected yet');

    hid.events.add(const HidState(ready: true, connected: true, device: 'Desk PC'));
    await settle();
    expect(platform.shows.single.mode, 'bluetooth');
    expect(platform.shows.single.artist, 'Desk PC');
    expect(platform.shows.single.canSeek, isFalse);

    platform.taps
      ..add(('play_pause', null))
      ..add(('next', null))
      ..add(('seek_to', 1000)) // no seeking over Bluetooth
      ..add(('vol_up', null));
    await settle();
    expect(hid.calls.where((c) => c.startsWith('media')), [
      'media ${MediaKey.playPause.toRadixString(16)}',
      'media ${MediaKey.next.toRadixString(16)}',
      'media ${MediaKey.volUp.toRadixString(16)}',
    ]);
    expect(link.calls, isEmpty);

    hid.events.add(const HidState(ready: true, connected: false));
    await settle();
    expect(platform.hides, 1);
  });

  test('the switch in Settings turns them off and on; the full iPhone player is passed along', () async {
    final link = FakeLink(state: sampleState());
    final m = MediaControls(link: link, settings: settings, platform: platform, isIOS: false);
    addTearDown(m.dispose);
    expect(platform.options.last['iosNowPlaying'], isFalse);
    await settings.set('lockControls', false);
    expect(platform.hides, 1);
    await settings.set('lockControls', true);
    expect(platform.shows, hasLength(2));
    expect(platform.permissionAsks, 2, reason: 'turning it back on asks again');
    await settings.set('iosNowPlaying', true);
    expect(platform.options.last['iosNowPlaying'], isTrue);
  });

  test('on iPhone the native buttons get the login details for the Wi-Fi PC, and lose them when turned off', () async {
    final id = await DeviceIdentity.generate();
    final link = LinkWithIdentity(id);
    final m = MediaControls(link: link, settings: settings, platform: platform, isIOS: true);
    addTearDown(m.dispose);
    await settle();
    final c = platform.creds.single!;
    expect(c['pcId'], 'pc-1');
    expect(c['host'], '192.168.1.20');
    expect(c['port'], 8765);
    expect(c['fp'], 'fp');
    expect(c['deviceId'], id.deviceId);
    expect(c['seed'], id.seedB64);
    expect(c.containsKey('cert'), isFalse);
    link.update(state: sampleState(playing: false));
    await settle();
    expect(platform.creds, hasLength(1), reason: 'only sent again when the PC changes');
    await settings.set('lockControls', false);
    await settle();
    expect(platform.creds.last, isNull, reason: 'turning the controls off removes the stored login');
  });

  test('Android never hands the login details to the native side', () async {
    final link = LinkWithIdentity(await DeviceIdentity.generate());
    final m = MediaControls(link: link, settings: settings, platform: platform, isIOS: false);
    addTearDown(m.dispose);
    await settle();
    expect(platform.creds, isEmpty);
  });
}
