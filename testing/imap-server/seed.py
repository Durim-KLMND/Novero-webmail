"""Write a deterministic test mailbox: 150 messages in INBOX, 5 in Sent."""

import os
import sys
from email.message import EmailMessage
from email.utils import format_datetime
from datetime import datetime, timedelta, timezone

root = sys.argv[1]
base = datetime(2026, 1, 1, 9, 0, tzinfo=timezone.utc)


def maildir(path):
    for sub in ("cur", "new", "tmp"):
        os.makedirs(os.path.join(path, sub), exist_ok=True)
    return path


def write(folder, n, msg, seen):
    flags = ":2,S" if seen else ":2,"
    name = f"{1767225600 + n}.M{n}P1.seed{flags}"
    with open(os.path.join(folder, "cur", name), "wb") as f:
        f.write(bytes(msg))


inbox = maildir(root)
for i in range(1, 151):
    m = EmailMessage()
    m["From"] = f"Sender {i % 7} <sender{i % 7}@example.test>"
    m["To"] = "Alice <alice@example.test>"
    m["Subject"] = f"Test message {i:03d}"
    m["Date"] = format_datetime(base + timedelta(hours=i))
    m["Message-ID"] = f"<seed-{i}@example.test>"
    m.set_content(f"Body of test message {i}.\n")
    write(inbox, i, m, seen=i % 3 == 0)

sent = maildir(os.path.join(root, ".Sent"))
for i in range(1, 6):
    m = EmailMessage()
    m["From"] = "Alice <alice@example.test>"
    m["To"] = f"Bob <bob{i}@example.test>"
    m["Subject"] = f"Sent message {i}"
    m["Date"] = format_datetime(base + timedelta(days=i))
    m["Message-ID"] = f"<sent-{i}@example.test>"
    m.set_content("Sent body.\n")
    write(sent, 1000 + i, m, seen=True)
