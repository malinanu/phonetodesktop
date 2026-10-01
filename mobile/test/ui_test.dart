import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:phoneremote/app/connection.dart';
import 'package:phoneremote/app/settings.dart';
import 'package:phoneremote/core/protocol.dart';
import 'package:phoneremote/core/session.dart';
import 'package:phoneremote/ui/files_screen.dart';
import 'package:phoneremote/ui/home_shell.dart';
import 'package:phoneremote/ui/pair_screen.dart';
import 'package:phoneremote/ui/remote_screen.dart';
import 'package:phoneremote/ui/settings_screen.dart';
import 'package:phoneremote/ui/touchpad_screen.dart';
import 'package:shared_preferences/shared_preferences.dart';

import 'support/fakes.dart';

Future<AppSettings> newSettings([Map<String, Object> values = const {}]) async {
  SharedPreferences.setMockInitialValues({});
  final s = await AppSettings.load();
  for (final e in values.entries) {
    await s.set(e.key, e.value);
  }
  return s;
}

void main() {
  group('remote', () {
    testWidgets('shows what is playing and sends transport commands', (tester) async {
      final link = FakeLink(state: sampleState());
      final settings = await newSettings({'vibrate': false, 'skip': 15});
      await pumpScreen(tester, RemoteScreen(link: link, settings: settings));
      expect(find.text('Everything In Its Right Place'), findsOneWidget);
      expect(find.text('Radiohead'), findsOneWidget);
      expect(find.text('SPOTIFY · PLAYING'), findsOneWidget);
      await shot(tester, 'remote-dark');

      await tester.tap(find.text('Pause'));
      await tester.tap(find.text('Next'));
      await tester.tap(find.text('Previous'));
      await tester.tap(find.text('−15s'));
      await tester.tap(find.text('+15s'));
      await tester.tap(find.byTooltip('Mute'));
      expect(link.calls, ['playPause', 'next', 'prev', 'seekRel -15', 'seekRel 15', 'mute']);
    });

    testWidgets('paused, light theme, nothing playing, and offline all render without layout errors', (tester) async {
      final settings = await newSettings();
      await pumpScreen(tester, RemoteScreen(link: FakeLink(state: sampleState(playing: false, muted: true)), settings: settings), dark: false);
      expect(find.text('SPOTIFY · PAUSED'), findsOneWidget);
      await shot(tester, 'remote-light-paused');

      await pumpScreen(tester, RemoteScreen(link: FakeLink(state: sampleState(players: const [], volume: null)), settings: settings));
      expect(find.text('Nothing playing'), findsOneWidget);
      expect(find.byTooltip('Louder'), findsOneWidget, reason: 'without a volume level it falls back to − and +');
      await shot(tester, 'remote-empty');

      final offline = FakeLink(status: LinkStatus.offline, state: sampleState());
      await pumpScreen(tester, RemoteScreen(link: offline, settings: settings));
      await tester.tap(find.text('Pause'));
      expect(offline.calls, isEmpty, reason: 'buttons are disabled while offline');
    });

    testWidgets('dragging the progress bar seeks, and a picker appears with several players', (tester) async {
      final link = FakeLink(
        state: sampleState(players: [
          const PlayerInfo(id: 'a', app: 'Spotify', title: 'Song A', artist: 'X', playing: true, posMs: 10000, durMs: 200000, canSeek: true),
          const PlayerInfo(id: 'b', app: 'VLC', title: 'Movie', artist: '', playing: false, posMs: 0, durMs: 0, canSeek: false),
        ]),
      );
      await pumpScreen(tester, RemoteScreen(link: link, settings: await newSettings()));
      final slider = find.byType(Slider).first;
      await tester.drag(slider, const Offset(100, 0));
      await tester.pump(const Duration(seconds: 1));
      expect(link.calls.where((c) => c.startsWith('seekAbs')), isNotEmpty);
      expect(find.byType(DropdownButton<String>), findsOneWidget);
      await shot(tester, 'remote-two-players');
      await tester.pump(const Duration(seconds: 1));
    });
  });

  group('touchpad', () {
    Finder pad() => find.textContaining('Drag to move the cursor');

    testWidgets('a tap clicks, a two-finger tap right-clicks, dragging moves, two fingers scroll', (tester) async {
      final link = FakeLink(state: sampleState());
      await pumpScreen(tester, TouchpadScreen(link: link, settings: await newSettings({'vibrate': false})));
      await shot(tester, 'touchpad');
      final center = tester.getCenter(pad());

      await tester.tapAt(center);
      await tester.pump(const Duration(milliseconds: 50));
      expect(link.calls, ['button left click']);

      link.calls.clear();
      final a = await tester.startGesture(center - const Offset(20, 0), pointer: 1);
      final b = await tester.startGesture(center + const Offset(20, 0), pointer: 2);
      await tester.pump(const Duration(milliseconds: 30));
      await a.up();
      await b.up();
      await tester.pump(const Duration(milliseconds: 50));
      expect(link.calls, ['button right click']);

      link.calls.clear();
      final drag = await tester.startGesture(center, pointer: 3);
      for (var i = 0; i < 5; i++) {
        await drag.moveBy(const Offset(10, 6), timeStamp: Duration(milliseconds: 100 * (i + 1)));
        await tester.pump(const Duration(milliseconds: 40));
      }
      await drag.up();
      await tester.pump(const Duration(milliseconds: 400));
      final moves = link.calls.where((c) => c.startsWith('move')).map((c) => c.split(' ')).toList();
      expect(moves, isNotEmpty);
      expect(moves.fold<int>(0, (s, m) => s + int.parse(m[1])), greaterThan(40), reason: 'moving right moves the pointer right');
      expect(moves.fold<int>(0, (s, m) => s + int.parse(m[2])), greaterThan(20));
      expect(link.calls.any((c) => c.startsWith('button')), isFalse, reason: 'a drag is not a click');

      link.calls.clear();
      final f1 = await tester.startGesture(center - const Offset(30, 0), pointer: 4);
      final f2 = await tester.startGesture(center + const Offset(30, 0), pointer: 5);
      for (var i = 0; i < 6; i++) {
        await f1.moveBy(const Offset(0, 12), timeStamp: Duration(milliseconds: 100 * (i + 1)));
        await f2.moveBy(const Offset(0, 12), timeStamp: Duration(milliseconds: 100 * (i + 1)));
        await tester.pump(const Duration(milliseconds: 40));
      }
      await f1.up();
      await f2.up();
      await tester.pump(const Duration(milliseconds: 400));
      final scrolls = link.calls.where((c) => c.startsWith('scroll')).map((c) => c.split(' ')).toList();
      expect(scrolls, isNotEmpty);
      expect(scrolls.fold<int>(0, (s, m) => s + int.parse(m[2])), greaterThan(0), reason: 'natural scrolling: fingers down = positive wheel');
      expect(link.calls.any((c) => c.startsWith('button')), isFalse);
    });

    testWidgets('classic scroll direction reverses, and tap-to-click can be switched off', (tester) async {
      final link = FakeLink(state: sampleState());
      await pumpScreen(tester, TouchpadScreen(link: link, settings: await newSettings({'vibrate': false, 'scrollNatural': false, 'tapToClick': false})));
      final center = tester.getCenter(pad());
      await tester.tapAt(center);
      await tester.pump(const Duration(milliseconds: 50));
      expect(link.calls, isEmpty);
      final f1 = await tester.startGesture(center - const Offset(30, 0), pointer: 1);
      final f2 = await tester.startGesture(center + const Offset(30, 0), pointer: 2);
      for (var i = 0; i < 6; i++) {
        await f1.moveBy(const Offset(0, 12), timeStamp: Duration(milliseconds: 100 * (i + 1)));
        await f2.moveBy(const Offset(0, 12), timeStamp: Duration(milliseconds: 100 * (i + 1)));
        await tester.pump(const Duration(milliseconds: 40));
      }
      await f1.up();
      await f2.up();
      await tester.pump(const Duration(milliseconds: 400));
      final sum = link.calls.where((c) => c.startsWith('scroll')).map((c) => int.parse(c.split(' ')[2])).fold<int>(0, (a, b) => a + b);
      expect(sum, lessThan(0));
    });

    testWidgets('the on-screen buttons and typing reach the PC', (tester) async {
      final link = FakeLink(state: sampleState());
      await pumpScreen(tester, TouchpadScreen(link: link, settings: await newSettings({'vibrate': false})));
      await tester.tap(find.text('Right'));
      await tester.tap(find.text('Esc'));
      await tester.scrollUntilVisible(find.text('Copy'), 200, scrollable: find.byType(Scrollable).last);
      await tester.drag(find.byType(Scrollable).last, const Offset(-150, 0));
      await tester.pump();
      await tester.tap(find.text('Copy'));
      await tester.enterText(find.byType(TextField), 'hello');
      await tester.testTextInput.receiveAction(TextInputAction.send);
      await tester.pump();
      expect(link.calls, ['button right click', 'key esc', 'key c ctrl', 'text hello', 'key enter']);
    });

    testWidgets('explains when the PC has switched mouse and keyboard off for this phone', (tester) async {
      await pumpScreen(tester, TouchpadScreen(link: FakeLink(state: sampleState(), input: false), settings: await newSettings()));
      expect(find.textContaining('Mouse and keyboard are off'), findsOneWidget);
      await shot(tester, 'touchpad-off');
    });
  });

  group('files', () {
    testWidgets('opens the configured address in the browser', (tester) async {
      Uri? opened;
      final settings = await newSettings({'filesUrl': 'files.example.com'});
      await pumpScreen(tester, FilesScreen(settings: settings, open: (u) async {
        opened = u;
        return true;
      }));
      await shot(tester, 'files');
      await tester.tap(find.text('Open Send files'));
      await tester.pump();
      expect(opened.toString(), 'https://files.example.com');
    });

    testWidgets('asks for an address when none is set, and never opens an insecure one', (tester) async {
      Uri? opened;
      var settingsOpened = false;
      final settings = await newSettings({'filesUrl': 'http://insecure.example.com'});
      await pumpScreen(tester, FilesScreen(settings: settings, open: (u) async {
        opened = u;
        return true;
      }, onOpenSettings: () => settingsOpened = true));
      expect(find.text('Open Settings'), findsOneWidget);
      await tester.tap(find.text('Open Settings'));
      expect(opened, isNull);
      expect(settingsOpened, isTrue);
    });
  });

  group('settings', () {
    testWidgets('changes are saved, the file server is validated, and a PC can be forgotten', (tester) async {
      final settings = await newSettings();
      final link = FakeLink();
      await pumpScreen(tester, SettingsScreen(settings: settings, link: link), size: const Size(390, 1500));
      await shot(tester, 'settings');

      await tester.tap(find.text('Dark'));
      await tester.pump();
      expect(settings.theme, 'dark');
      await tester.tap(find.text('30 s'));
      await tester.pump();
      expect(settings.skip, 30);

      await tester.enterText(find.byType(TextField), 'http://nope.example.com');
      await tester.pump();
      expect(find.textContaining('Use an https:// address'), findsOneWidget);
      expect(settings.filesUrl, '');
      await tester.enterText(find.byType(TextField), 'Files.Example.com');
      await tester.pump();
      expect(settings.filesUrl, 'https://files.example.com');

      await tester.scrollUntilVisible(find.text('Forget'), 300, scrollable: find.byType(Scrollable).first);
      await tester.tap(find.text('Forget').first);
      await tester.pumpAndSettle();
      expect(find.text('Forget Living room PC?'), findsOneWidget);
      await tester.tap(find.text('Forget').last);
      await tester.pumpAndSettle();
      expect(link.calls, ['forget pc-1']);

      await tester.tap(find.text('Reset settings to defaults'));
      await tester.pump();
      expect(settings.theme, 'system');
      expect(settings.skip, 10);
    });

    testWidgets('settings survive a restart and ignore damaged values', (tester) async {
      SharedPreferences.setMockInitialValues({'pr.settings': '{"theme":"light","skip":"ten","volStep":5,"unknown":1,"padSpeed":2}'});
      final s = await AppSettings.load();
      expect((s.theme, s.skip, s.volStep, s.padSpeed), ('light', 10, 5, 2.0));
      SharedPreferences.setMockInitialValues({'pr.settings': 'not json'});
      expect((await AppSettings.load()).theme, 'system');
    });
  });

  group('home', () {
    testWidgets('each connection problem says what is wrong and what to do', (tester) async {
      final settings = await newSettings();
      final cases = {
        LinkStatus.connecting: 'Connecting to Living room PC…',
        LinkStatus.offline: 'Can’t reach Living room PC. Same Wi-Fi? Retrying…',
        LinkStatus.revoked: 'Living room PC removed this phone.',
        LinkStatus.keyChanged: 'Living room PC now has a different security key',
        LinkStatus.insecure: 'Living room PC needs the newest Phone Remote',
      };
      for (final e in cases.entries) {
        final link = FakeLink(status: e.key, state: sampleState());
        await pumpScreen(tester, HomeShell(link: link, settings: settings));
        expect(find.textContaining(e.value), findsOneWidget, reason: '${e.key}');
        await shot(tester, 'home-${e.key.name}');
      }
      final offline = FakeLink(status: LinkStatus.offline);
      await pumpScreen(tester, HomeShell(link: offline, settings: settings));
      await tester.tap(find.text('Try again'));
      expect(offline.calls, ['reconnect']);

      final gone = FakeLink(status: LinkStatus.revoked);
      await pumpScreen(tester, HomeShell(link: gone, settings: settings));
      await tester.tap(find.text('Forget'));
      await tester.pump();
      expect(gone.calls, ['forget pc-1']);
    });

    testWidgets('connected: no banner, tabs switch between remote, touchpad and files', (tester) async {
      final settings = await newSettings();
      await pumpScreen(tester, HomeShell(link: FakeLink(state: sampleState()), settings: settings));
      expect(find.textContaining('Can’t reach'), findsNothing);
      expect(find.text('Living room PC'), findsOneWidget);
      await tester.tap(find.text('Touchpad'));
      await tester.pump();
      expect(find.textContaining('Drag to move the cursor'), findsOneWidget);
      await tester.tap(find.text('Files'));
      await tester.pump();
      expect(find.text('Send files.'), findsOneWidget);
    });

    testWidgets('with no PC paired the first screen is the pairing screen', (tester) async {
      await pumpScreen(tester, HomeShell(link: FakeLink(pcs: []), settings: await newSettings()));
      expect(find.text('Take the remote.'), findsOneWidget);
      expect(find.text('Scan QR code'), findsOneWidget);
      await shot(tester, 'pair');
    });
  });

  group('pairing', () {
    const info = PairingInfo(host: '192.168.1.20', port: 8765, code: 'C', pcId: 'pc-9', name: 'Desk PC', fingerprint: 'fp');

    Future<FakeLink> run(WidgetTester tester, {required Future<PairingInfo?> Function(BuildContext) scan, Pairer? pair}) async {
      final link = FakeLink(pcs: []);
      await pumpScreen(tester, PairScreen(link: link, scan: scan, pair: pair ?? (i, l, p) async => PairResult.approved));
      await tester.tap(find.text('Scan QR code'));
      await tester.pumpAndSettle();
      return link;
    }

    testWidgets('an approved pairing saves the PC', (tester) async {
      var paired = false;
      final link = FakeLink(pcs: []);
      await pumpScreen(tester, PairScreen(link: link, scan: (_) async => info, pair: (i, l, p) async => PairResult.approved, onPaired: () => paired = true));
      await tester.tap(find.text('Scan QR code'));
      await tester.pumpAndSettle();
      expect(link.calls, ['addPaired pc-9']);
      expect(paired, isTrue);
    });

    testWidgets('while waiting for the owner it says so', (tester) async {
      final answer = Completer2();
      final link = FakeLink(pcs: []);
      await pumpScreen(tester, PairScreen(link: link, scan: (_) async => info, pair: (i, l, onPending) async {
        onPending();
        await answer.future;
        return PairResult.denied;
      }));
      await tester.tap(find.text('Scan QR code'));
      await tester.pump(const Duration(milliseconds: 50));
      expect(find.textContaining('Waiting for you to allow this phone on Desk PC'), findsOneWidget);
      await shot(tester, 'pair-waiting');
      answer.complete();
      await tester.pumpAndSettle();
      expect(find.textContaining('did not allow this phone'), findsOneWidget);
      expect(link.calls, isEmpty);
    });

    testWidgets('each failure has a plain-language message and saves nothing', (tester) async {
      final messages = {
        PairResult.denied: 'did not allow this phone',
        PairResult.expired: 'Nobody answered on Desk PC in time',
        PairResult.badCode: 'out of date',
        PairResult.badKey: 'could not be set up',
      };
      for (final e in messages.entries) {
        final link = await run(tester, scan: (_) async => info, pair: (i, l, p) async => e.key);
        expect(find.textContaining(e.value), findsOneWidget, reason: '${e.key}');
        expect(link.calls, isEmpty);
      }
    });

    testWidgets('a code from a PC without a secure connection is refused before connecting', (tester) async {
      var connected = false;
      final link = await run(tester, scan: (_) async => const PairingInfo(host: 'h', port: 1, code: 'c', pcId: 'p', name: 'Old PC', fingerprint: ''), pair: (i, l, p) async {
        connected = true;
        return PairResult.approved;
      });
      expect(find.textContaining('does not offer a secure connection'), findsOneWidget);
      expect(connected, isFalse);
      expect(link.calls, isEmpty);
    });

    testWidgets('a network failure is explained, and cancelling the scan does nothing', (tester) async {
      await run(tester, scan: (_) async => info, pair: (i, l, p) async => throw Exception('no route'));
      expect(find.textContaining('Could not reach Desk PC'), findsOneWidget);
      final link = await run(tester, scan: (_) async => null);
      expect(find.textContaining('Could not reach'), findsNothing);
      expect(link.calls, isEmpty);
    });
  });
}

class Completer2 {
  final _c = Future<void>.delayed(const Duration(milliseconds: 200));
  Future<void> get future => _c;
  void complete() {}
}
