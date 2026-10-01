"""Sending the sign-in code."""
from __future__ import annotations

import logging
import smtplib
from email.message import EmailMessage

log = logging.getLogger("account.mail")


class Mailer:
    def send_code(self, email: str, code: str, minutes: int) -> None:  # pragma: no cover - interface
        raise NotImplementedError


def _message(sender: str, email: str, code: str, minutes: int) -> EmailMessage:
    m = EmailMessage()
    m["From"], m["To"], m["Subject"] = sender, email, "Your Phone Remote sign-in code"
    m.set_content(
        f"Your Phone Remote sign-in code is:\n\n    {code}\n\n"
        f"It works once and expires in {minutes} minutes. If you did not ask for it, ignore this email: nothing happens "
        "unless the code is entered."
    )
    return m


class SmtpMailer(Mailer):
    def __init__(self, host: str, port: int, user: str, password: str, sender: str, starttls: bool = True) -> None:
        self.host, self.port, self.user, self.password, self.sender, self.starttls = host, port, user, password, sender, starttls

    def send_code(self, email: str, code: str, minutes: int) -> None:
        with smtplib.SMTP(self.host, self.port, timeout=15) as s:
            if self.starttls:
                s.starttls()
            if self.user:
                s.login(self.user, self.password)
            s.send_message(_message(self.sender, email, code, minutes))


class LogMailer(Mailer):
    """Development only: the code goes to the log, nothing is sent."""

    def send_code(self, email: str, code: str, minutes: int) -> None:
        log.warning("sign-in code for %s: %s", email, code)


class MemoryMailer(Mailer):
    """Tests: remembers what would have been sent."""

    def __init__(self) -> None:
        self.sent: list[tuple[str, str]] = []

    def send_code(self, email: str, code: str, minutes: int) -> None:
        self.sent.append((email, code))
