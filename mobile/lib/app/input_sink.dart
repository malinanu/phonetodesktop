import 'package:flutter/foundation.dart';

/// Where the touchpad sends mouse and keyboard input: the PC over Wi-Fi, or a Bluetooth link.
abstract interface class InputSink implements Listenable {
  /// The link to the PC is up.
  bool get inputConnected;

  /// The PC lets this phone use the mouse and keyboard.
  bool get inputAllowed;

  void mouseMove(int dx, int dy);
  void mouseButton(String button, String action);
  void scroll(int dx, int dy);
  void typeText(String s);
  void pressKey(String name, [List<String> mods = const []]);
}
