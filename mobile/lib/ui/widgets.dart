import 'package:flutter/material.dart';
import 'package:flutter/services.dart';

import '../app/settings.dart';
import '../app/theme.dart';

/// A small label above a block, like the web pages' "kicker".
class Kicker extends StatelessWidget {
  const Kicker(this.text, {super.key});
  final String text;
  @override
  Widget build(BuildContext context) => Text(text.toUpperCase(), style: TextStyle(color: context.pr.dim, fontSize: 12, fontWeight: FontWeight.w700, letterSpacing: 1.4));
}

class Card2 extends StatelessWidget {
  const Card2({super.key, required this.child, this.padding = const EdgeInsets.all(18)});
  final Widget child;
  final EdgeInsetsGeometry padding;
  @override
  Widget build(BuildContext context) => Container(
        width: double.infinity,
        padding: padding,
        decoration: BoxDecoration(color: context.pr.surface, borderRadius: BorderRadius.circular(20), border: Border.all(color: context.pr.line)),
        child: child,
      );
}

/// A round control button with a label under it, large enough to hit without looking.
class PadButton extends StatelessWidget {
  const PadButton({super.key, required this.icon, required this.label, required this.onPressed, this.filled = false, this.size = 64});
  final IconData icon;
  final String label;
  final VoidCallback? onPressed;
  final bool filled;
  final double size;

  @override
  Widget build(BuildContext context) {
    final c = context.pr;
    return Semantics(
      button: true,
      label: label,
      child: InkResponse(
        onTap: onPressed,
        radius: size * .7,
        child: Column(mainAxisSize: MainAxisSize.min, children: [
          Container(
            width: size,
            height: size,
            decoration: BoxDecoration(shape: BoxShape.circle, color: onPressed == null ? c.surface : (filled ? c.accent : c.surface2)),
            child: Icon(icon, size: size * .46, color: filled ? c.onAccent : (onPressed == null ? c.dim : c.ink)),
          ),
          const SizedBox(height: 6),
          Text(label, style: TextStyle(color: c.dim, fontSize: 12, fontWeight: FontWeight.w600)),
        ]),
      ),
    );
  }
}

/// A short buzz on button presses, if the user left vibration on.
void buzz(AppSettings s) {
  if (s.vibrate) HapticFeedback.selectionClick();
}

String clock(int ms) {
  final s = (ms / 1000).floor().clamp(0, 359999);
  final h = s ~/ 3600, m = (s % 3600) ~/ 60, sec = s % 60;
  String two(int n) => n.toString().padLeft(2, '0');
  return h > 0 ? '$h:${two(m)}:${two(sec)}' : '$m:${two(sec)}';
}
