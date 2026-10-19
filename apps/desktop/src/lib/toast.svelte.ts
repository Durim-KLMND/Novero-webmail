// Short messages in the corner of the window: what just happened, or what went wrong.

export type ToastKind = "info" | "error";

export interface Toast {
  id: number;
  text: string;
  kind: ToastKind;
}

/** Confirmations fade by themselves; errors stay until dismissed. */
const INFO_FOR = 4000;
/** Older toasts give way when this many are showing. */
const MAX = 4;

let next = 1;
export const toasts = $state<Toast[]>([]);

export function dismiss(id: number) {
  const i = toasts.findIndex((t) => t.id === id);
  if (i >= 0) toasts.splice(i, 1);
}

export function toast(text: string, kind: ToastKind = "info"): number {
  // The same text again (say, repeated sync errors) shows once.
  const same = toasts.find((t) => t.text === text && t.kind === kind);
  if (same) dismiss(same.id);
  const id = next++;
  toasts.push({ id, text, kind });
  while (toasts.length > MAX) toasts.shift();
  if (kind === "info") setTimeout(() => dismiss(id), INFO_FOR);
  return id;
}
