import 'package:flutter/material.dart';

/// Colours from shared/base.css (the same tokens the PC pages and the old Android app use): warm neutrals, one amber
/// accent. Text colours are checked for 4.5:1 contrast in both themes (see test/theme_test.dart).
class PrColors extends ThemeExtension<PrColors> {
  const PrColors({required this.bg, required this.surface, required this.surface2, required this.line, required this.ink, required this.dim, required this.accent, required this.accentText, required this.onAccent, required this.good, required this.bad});

  final Color bg, surface, surface2, line, ink, dim, accent, accentText, onAccent, good, bad;

  static const dark = PrColors(
    bg: Color(0xFF15110E), surface: Color(0xFF1F1A16), surface2: Color(0xFF2A231D), line: Color(0xFF372E26),
    ink: Color(0xFFF5EBDD), dim: Color(0xFFB3A594), accent: Color(0xFFFF9A3C), accentText: Color(0xFFFF9A3C),
    onAccent: Color(0xFF1B1006), good: Color(0xFF86D9A6), bad: Color(0xFFFF8A7A),
  );

  static const light = PrColors(
    bg: Color(0xFFF7F1E8), surface: Color(0xFFFFFAF2), surface2: Color(0xFFEFE6D8), line: Color(0xFFE0D3C0),
    ink: Color(0xFF241C14), dim: Color(0xFF6C6050), accent: Color(0xFFE8710A), accentText: Color(0xFFA04A00),
    onAccent: Color(0xFF1B1006), good: Color(0xFF17703F), bad: Color(0xFFB0352A),
  );

  @override
  PrColors copyWith() => this;

  @override
  PrColors lerp(PrColors? other, double t) => t < 0.5 ? this : (other ?? this);
}

extension PrTheme on BuildContext {
  PrColors get pr => Theme.of(this).extension<PrColors>()!;
}

ThemeData buildTheme(PrColors c, Brightness brightness) {
  final text = ThemeData(brightness: brightness, fontFamily: 'Bricolage').textTheme.apply(bodyColor: c.ink, displayColor: c.ink);
  return ThemeData(
    useMaterial3: true,
    brightness: brightness,
    fontFamily: 'Bricolage',
    scaffoldBackgroundColor: c.bg,
    colorScheme: ColorScheme(
      brightness: brightness, primary: c.accent, onPrimary: c.onAccent, secondary: c.accent, onSecondary: c.onAccent,
      error: c.bad, onError: c.onAccent, surface: c.surface, onSurface: c.ink, surfaceContainerHighest: c.surface2, outline: c.line,
    ),
    textTheme: text,
    extensions: [c],
    appBarTheme: AppBarTheme(backgroundColor: c.bg, foregroundColor: c.ink, elevation: 0, scrolledUnderElevation: 0),
    navigationBarTheme: NavigationBarThemeData(
      backgroundColor: c.surface,
      indicatorColor: c.surface2,
      labelTextStyle: WidgetStatePropertyAll(TextStyle(fontFamily: 'Bricolage', fontWeight: FontWeight.w600, color: c.dim, fontSize: 12)),
      iconTheme: WidgetStateProperty.resolveWith((s) => IconThemeData(color: s.contains(WidgetState.selected) ? c.accentText : c.dim)),
    ),
    sliderTheme: SliderThemeData(activeTrackColor: c.accent, thumbColor: c.accent, inactiveTrackColor: c.surface2, overlayColor: c.accent.withValues(alpha: .15)),
    filledButtonTheme: FilledButtonThemeData(
      style: FilledButton.styleFrom(backgroundColor: c.accent, foregroundColor: c.onAccent, minimumSize: const Size(48, 52), shape: RoundedRectangleBorder(borderRadius: BorderRadius.circular(16)), textStyle: const TextStyle(fontFamily: 'Bricolage', fontWeight: FontWeight.w700, fontSize: 16)),
    ),
    outlinedButtonTheme: OutlinedButtonThemeData(
      style: OutlinedButton.styleFrom(foregroundColor: c.ink, side: BorderSide(color: c.line), minimumSize: const Size(48, 52), shape: RoundedRectangleBorder(borderRadius: BorderRadius.circular(16)), textStyle: const TextStyle(fontFamily: 'Bricolage', fontWeight: FontWeight.w600, fontSize: 16)),
    ),
    textButtonTheme: TextButtonThemeData(style: TextButton.styleFrom(foregroundColor: c.accentText, textStyle: const TextStyle(fontFamily: 'Bricolage', fontWeight: FontWeight.w600))),
    switchTheme: SwitchThemeData(
      thumbColor: WidgetStateProperty.resolveWith((s) => s.contains(WidgetState.selected) ? c.onAccent : c.dim),
      trackColor: WidgetStateProperty.resolveWith((s) => s.contains(WidgetState.selected) ? c.accent : c.surface2),
      trackOutlineColor: const WidgetStatePropertyAll(Colors.transparent),
    ),
    dividerColor: c.line,
    inputDecorationTheme: InputDecorationTheme(
      filled: true, fillColor: c.surface,
      border: OutlineInputBorder(borderRadius: BorderRadius.circular(14), borderSide: BorderSide(color: c.line)),
      enabledBorder: OutlineInputBorder(borderRadius: BorderRadius.circular(14), borderSide: BorderSide(color: c.line)),
      focusedBorder: OutlineInputBorder(borderRadius: BorderRadius.circular(14), borderSide: BorderSide(color: c.accent, width: 2)),
    ),
  );
}

ThemeData lightTheme() => buildTheme(PrColors.light, Brightness.light);
ThemeData darkTheme() => buildTheme(PrColors.dark, Brightness.dark);
