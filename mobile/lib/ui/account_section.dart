import 'package:flutter/material.dart';

import '../app/account.dart';
import '../app/connection.dart';
import '../app/theme.dart';

/// Settings > Account (optional). Signed out: email, code. Signed in: find my computers, sign out, delete.
class AccountSection extends StatefulWidget {
  const AccountSection({super.key, required this.account, required this.link, this.defaultUrl = 'https://lanyardfileshare.3logiq.com/account'});
  final AccountController account;
  final ConnectionController link;
  final String defaultUrl;

  @override
  State<AccountSection> createState() => _AccountSectionState();
}

class _AccountSectionState extends State<AccountSection> {
  late final TextEditingController _url = TextEditingController(text: widget.defaultUrl);
  final _email = TextEditingController(), _code = TextEditingController();
  String? _note;

  AccountController get a => widget.account;

  @override
  void dispose() {
    _url.dispose();
    _email.dispose();
    _code.dispose();
    super.dispose();
  }

  Future<void> _find() async {
    setState(() => _note = 'Looking…');
    final list = await a.computers();
    if (list == null) return setState(() => _note = null);
    final r = await widget.link.addAccountPcs([for (final d in list) (pcId: d.pcId!, name: d.name, fingerprint: d.fingerprint!)]);
    if (!mounted) return;
    setState(() {
      _note = list.isEmpty
          ? 'No computer has joined this account yet. On the computer, open Lanyard, Settings, Account.'
          : r.found.isEmpty
          ? 'None of your computers are on this Wi-Fi right now (${r.missing.join(', ')}).'
          : 'Found: ${r.found.join(', ')}.${r.missing.isEmpty ? '' : ' Not on this Wi-Fi: ${r.missing.join(', ')}.'}';
    });
  }

  Future<bool> _confirm(String title, String body, String yes) async =>
      await showDialog<bool>(
        context: context,
        builder: (_) => AlertDialog(
          title: Text(title),
          content: Text(body),
          actions: [
            TextButton(onPressed: () => Navigator.pop(context, false), child: const Text('Cancel')),
            TextButton(onPressed: () => Navigator.pop(context, true), child: Text(yes)),
          ],
        ),
      ) ??
      false;

  Future<void> _signOut() async {
    if (!await _confirm('Sign out?', 'Computers you found through the account will be forgotten on this phone.', 'Sign out')) {
      return;
    }
    await a.signOut();
    await widget.link.forgetAccountPcs();
  }

  Future<void> _delete() async {
    if (!await _confirm('Delete the account?', 'This removes your email and every device from the account. It cannot be undone.', 'Delete')) {
      return;
    }
    if (await a.deleteAccount()) await widget.link.forgetAccountPcs();
  }

  @override
  Widget build(BuildContext context) {
    final c = context.pr;
    return ListenableBuilder(
      listenable: a,
      builder: (context, _) {
        final s = a.session;
        final err = a.error;
        final errorText = err == null
            ? null
            : Padding(
                padding: const EdgeInsets.only(top: 8),
                child: Text(err, style: TextStyle(color: c.bad)),
              );
        if (s == null) {
          final waiting = a.waitingForCodeAt;
          return Column(
            crossAxisAlignment: CrossAxisAlignment.stretch,
            children: [
              Text(
                'Not needed. With an account, your computers show up here without scanning a code. Controlling a computer on your Wi-Fi never needs the internet.',
                style: TextStyle(color: c.dim, fontSize: 14),
              ),
              const SizedBox(height: 10),
              if (waiting == null) ...[
                TextField(
                  controller: _url,
                  keyboardType: TextInputType.url,
                  autocorrect: false,
                  decoration: const InputDecoration(labelText: 'Account service'),
                ),
                const SizedBox(height: 8),
                TextField(
                  controller: _email,
                  keyboardType: TextInputType.emailAddress,
                  autocorrect: false,
                  decoration: const InputDecoration(labelText: 'Email address'),
                ),
                const SizedBox(height: 10),
                FilledButton(onPressed: a.busy ? null : () => a.requestCode(_url.text, _email.text), child: Text(a.busy ? 'Sending…' : 'Email me a code')),
              ] else ...[
                Text('We sent a code to $waiting.', style: const TextStyle(fontWeight: FontWeight.w600)),
                const SizedBox(height: 8),
                TextField(
                  controller: _code,
                  keyboardType: TextInputType.number,
                  autocorrect: false,
                  decoration: const InputDecoration(labelText: 'Code from the email'),
                ),
                const SizedBox(height: 10),
                FilledButton(onPressed: a.busy ? null : () => a.signIn(_code.text), child: Text(a.busy ? 'Checking…' : 'Sign in')),
                TextButton(onPressed: a.busy ? null : a.cancelCode, child: const Text('Use a different email')),
              ],
              ?errorText,
            ],
          );
        }
        return Column(
          crossAxisAlignment: CrossAxisAlignment.stretch,
          children: [
            Text(s.email, style: const TextStyle(fontSize: 16, fontWeight: FontWeight.w700)),
            Text(
              s.expired
                  ? 'Your sign-in ended. Sign in again to find new computers; the ones you already have keep working.'
                  : 'Signed in. This phone is listed under “My devices”.',
              style: TextStyle(color: c.dim, fontSize: 14),
            ),
            const SizedBox(height: 10),
            if (s.expired)
              OutlinedButton(
                onPressed: () async {
                  await a.signOut();
                },
                child: const Text('Sign in again'),
              )
            else
              FilledButton.icon(onPressed: a.busy ? null : _find, icon: const Icon(Icons.search_rounded), label: const Text('Find my computers')),
            if (_note != null)
              Padding(
                padding: const EdgeInsets.only(top: 8),
                child: Text(_note!, style: TextStyle(color: c.dim)),
              ),
            ?errorText,
            const SizedBox(height: 6),
            Row(
              children: [
                TextButton(onPressed: a.busy ? null : _signOut, child: const Text('Sign out')),
                const Spacer(),
                TextButton(
                  onPressed: a.busy ? null : _delete,
                  child: Text('Delete account', style: TextStyle(color: c.bad)),
                ),
              ],
            ),
          ],
        );
      },
    );
  }
}
