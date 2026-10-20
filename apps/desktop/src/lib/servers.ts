import type { Endpoint, Security } from "./api";

/** The usual port for each kind of security. */
export const imapPorts: Record<Security, number> = { implicitTls: 993, startTls: 143, insecure: 143 };
export const smtpPorts: Record<Security, number> = { implicitTls: 465, startTls: 587, insecure: 25 };

export const securityChoices: { value: Security; label: string }[] = [
  { value: "implicitTls", label: "SSL/TLS" },
  { value: "startTls", label: "STARTTLS" },
  { value: "insecure", label: "None (unsafe)" },
];

/** After the security changes: keeps a custom port, swaps a standard one. */
export function portFor(endpoint: Endpoint, ports: Record<Security, number>): number {
  return Object.values(ports).includes(endpoint.port) ? ports[endpoint.security] : endpoint.port;
}

/** Errors that mean the server settings are probably wrong. */
export const serverProblem = /could not reach|timed out|secure connection|STARTTLS/i;
