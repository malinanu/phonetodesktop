import 'package:flutter/material.dart';
import 'package:url_launcher/url_launcher.dart';

import '../app/files_url.dart';
import '../app/settings.dart';
import '../app/theme.dart';
import 'widgets.dart';

/// Send files runs in the phone's browser: receiving big files streams to storage there, which an in-app web view
/// cannot do. The server address comes from the build or from Settings.
const buildFilesUrl = String.fromEnvironment('FILES_URL');

class FilesScreen extends StatelessWidget {
  const FilesScreen({super.key, required this.settings, this.onOpenSettings, this.open});
  final AppSettings settings;
  final VoidCallback? onOpenSettings;

  /// Opens a URL (replaced in tests).
  final Future<bool> Function(Uri url)? open;

  @override
  Widget build(BuildContext context) {
    return ListenableBuilder(
      listenable: settings,
      builder: (context, _) {
        final c = context.pr;
        final url = FilesUrl.resolve(settings.filesUrl, buildFilesUrl);
        return ListView(padding: const EdgeInsets.fromLTRB(24, 16, 24, 24), children: [
          const Kicker('Files'),
          const SizedBox(height: 8),
          const Text('Send files.', style: TextStyle(fontSize: 38, fontWeight: FontWeight.w700, height: .95)),
          const SizedBox(height: 14),
          Text(
            url == null
                ? 'Send files of any size to any device, privately. First add your file server address in Settings.'
                : 'Send files of any size to any device, privately. They open in your browser, which is what lets big files save straight to storage.',
            style: TextStyle(color: c.dim, fontSize: 17, height: 1.35),
          ),
          const SizedBox(height: 18),
          for (final (i, t) in ['Open it here and on the other device', 'Share the room link or QR code', 'Pick files; they go straight between devices'].indexed)
            Padding(
              padding: const EdgeInsets.symmetric(vertical: 10),
              child: Row(children: [
                SizedBox(width: 38, child: Text('${i + 1}', style: TextStyle(color: c.accentText, fontSize: 26, fontWeight: FontWeight.w700))),
                Expanded(child: Text(t, style: const TextStyle(fontSize: 16))),
              ]),
            ),
          const SizedBox(height: 18),
          FilledButton(
            onPressed: () async {
              if (url == null) {
                onOpenSettings?.call();
                return;
              }
              final ok = await (open ?? (u) => launchUrl(u, mode: LaunchMode.externalApplication))(Uri.parse(url));
              if (!ok && context.mounted) ScaffoldMessenger.of(context).showSnackBar(const SnackBar(content: Text('No browser found')));
            },
            child: Text(url == null ? 'Open Settings' : 'Open Send files'),
          ),
          const SizedBox(height: 6),
          TextButton(onPressed: onOpenSettings, child: const Text('Change server address')),
        ]);
      },
    );
  }
}
