import 'dart:io' show Platform;

import 'package:flutter/material.dart';
import 'package:wakelock_plus/wakelock_plus.dart';

import 'app/connection.dart';
import 'app/discovery.dart';
import 'app/pc_store.dart';
import 'app/settings.dart';
import 'app/theme.dart';
import 'core/hid.dart';
import 'core/identity.dart';
import 'ui/home_shell.dart';

Future<void> main() async {
  WidgetsFlutterBinding.ensureInitialized();
  final settings = await AppSettings.load();
  final link = ConnectionController(pcStore: PrefsPcStore(), identityStore: SecureIdentityStore(), discovery: NsdDiscovery());
  await link.load();
  void applyAwake() {
    try {
      WakelockPlus.toggle(enable: settings.keepAwake);
    } catch (_) {}
  }

  settings.addListener(applyAwake);
  applyAwake();
  runApp(PhoneRemoteApp(settings: settings, link: link, bluetooth: Platform.isAndroid ? BluetoothController(ChannelHidPlatform()) : null));
}

class PhoneRemoteApp extends StatelessWidget {
  const PhoneRemoteApp({super.key, required this.settings, required this.link, this.bluetooth});
  final AppSettings settings;
  final ConnectionController link;
  final BluetoothController? bluetooth;

  @override
  Widget build(BuildContext context) {
    return ListenableBuilder(
      listenable: settings,
      builder: (context, _) => MaterialApp(
        title: 'Phone Remote',
        debugShowCheckedModeBanner: false,
        theme: lightTheme(),
        darkTheme: darkTheme(),
        themeMode: switch (settings.theme) {
          'dark' => ThemeMode.dark,
          'light' => ThemeMode.light,
          _ => ThemeMode.system,
        },
        home: HomeShell(link: link, settings: settings, bluetooth: bluetooth),
      ),
    );
  }
}
