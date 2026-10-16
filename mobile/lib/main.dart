import 'dart:io' show Platform;

import 'package:flutter/material.dart';
import 'package:wakelock_plus/wakelock_plus.dart';

import 'app/account.dart';
import 'app/connection.dart';
import 'app/discovery.dart';
import 'app/media_controls.dart';
import 'app/pc_store.dart';
import 'app/settings.dart';
import 'app/theme.dart';
import 'core/hid.dart';
import 'core/identity.dart';
import 'ui/home_shell.dart';

Future<void> main() async {
  WidgetsFlutterBinding.ensureInitialized();
  final settings = await AppSettings.load();
  final identityStore = SecureIdentityStore();
  final account = AccountController(
    store: SecureAccountStore(),
    identityStore: identityStore,
    deviceName: Platform.isIOS ? 'iPhone' : 'Android phone',
    platform: Platform.isIOS ? 'ios' : 'android',
  );
  await account.load();
  final link = ConnectionController(pcStore: PrefsPcStore(), identityStore: identityStore, discovery: NsdDiscovery(), certProvider: account.freshCert);
  await link.load();
  void applyAwake() {
    try {
      WakelockPlus.toggle(enable: settings.keepAwake);
    } catch (_) {}
  }

  settings.addListener(applyAwake);
  applyAwake();
  final bluetooth = Platform.isAndroid ? BluetoothController(ChannelHidPlatform()) : null;
  // Play, pause and skip from the notification shade and the lock screen (Android), and the lock screen,
  // Dynamic Island and Control Center (iPhone).
  final media = ChannelMediaPlatform();
  MediaControls(link: link, bluetooth: bluetooth, settings: settings, platform: media);
  runApp(PhoneRemoteApp(settings: settings, link: link, account: account, bluetooth: bluetooth));
}

class PhoneRemoteApp extends StatelessWidget {
  const PhoneRemoteApp({super.key, required this.settings, required this.link, this.account, this.bluetooth});
  final AppSettings settings;
  final ConnectionController link;
  final AccountController? account;
  final BluetoothController? bluetooth;

  @override
  Widget build(BuildContext context) {
    return ListenableBuilder(
      listenable: settings,
      builder: (context, _) => MaterialApp(
        title: 'Lanyard',
        debugShowCheckedModeBanner: false,
        theme: lightTheme(),
        darkTheme: darkTheme(),
        themeMode: switch (settings.theme) {
          'dark' => ThemeMode.dark,
          'light' => ThemeMode.light,
          _ => ThemeMode.system,
        },
        home: HomeShell(link: link, settings: settings, account: account, bluetooth: bluetooth),
      ),
    );
  }
}
