// Names, icons and wording for mail categories, and accepting suggestions.
import { api, type Category, type CategoryReason, type SenderGroup } from "../api";
import type { IconName } from "../ui/Icon.svelte";

export const categories: { id: Category; label: string; icon: IconName; about: string }[] = [
  { id: "personal", label: "Personal", icon: "contact", about: "Mail from people you know or that looks hand written." },
  { id: "updates", label: "Updates", icon: "bell", about: "Receipts, orders, bills, sign-in codes and notifications." },
  { id: "social", label: "Social", icon: "contacts", about: "Social networks and community sites." },
  { id: "forums", label: "Forums", icon: "forums", about: "Discussion groups and mailing lists." },
  { id: "newsletters", label: "Newsletters", icon: "newsletters", about: "Newsletters and other mail you subscribed to." },
  { id: "promotions", label: "Promotions", icon: "promotions", about: "Sales, offers and marketing." },
  { id: "spam", label: "Spam", icon: "junk", about: "Unwanted or suspicious mail. New mail from senders you mark as spam goes to Junk." },
];

/** The categories that can be filed into a folder of your own (mail/category_folders.rs). */
export const filedCategories: Category[] = ["personal", "updates", "social", "forums", "newsletters", "promotions"];

export const categoryOf = Object.fromEntries(categories.map((c) => [c.id, c])) as Record<
  Category,
  (typeof categories)[number]
>;

const reasonText: Record<CategoryReason, string> = {
  spamFilter: "your provider's spam filter flagged it",
  spamWords: "the subject reads like spam",
  nameMismatch: "the sender's name shows a different address",
  contact: "you have written to them",
  replied: "you replied",
  ownAddress: "it's from your own address",
  personLike: "no mailing list headers",
  socialSite: "it's from a social network",
  discussion: "it's from a discussion group",
  mailingList: "it came through a mailing list",
  serviceSite: "it's from a service that sends notifications",
  automated: "sent automatically",
  noReply: "sent from a no-reply or notifications address",
  updateWords: "the subject is about an order, account or payment",
  promoWords: "the subject is about a sale or offer",
  promoSender: "sent from a deals or marketing address",
  newsletterSender: "sent from a newsletter address",
  newsWords: "the subject reads like a newsletter",
  unsubscribe: "it has an unsubscribe link",
  bulkMail: "sent in bulk",
};

/** "Suggested because it has an unsubscribe link and sent in bulk." */
export function why(reasons: CategoryReason[], sure: boolean): string {
  if (!reasons.length) return sure ? "" : "Not much to go on.";
  const parts = reasons.map((r) => reasonText[r]);
  const list = parts.length > 1 ? `${parts.slice(0, -1).join(", ")} and ${parts.at(-1)}` : parts[0];
  return list.charAt(0).toUpperCase() + list.slice(1) + ".";
}

/** A group's key, unique within a report. */
export const groupKey = (g: SenderGroup) => `${g.email}\n${g.category}`;

/** Confirms the suggested category of groups. A sender whose mail is all in
 *  one group gets the category as a whole, so new mail from them follows it;
 *  otherwise just the group's messages are confirmed. */
export async function accept(groups: SenderGroup[], all: SenderGroup[]) {
  const groupsPerSender = new Map<string, number>();
  for (const g of all) groupsPerSender.set(g.email, (groupsPerSender.get(g.email) ?? 0) + 1);
  for (const g of groups) {
    if (g.confirmed) continue;
    if (g.email && groupsPerSender.get(g.email) === 1) await api.setSenderCategory(g.email, g.category);
    else await api.setMessageCategory(g.ids, g.category);
  }
}

/** "Ann Lee" or the address when there's no name. */
export function senderLabel(g: { name: string | null; email: string }): string {
  return g.name?.trim() || g.email || "(no sender)";
}
