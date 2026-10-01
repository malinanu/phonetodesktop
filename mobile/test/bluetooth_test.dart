import 'dart:async';

import 'package:flutter_test/flutter_test.dart';
import 'package:phoneremote/app/settings.dart';
import 'package:phoneremote/core/hid.dart';
import 'package:phoneremote/ui/bluetooth_screen.dart';
import 'package:phoneremote/ui/home_shell.dart';
import 'package:shared_preferences/shared_preferences.dart';

import 'support/fakes.dart';

class FakeHid implements HidPlatform {
  final events = StreamController<HidState>.broadcast();
  final calls = <String>[];
  bool allow = true;
  int skip = 0;
  List<BondedPc> paired = const [
    BondedPc(name: 'Headphones', address: 'AA', isComputer: false),
    BondedPc(name: 'Desk PC', address: 'BB', isComputer: true),
  ];

  @override
  Stream<HidState> get states => events.stream;
  @override
  Future<bool> start() async {
    calls.add('start');
    return allow;
  }

  @override
  Future<void> stop() async => calls.add('stop');
  @override
  Future<List<BondedPc>> bonded() async => paired;
  @override
  Future<bool> connect(String address) async {
    calls.add('connect $address');
    return true;
  }

  @override
  Future<void> discoverable() async => calls.add('discoverable');
  @override
  Future<void> media(int usage) async =>
      calls.add('media ${usage.toRadixString(16)}');
  @override
  Future<bool> key(String name, List<String> mods) async {
    calls.add('key $name ${mods.join('+')}'.trim());
    return true;
  }

  @override
  Future<int> text(String s) async {
    calls.add('text $s');
    return skip;
  }

  @override
  Future<void> move(int dx, int dy) async => calls.add('move $dx $dy');
  @override
  Future<void> button(String button, String action) async =>
      calls.add('button $button $action');
  @override
  Future<void> scroll(int dx, int dy) async => calls.add('scroll $dx $dy');
}

Future<AppSettings> settings() async {
  SharedPreferences.setMockInitialValues({});
  final s = await AppSettings.load();
  await s.set('vibrate', false);
  return s;
}

void main() {
  group('controller', () {
    test('start lists paired computers first, and a denied permission leaves it off', () async {
      final hid = FakeHid();
      final bt = BluetoothController(hid);
      expect(await bt.start(), isTrue);
      expect(bt.started, isTrue);
      expect(bt.pcs.map((p) => p.name), ['Desk PC', 'Headphones']);

      final denied = BluetoothController(FakeHid()..allow = false);
      expect(await denied.start(), isFalse);
      expect(denied.started, isFalse);
      expect(denied.pcs, isEmpty);
    });

    test('state events drive what the UI sees', () async {
      final hid = FakeHid();
      final bt = BluetoothController(hid);
      await bt.start();
      expect(bt.inputConnected, isFalse);
      hid.events.add(
        const HidState(
          ready: true,
          connected: true,
          message: 'Connected',
          device: 'Desk PC',
        ),
      );
      await Future<void>.delayed(Duration.zero);
      expect(bt.inputConnected, isTrue);
      expect(bt.state.device, 'Desk PC');
    });

    test(
      'media keys use the HID consumer usages, and volume steps repeat',
      () async {
        final hid = FakeHid();
        final bt = BluetoothController(hid);
        bt
          ..playPause()
          ..next()
          ..prev()
          ..mute()
          ..volume(2)
          ..volume(-1);
        await Future<void>.delayed(Duration.zero);
        expect(hid.calls, [
          'media cd',
          'media b5',
          'media b6',
          'media e2',
          'media e9',
          'media e9',
          'media ea',
        ]);
      },
    );

    test(
      'input is forwarded, and characters that cannot be typed are explained',
      () async {
        final hid = FakeHid()..skip = 1;
        final bt = BluetoothController(hid);
        bt
          ..mouseMove(3, -4)
          ..mouseButton('left', 'click')
          ..scroll(0, 120)
          ..pressKey('c', const ['ctrl'])
          ..typeText('é');
        await Future<void>.delayed(Duration.zero);
        expect(hid.calls, [
          'move 3 -4',
          'button left click',
          'scroll 0 120',
          'key c ctrl',
          'text é',
        ]);
        expect(bt.notice, contains('Wi-Fi'));
        bt.clearNotice();
        expect(bt.notice, isNull);
      },
    );
  });

  group('screen', () {
    testWidgets('turning it on asks for permission, and explains a refusal', (
      tester,
    ) async {
      final hid = FakeHid()..allow = false;
      final bt = BluetoothController(hid);
      await pumpScreen(
        tester,
        BluetoothScreen(controller: bt, settings: await settings()),
      );
      expect(find.text('Use your phone as a Bluetooth remote'), findsOneWidget);
      await shot(tester, 'bluetooth-off');
      await tester.tap(find.text('Turn on Bluetooth remote'));
      await tester.pumpAndSettle();
      expect(find.textContaining('Allow “Nearby devices”'), findsOneWidget);

      hid.allow = true;
      await tester.tap(find.text('Turn on Bluetooth remote'));
      await tester.pumpAndSettle();
      expect(find.text('Desk PC'), findsOneWidget);
      expect(find.text('Headphones'), findsOneWidget);
      await shot(tester, 'bluetooth-pick');
      await tester.tap(find.text('Desk PC'));
      await tester.tap(find.text('Make this phone visible'));
      expect(hid.calls, containsAll(['connect BB', 'discoverable']));
    });

    testWidgets('when connected the media buttons and the touchpad work', (
      tester,
    ) async {
      final hid = FakeHid();
      final bt = BluetoothController(hid);
      await bt.start();
      hid.events.add(
        const HidState(
          ready: true,
          connected: true,
          message: 'Connected',
          device: 'Desk PC',
        ),
      );
      await pumpScreen(
        tester,
        BluetoothScreen(controller: bt, settings: await settings()),
      );
      await tester.pump();
      await shot(tester, 'bluetooth-connected');
      await tester.tap(find.text('Play/Pause'));
      await tester.tap(find.text('Next'));
      await tester.tap(find.text('Right'));
      await tester.pump();
      expect(
        hid.calls,
        containsAll(['media cd', 'media b5', 'button right click']),
      );
    });
  });

  group('home', () {
    testWidgets('the Bluetooth tab exists only when the platform offers it', (
      tester,
    ) async {
      final s = await settings();
      await pumpScreen(
        tester,
        HomeShell(
          link: FakeLink(state: sampleState()),
          settings: s,
        ),
      );
      expect(find.text('Bluetooth'), findsNothing);
      await pumpScreen(
        tester,
        HomeShell(
          link: FakeLink(state: sampleState()),
          settings: s,
          bluetooth: BluetoothController(FakeHid()),
        ),
      );
      expect(find.text('Bluetooth'), findsOneWidget);
    });

    testWidgets(
      'with no PC paired, Bluetooth can be used without pairing and the user can go back',
      (tester) async {
        final s = await settings();
        await pumpScreen(
          tester,
          HomeShell(
            link: FakeLink(pcs: []),
            settings: s,
            bluetooth: BluetoothController(FakeHid()),
          ),
        );
        await tester.tap(find.text('Use Bluetooth instead'));
        await tester.pumpAndSettle();
        expect(find.text('Turn on Bluetooth remote'), findsOneWidget);
        await tester.tap(find.byTooltip('Back'));
        await tester.pumpAndSettle();
        expect(find.text('Scan QR code'), findsOneWidget);

        await pumpScreen(
          tester,
          HomeShell(
            link: FakeLink(pcs: []),
            settings: s,
          ),
        );
        expect(find.text('Use Bluetooth instead'), findsNothing);
      },
    );
  });
}
