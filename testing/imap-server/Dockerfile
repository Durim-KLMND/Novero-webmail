# Dovecot IMAP server for integration tests. Not for production use.
FROM ubuntu:24.04

RUN apt-get update \
    && DEBIAN_FRONTEND=noninteractive apt-get install -y --no-install-recommends dovecot-imapd python3 \
    && rm -rf /var/lib/apt/lists/*

COPY dovecot.conf /etc/dovecot/dovecot.conf
COPY seed.py /usr/local/bin/seed.py
COPY oauth_mock.py /usr/local/bin/oauth_mock.py
COPY dovecot-oauth2.conf.ext /etc/dovecot/dovecot-oauth2.conf.ext

RUN useradd --system --home /srv/mail --uid 5000 vmail \
    && python3 /usr/local/bin/seed.py /srv/mail/alice/Maildir \
    && chown -R vmail:vmail /srv/mail

EXPOSE 143 8080
# The fake OAuth2 provider runs beside Dovecot, which asks it about tokens.
CMD ["sh", "-c", "python3 /usr/local/bin/oauth_mock.py & exec dovecot -F"]
