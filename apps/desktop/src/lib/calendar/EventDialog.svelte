<script lang="ts" module>
  const LAST_KEY = "novero.calendar.last";

  /** The calendar the last new event went in, for the next one. */
  export function lastCalendar(): number | null {
    try {
      const id = Number(localStorage.getItem(LAST_KEY));
      return Number.isInteger(id) && id > 0 ? id : null;
    } catch {
      return null;
    }
  }
</script>

<script lang="ts">
  // Making or changing an event: what, when, where, how often and the reminder.
  import { onMount, tick, untrack } from "svelte";
  import { api, errorText, type Calendar, type CalendarAccount, type CalendarEvent } from "../api";
  import {
    REMINDERS,
    addDays,
    allDayDate,
    allDaySecs,
    dateValue,
    fromSecs,
    parseLocal,
    repeatLabel,
    repeatOf,
    repeatRule,
    timeValue,
    toSecs,
    type Repeat,
  } from "./calendar";
  import { toggleMenuUnder } from "../shell/menus.svelte";
  import { toast } from "../shell/toast.svelte";
  import Icon from "../ui/Icon.svelte";

  interface Props {
    event: CalendarEvent;
    calendars: Calendar[];
    /** Online accounts, to group their calendars under. */
    accounts?: CalendarAccount[];
    /** The occurrence that was opened, for "Delete only this one". */
    occurrence?: number | null;
    onClose: () => void;
  }
  let { event: given, calendars, accounts = [], occurrence = null, onClose }: Props = $props();
  // The form edits a copy of the event it opened with.
  const event = untrack(() => $state.snapshot(given)) as CalendarEvent;

  const isNew = event.id === 0;
  const invitation = !!event.organizer;
  const home = $derived(calendars.find((c) => c.id === event.calendar));
  /** In an online calendar that can only be looked at. */
  const readOnly = $derived(!!home?.readOnly);
  /** One occurrence of an online series, kept as its own event. */
  const instance = !!event.series;
  const choices = $derived(calendars.filter((c) => !c.readOnly || c.id === event.calendar));
  const SERVICE = { google: "Google Calendar", microsoft: "Outlook" } as const;
  /** The choices by where they are kept: this computer, then each account. */
  const groups = $derived([
    { label: "On this computer", list: choices.filter((c) => c.source == null) },
    ...accounts.map((a) => ({
      label: `${SERVICE[a.provider]} · ${a.email}`,
      list: choices.filter((c) => c.source === a.id),
    })),
  ].filter((g) => g.list.length));
  const chosenAccount = $derived(accounts.find((a) => a.id === calendars.find((c) => c.id === calendar)?.source));

  // The form works in local dates and times. An all-day event's last day is
  // the day before its end.
  const start0 = event.allDay ? allDayDate(event.starts) : fromSecs(event.starts);
  const end0 = event.allDay ? addDays(allDayDate(event.ends), -1) : fromSecs(event.ends);

  let title = $state(event.title);
  let calendar = $state(event.calendar);
  let allDay = $state(event.allDay);
  let startDate = $state(dateValue(start0));
  let startTime = $state(event.allDay ? "09:00" : timeValue(start0));
  let endDate = $state(dateValue(end0));
  let endTime = $state(event.allDay ? "10:00" : timeValue(end0));
  let location = $state(event.location);
  let notes = $state(event.notes);
  let repeat = $state<Repeat>(repeatOf(event.rrule, start0));
  let reminder = $state<number | null>(event.reminder);
  let error = $state("");
  let saving = $state(false);
  let titleInput = $state<HTMLInputElement>();

  const startDay = $derived(parseLocal(startDate) ?? start0);
  const repeats: Repeat[] = $derived(
    (["none", "daily", "weekdays", "weekly", "monthly", "yearly"] as Repeat[]).concat(
      repeatOf(event.rrule, start0) === "custom" ? ["custom"] : [],
    ),
  );

  /** Keeps the length of the event when its start moves. */
  let lastStart = untrack(() => ({ date: startDate, time: startTime }));
  function startMoved() {
    const before = parseLocal(lastStart.date, lastStart.time);
    const after = parseLocal(startDate, startTime);
    const end = parseLocal(endDate, endTime);
    if (before && after && end) {
      const moved = new Date(end.getTime() + (after.getTime() - before.getTime()));
      endDate = dateValue(moved);
      endTime = timeValue(moved);
    }
    lastStart = { date: startDate, time: startTime };
  }

  function times(): { starts: number; ends: number } | string {
    if (allDay) {
      const first = parseLocal(startDate);
      const last = parseLocal(endDate);
      if (!first || !last) return "Enter the dates.";
      if (last < first) return "The event ends before it starts.";
      return { starts: allDaySecs(first), ends: allDaySecs(addDays(last, 1)) };
    }
    const start = parseLocal(startDate, startTime);
    const end = parseLocal(endDate, endTime);
    if (!start || !end) return "Enter the dates and times.";
    if (end < start) return "The event ends before it starts.";
    return { starts: toSecs(start), ends: toSecs(end) };
  }

  async function save() {
    if (readOnly) return;
    const when = times();
    if (typeof when === "string") {
      error = when;
      return;
    }
    saving = true;
    error = "";
    try {
      await api.saveEvent({
        ...event,
        title: title.trim(),
        calendar,
        allDay,
        // A changed time zone is the computer's; the store fills it in.
        tz: allDay ? null : event.allDay ? null : event.tz,
        starts: when.starts,
        ends: when.ends,
        location: location.trim(),
        notes,
        rrule: repeatRule(repeat, startDay, event.rrule),
        reminder,
      });
      if (isNew) {
        try {
          localStorage.setItem(LAST_KEY, String(calendar));
        } catch {
          // Not remembered, then.
        }
      }
      onClose();
    } catch (err) {
      error = errorText(err);
      saving = false;
    }
  }

  async function removeAll() {
    try {
      await api.deleteEvent(event.id);
      toast(`Deleted "${event.title || "(no title)"}".`);
      onClose();
    } catch (err) {
      error = errorText(err);
    }
  }

  async function removeOne() {
    if (occurrence == null) return;
    try {
      await api.skipOccurrence(event.id, occurrence);
      toast("Deleted this time only.");
      onClose();
    } catch (err) {
      error = errorText(err);
    }
  }

  function deleteClicked(e: MouseEvent) {
    if ((!event.rrule && !instance) || occurrence == null) {
      removeAll();
      return;
    }
    toggleMenuUnder(e.currentTarget as Element, [
      { label: "Delete only this time", icon: "trash", run: removeOne },
      { label: "Delete every time", icon: "trash", danger: true, run: removeAll },
    ]);
  }

  function keydown(e: KeyboardEvent) {
    if (e.key === "Escape") {
      e.preventDefault();
      e.stopPropagation();
      onClose();
    } else if (e.key === "Enter" && e.ctrlKey) {
      e.preventDefault();
      save();
    }
  }

  onMount(async () => {
    await tick();
    titleInput?.focus();
  });
</script>

<div class="backdrop" onkeydown={keydown} role="presentation">
  <div class="dialog" role="dialog" aria-modal="true" aria-labelledby="event-title">
    <h2 id="event-title" class="sr">{isNew ? "New event" : "Event"}</h2>
    <input
      class="title"
      placeholder="Add a title"
      aria-label="Title"
      bind:value={title}
      bind:this={titleInput}
      readonly={readOnly}
      onkeydown={(e) => e.key === "Enter" && !e.ctrlKey && (e.preventDefault(), save())}
    />
    {#if readOnly}
      <p class="note">{home?.name} can only be looked at, not changed.</p>
    {:else if invitation && home?.source != null}
      <p class="note">From {event.organizer}.</p>
    {:else if invitation}
      <p class="note">From {event.organizer}. Changes here are only in your calendar.</p>
    {/if}
    <fieldset class="fields" disabled={readOnly}>

    <div class="row">
      <Icon name="clock" />
      <div class="when">
        <span class="quiet">Starts</span>
        <input type="date" aria-label="Starts on" bind:value={startDate} onchange={startMoved} />
        {#if !allDay}<input type="time" aria-label="Starts at" step="300" bind:value={startTime} onchange={startMoved} />{:else}<span></span>{/if}
        <span class="quiet">Ends</span>
        <input type="date" aria-label="Ends on" bind:value={endDate} />
        {#if !allDay}<input type="time" aria-label="Ends at" step="300" bind:value={endTime} />{:else}<span></span>{/if}
        <label class="check"><input type="checkbox" bind:checked={allDay} /> All day</label>
      </div>
    </div>

    {#if instance}
      <p class="hint quiet">Changes apply to this time only.</p>
    {:else}
      <div class="row">
        <Icon name="repeat" />
        <select aria-label="Repeat" bind:value={repeat}>
          {#each repeats as r (r)}
            <option value={r}>{repeatLabel(r, startDay)}</option>
          {/each}
        </select>
      </div>
    {/if}
    {#if event.rrule && !isNew}
      <p class="hint quiet">Changes apply to every time this event repeats.</p>
    {/if}

    <div class="row">
      <Icon name="bell" />
      <select aria-label="Reminder" bind:value={reminder}>
        {#each REMINDERS as [minutes, label] (minutes)}
          <option value={minutes}>{label}</option>
        {/each}
      </select>
    </div>

    <div class="row">
      <Icon name="location" />
      <input placeholder="Add a location" aria-label="Location" bind:value={location} />
    </div>

    <div class="row">
      <Icon name="calendar" />
      <select aria-label="Calendar" bind:value={calendar}>
        {#if groups.length > 1}
          {#each groups as g (g.label)}
            <optgroup label={g.label}>
              {#each g.list as c (c.id)}
                <option value={c.id}>{c.name}</option>
              {/each}
            </optgroup>
          {/each}
        {:else}
          {#each choices as c (c.id)}
            <option value={c.id}>{c.name}</option>
          {/each}
        {/if}
      </select>
      <span class="swatch" style:background={calendars.find((c) => c.id === calendar)?.color}></span>
    </div>
    {#if chosenAccount && !readOnly}
      <p class="hint quiet">Saved to {SERVICE[chosenAccount.provider]} for {chosenAccount.email}.</p>
    {/if}

    <textarea placeholder="Add notes" aria-label="Notes" rows="4" bind:value={notes}></textarea>
    </fieldset>

    {#if error}<p class="error">{error}</p>{/if}

    <div class="buttons">
      {#if !isNew && !readOnly}
        <button class="danger" onclick={deleteClicked}><Icon name="trash" /> Delete</button>
      {/if}
      <span class="grow"></span>
      {#if readOnly}
        <button onclick={onClose}>Close</button>
      {:else}
        <button onclick={onClose}>Cancel</button>
        <button class="primary" disabled={saving} onclick={save} title="Save (Ctrl+Enter)">Save</button>
      {/if}
    </div>
  </div>
</div>

<style>
  .backdrop {
    position: fixed;
    inset: 0;
    background: rgba(0, 0, 0, 0.35);
    display: grid;
    place-items: center;
    z-index: 16;
  }
  .dialog {
    background: var(--bg);
    border: 1px solid var(--border);
    border-radius: 12px;
    width: min(520px, calc(100vw - 32px));
    max-height: calc(100vh - 48px);
    overflow-y: auto;
    padding: 20px 24px;
    box-shadow: 0 12px 40px rgba(0, 0, 0, 0.25);
    display: grid;
    gap: 12px;
  }
  .fields {
    display: contents;
  }
  .sr {
    position: absolute;
    width: 1px;
    height: 1px;
    overflow: hidden;
    clip: rect(0 0 0 0);
  }
  .title {
    font-size: 18px;
    font-weight: 600;
    border: none;
    border-bottom: 2px solid var(--border);
    border-radius: 0;
    padding: 4px 0 6px;
    background: none;
  }
  .title:focus {
    outline: none;
    border-bottom-color: var(--accent-ink);
  }
  .row {
    display: flex;
    align-items: flex-start;
    gap: 12px;
    color: var(--text-quiet);
  }
  .row > :global(svg.icon) {
    margin-top: 8px;
    flex: none;
  }
  .row > input,
  .row > select {
    flex: 1;
    color: var(--text);
  }
  .when {
    flex: 1;
    display: grid;
    grid-template-columns: auto minmax(0, 1fr) 120px;
    align-items: center;
    gap: 6px 8px;
    color: var(--text);
  }
  .when .check {
    grid-column: 2 / -1;
  }
  .check {
    display: flex;
    align-items: center;
    gap: 6px;
  }
  .check input {
    width: auto;
  }
  .hint {
    margin: -6px 0 0 28px;
    font-size: 12px;
  }
  .swatch {
    width: 14px;
    height: 14px;
    border-radius: 4px;
    margin-top: 9px;
    flex: none;
  }
  textarea {
    resize: vertical;
    font: inherit;
    color: var(--text);
    background: var(--bg);
    border: 1px solid var(--border);
    border-radius: 6px;
    padding: 8px 10px;
  }
  .note {
    margin: 0;
  }
  .buttons {
    display: flex;
    gap: 8px;
  }
  .grow {
    flex: 1;
  }
  .danger {
    color: var(--danger);
  }
</style>
