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

# A folder of HTML mail for checking the reader: remote images, links,
# inline images and attachments.
news = maildir(os.path.join(root, ".Newsletters"))

m = EmailMessage()
m["From"] = "Weekly News <news@example.test>"
m["To"] = "Alice <alice@example.test>"
m["Subject"] = "Your weekly digest"
m["Date"] = format_datetime(base + timedelta(days=200))
m["Message-ID"] = "<news-1@example.test>"
m.set_content("This week's digest. Read it at https://example.test/digest")
m.add_alternative(
    """<html><body>
<h1 style="color:#2563eb">This week's digest</h1>
<p>Three stories you might like. <a href="https://example.test/digest">Read online</a>.</p>
<img src="https://tracker.example.test/open.gif" width="1" height="1">
<img src="https://cdn.example.test/banner.png" alt="Banner">
<script>alert('should never run')</script>
<p onclick="alert(1)">Thanks for reading.</p>
</body></html>""",
    subtype="html",
)
write(news, 2001, m, seen=False)

m = EmailMessage()
m["From"] = "Billing <billing@example.test>"
m["To"] = "Alice <alice@example.test>"
m["Subject"] = "Invoice for October"
m["Date"] = format_datetime(base + timedelta(days=201))
m["Message-ID"] = "<news-2@example.test>"
m.set_content("Your invoice is attached.")
m.add_attachment(b"%PDF-1.4 test", maintype="application", subtype="pdf", filename="invoice-october.pdf")
write(news, 2002, m, seen=False)
