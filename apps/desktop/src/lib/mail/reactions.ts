import type { MessageReaction } from "../api";
import { senderName } from "../format";

/** Reactions with the same emoji together, in the order first given. */
export interface ReactionGroup {
  emoji: string;
  names: string[];
}

export function groupReactions(reactions: MessageReaction[]): ReactionGroup[] {
  const groups: ReactionGroup[] = [];
  for (const r of reactions) {
    const name = senderName(r.from);
    const group = groups.find((g) => g.emoji === r.emoji);
    if (!group) groups.push({ emoji: r.emoji, names: [name] });
    else if (!group.names.includes(name)) group.names.push(name);
  }
  return groups;
}

/** "Ann and Bob reacted 👍" lines, for a tooltip. */
export function reactionText(groups: ReactionGroup[]): string {
  return groups
    .map((g) => {
      const names =
        g.names.length > 1
          ? `${g.names.slice(0, -1).join(", ")} and ${g.names.at(-1)}`
          : g.names[0];
      return `${names} reacted ${g.emoji}`;
    })
    .join("\n");
}
