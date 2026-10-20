<script lang="ts">
  // A calendar invitation in a message: when and where, and Accept, Maybe or
  // Decline. Answers to the user's own invitations and cancellations show
  // what changed in the calendar.
  import { onMount } from "svelte";
  import { api, errorText, type InviteResponse, type InviteView } from "./api";
  import { allDayDate, addDays, fromSecs, sameDay, timeText } from "./calendar";
  import { senderName } from "./format";
  import { toast } from "./toast.svelte";
  import Icon from "./Icon.svelte";

  interface Props {
    /** The message carrying the invitation. */
    id: number;
  }
  let { id }: Props = $props();

  let invite = $state<InviteView | null>(null);
  let error = $state("");
  let busy = $state(false);
  let send = $state(true);

  async function load() {
    try {
      invite = await api.messageInvite(id);
      error = "";
    } catch (err) {
      error = errorText(err);
    }
  }
  onMount(load);

  const event = $derived(invite?.event);
  const first = $derived(event ? (event.allDay ? allDayDate(event.starts) : fromSecs(event.starts)) : new Date());
  const organizer = $derived(event?.organizer ? senderName(event.organizer) : "");
  const current = $derived(invite?.saved?.response ?? null);

  const when = $derived.by(() => {
    if (!event) return "";
    const day = (d: Date) => d.toLocaleDateString(undefined, { weekday: "long", day: "numeric", month: "long" });
    if (event.allDay) {
      const last = addDays(allDayDate(event.ends), -1);
      return sameDay(first, last) ? `${day(first)}, all day` : `${day(first)} to ${day(last)}`;
    }
    const end = fromSecs(event.ends);
    return sameDay(first, end)
      ? `${day(first)}, ${timeText(first)} – ${timeText(end)}`
      : `${day(first)} ${timeText(first)} to ${day(end)} ${timeText(end)}`;
  });

  const answerWords: Record<InviteResponse, string> = {
    accepted: "accepted",
    tentative: "said maybe to",
    declined: "declined",
    needsAction: "hasn't answered",
  };

  async function answer(response: InviteResponse) {
    busy = true;
    try {
      const saved = await api.answerInvite(id, response, send);
      if (invite) invite.saved = saved;
      const sent = send && invite?.method === "request" && organizer;
      if (invite?.method === "publish") toast("Added to your calendar.");
      else toast(sent ? `Answer sent to ${organizer}.` : "Saved in your calendar.");
    } catch (err) {
      toast(errorText(err), "error");
    } finally {
      busy = false;
    }
  }

  async function remove() {
    if (!invite?.saved) return;
    busy = true;
    try {
      await api.deleteEvent(invite.saved.id);
      invite.saved = null;
      toast("Removed from your calendar.");
    } catch (err) {
      toast(errorText(err), "error");
    } finally {
      busy = false;
    }
  }

  const choices: [InviteResponse, string][] = [
    ["accepted", "Accept"],
    ["tentative", "Maybe"],
    ["declined", "Decline"],
  ];
</script>

{#if error}
  <p class="error invite-error">Could not read the invitation: {error}</p>
{:else if invite && event}
  <section class="invite" class:cancelled={invite.method === "cancel"} aria-label="Calendar invitation">
    <div class="date" aria-hidden="true">
      <span class="month">{first.toLocaleDateString(undefined, { month: "short" })}</span>
      <span class="day">{first.getDate()}</span>
    </div>
    <div class="details">
      <h3>{event.title || "(no title)"}</h3>
      <p class="line"><Icon name="clock" size={14} /> {when}</p>
      {#if event.rrule}<p class="line"><Icon name="repeat" size={14} /> Repeats</p>{/if}
      {#if event.location}<p class="line"><Icon name="location" size={14} /> {event.location}</p>{/if}
      {#if organizer && invite.method !== "reply"}<p class="line quiet"><Icon name="contact" size={14} /> From {organizer}</p>{/if}
      {#if invite.conflicts.length && invite.method !== "cancel"}
        <p class="line warn"><Icon name="alert" size={14} /> At the same time as {invite.conflicts.join(", ")}</p>
      {/if}

      {#if invite.method === "request" && invite.canAnswer}
        <div class="answers" role="group" aria-label="Your answer">
          {#each choices as [response, label] (response)}
            <button
              class:chosen={current === response}
              aria-pressed={current === response}
              disabled={busy}
              onclick={() => answer(response)}
            >
              {#if current === response}<Icon name="check" size={14} />{/if}
              {label}
            </button>
          {/each}
        </div>
        {#if organizer}
          <label class="send"><input type="checkbox" bind:checked={send} /> Send my answer to {organizer}</label>
        {/if}
        {#if current}<p class="status quiet">In your calendar.</p>{/if}
      {:else if invite.method === "request"}
        <p class="status quiet">You sent this invitation.</p>
      {:else if invite.method === "publish"}
        {#if invite.saved}
          <p class="status quiet">In your calendar.</p>
        {:else}
          <div class="answers"><button class="primary" disabled={busy} onclick={() => answer("accepted")}><Icon name="plus" size={14} /> Add to calendar</button></div>
        {/if}
      {:else if invite.method === "reply" && invite.answer}
        <p class="status">
          <strong>{invite.answer.name || invite.answer.email}</strong> {answerWords[invite.answer.response]} your invitation.
          {#if invite.saved}<span class="quiet">Noted in your calendar.</span>{/if}
        </p>
      {:else if invite.method === "cancel"}
        <p class="status"><strong>This event was cancelled.</strong></p>
        {#if invite.saved}
          <div class="answers"><button disabled={busy} onclick={remove}><Icon name="trash" size={14} /> Remove from calendar</button></div>
        {/if}
      {/if}
    </div>
  </section>
{/if}

<style>
  .invite {
    display: flex;
    gap: 14px;
    margin: 0 16px 12px;
    padding: 14px 16px;
    border: 1px solid var(--border);
    border-left: 4px solid var(--accent-ink);
    border-radius: var(--radius);
    background: var(--bg-side);
  }
  .invite.cancelled {
    border-left-color: var(--danger);
  }
  .invite.cancelled h3 {
    text-decoration: line-through;
  }
  .invite-error {
    margin: 0 16px 12px;
  }
  .date {
    flex: none;
    width: 52px;
    border-radius: 8px;
    overflow: hidden;
    border: 1px solid var(--border);
    background: var(--bg);
    text-align: center;
    align-self: flex-start;
  }
  .month {
    display: block;
    background: var(--accent);
    color: var(--accent-text);
    font-size: 11px;
    font-weight: 600;
    text-transform: uppercase;
    padding: 2px 0;
  }
  .day {
    display: block;
    font-size: 22px;
    font-weight: 600;
    padding: 2px 0 4px;
  }
  .details {
    min-width: 0;
    flex: 1;
  }
  h3 {
    margin: 0 0 4px;
    font-size: 15px;
  }
  .line {
    display: flex;
    align-items: center;
    gap: 6px;
    margin: 2px 0;
    font-size: 13px;
  }
  .warn {
    color: var(--danger);
  }
  .answers {
    display: flex;
    flex-wrap: wrap;
    gap: 6px;
    margin-top: 10px;
  }
  .answers button {
    padding: 5px 14px;
  }
  .answers button.chosen {
    background: var(--bg-selected);
    border-color: var(--accent-ink);
    color: var(--accent-ink);
    font-weight: 600;
  }
  .send {
    display: flex;
    align-items: center;
    gap: 6px;
    margin-top: 8px;
    font-size: 13px;
    color: var(--text-quiet);
  }
  .send input {
    width: auto;
    margin: 0;
  }
  .status {
    margin: 8px 0 0;
    font-size: 13px;
  }
</style>
