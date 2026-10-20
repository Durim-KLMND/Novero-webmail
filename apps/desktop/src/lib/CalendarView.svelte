<script lang="ts">
  // The calendar: a month, week or day at a time, with the list of calendars
  // and a small month to jump around on the left.
  import { onMount, tick } from "svelte";
  import {
    api,
    errorText,
    onCalendarChanged,
    type Calendar,
    type CalendarAccount,
    type CalendarAccounts,
    type CalendarEvent,
    type OAuthProvider,
    type Occurrence,
  } from "./api";
  import {
    COLORS,
    addDays,
    allDaySecs,
    columns,
    onDay,
    sameDay,
    span,
    startOfDay,
    startOfMonth,
    step,
    timeText,
    toSecs,
    viewDays,
    viewTitle,
    type CalendarMode,
  } from "./calendar";
  import { items, openMenu, toggleMenuUnder, type MenuItem } from "./menus.svelte";
  import { toast } from "./toast.svelte";
  import EventDialog from "./EventDialog.svelte";
  import Icon from "./Icon.svelte";

  interface Props {
    /** Shown; keyboard shortcuts only work then. */
    active: boolean;
    /** A dialog of the window is open, so shortcuts stay off. */
    blocked?: boolean;
  }
  let { active, blocked = false }: Props = $props();

  const MODE_KEY = "novero.calendar.mode";
  const HOUR_PX = 48;

  function savedMode(): CalendarMode {
    try {
      const m = localStorage.getItem(MODE_KEY);
      if (m === "month" || m === "week" || m === "day") return m;
    } catch {
      // Storage may be unavailable; the week is the default.
    }
    return "week";
  }

  let mode = $state<CalendarMode>(savedMode());
  let anchor = $state(startOfDay(new Date()));
  let mini = $state(startOfMonth(new Date()));
  let now = $state(new Date());
  let calendars = $state<Calendar[]>([]);
  let occurrences = $state<Occurrence[]>([]);
  let error = $state("");
  let editing = $state<{ event: CalendarEvent; occurrence: number | null } | null>(null);
  let addingCalendar = $state(false);
  let newCalendarName = $state("");
  let renaming = $state<number | null>(null);
  let renameText = $state("");
  let online = $state<CalendarAccounts>({ accounts: [], syncing: false, providers: [] });
  /** Waiting for the browser sign-in of this provider. */
  let connecting = $state<OAuthProvider | null>(null);

  const SERVICE: Record<OAuthProvider, string> = { google: "Google Calendar", microsoft: "Outlook" };
  const localCalendars = $derived(calendars.filter((c) => c.source == null));
  /** Calendars events can be added to or moved to. */
  const writable = $derived(calendars.filter((c) => !c.readOnly));

  const days = $derived(viewDays(mode, anchor));
  const title = $derived(viewTitle(mode, anchor));
  const colorOf = $derived(new Map(calendars.map((c) => [c.id, c.color])));
  const miniDays = $derived(viewDays("month", mini));
  const weekdayNames = $derived(
    viewDays("week", new Date()).map((d) => d.toLocaleDateString(undefined, { weekday: "short" })),
  );

  /** Events of 24 hours or more go in the all-day row of the week view. */
  const long = (o: Occurrence) => o.allDay || o.ends - o.starts >= 86_400;

  let loadCount = 0;
  async function load() {
    const first = days[0];
    const last = addDays(days[days.length - 1], 1);
    // A day either side catches all-day events, which are kept in UTC.
    const from = toSecs(first) - 86_400;
    const to = toSecs(last) + 86_400;
    const mine = ++loadCount;
    try {
      const [cals, found, accounts] = await Promise.all([
        api.listCalendars(),
        api.calendarOccurrences(from, to),
        api.listCalendarAccounts(),
      ]);
      if (mine !== loadCount) return;
      calendars = cals;
      online = accounts;
      occurrences = found;
      error = "";
    } catch (err) {
      error = errorText(err);
    }
  }

  $effect(() => {
    void days;
    load();
  });

  // Opening the calendar brings online calendars up to date.
  $effect(() => {
    if (active) api.syncCalendars().catch(() => {});
  });

  $effect(() => {
    try {
      localStorage.setItem(MODE_KEY, mode);
    } catch {
      // Not remembered, then.
    }
  });

  onMount(() => {
    const unlisten = onCalendarChanged(load);
    const clock = setInterval(() => (now = new Date()), 60_000);
    return () => {
      unlisten.then((f) => f());
      clearInterval(clock);
    };
  });

  /** The hours open on the working day: 8 o'clock at the top. */
  function morning(node: HTMLElement) {
    requestAnimationFrame(() => (node.scrollTop = HOUR_PX * 8 - 8));
  }

  function go(by: number) {
    anchor = step(mode, anchor, by);
    mini = startOfMonth(anchor);
  }

  function today() {
    anchor = startOfDay(new Date());
    mini = startOfMonth(anchor);
  }

  function show(day: Date, as: CalendarMode = "day") {
    anchor = startOfDay(day);
    mini = startOfMonth(anchor);
    mode = as;
  }

  function defaultCalendar(): number {
    return (writable.find((c) => c.visible) ?? writable[0] ?? calendars[0])?.id ?? 1;
  }

  /** A new event at `start`, or at the next hour of the shown day. */
  export function newEvent(start?: Date, allDay = false) {
    if (!calendars.length) return;
    let begin = start;
    if (!begin) {
      const base = sameDay(anchor, new Date()) || mode === "month" ? new Date() : anchor;
      begin = new Date(base);
      if (sameDay(base, new Date())) begin.setHours(begin.getHours() + 1, 0, 0, 0);
      else begin.setHours(9, 0, 0, 0);
    }
    const starts = allDay ? allDaySecs(begin) : toSecs(begin);
    editing = {
      occurrence: null,
      event: {
        id: 0,
        calendar: defaultCalendar(),
        uid: "",
        title: "",
        location: "",
        notes: "",
        starts,
        ends: allDay ? starts + 86_400 : starts + 3600,
        allDay,
        tz: null,
        rrule: null,
        exdates: [],
        organizer: null,
        attendees: [],
        response: null,
        reminder: allDay ? null : 15,
        sequence: 0,
        cancelled: false,
      },
    };
  }

  async function open(o: Occurrence) {
    try {
      const event = await api.getEvent(o.event);
      if (event) editing = { event, occurrence: o.repeats ? o.starts : null };
    } catch (err) {
      toast(errorText(err), "error");
    }
  }

  async function deleteOccurrence(o: Occurrence, every: boolean) {
    try {
      if (every || !o.repeats) await api.deleteEvent(o.event);
      else await api.skipOccurrence(o.event, o.starts);
      toast(every || !o.repeats ? `Deleted "${o.title || "(no title)"}".` : "Deleted this time only.");
    } catch (err) {
      toast(errorText(err), "error");
    }
  }

  async function moveTo(o: Occurrence, calendar: number) {
    try {
      const event = await api.getEvent(o.event);
      if (event) await api.saveEvent({ ...event, calendar });
    } catch (err) {
      toast(errorText(err), "error");
    }
  }

  function eventMenu(e: MouseEvent, o: Occurrence) {
    openMenu(
      e,
      items(
        { label: "Open", icon: "edit", run: () => open(o) },
        writable.length > 1 &&
          !calendars.find((c) => c.id === o.calendar)?.readOnly && {
          label: "Move to calendar",
          icon: "calendar",
          items: writable.map((c) => ({
            label: c.name,
            icon: c.id === o.calendar ? ("check" as const) : undefined,
            disabled: c.id === o.calendar,
            run: () => moveTo(o, c.id),
          })),
        },
        "separator",
        o.repeats && { label: "Delete only this time", icon: "trash", run: () => deleteOccurrence(o, false) },
        !calendars.find((c) => c.id === o.calendar)?.readOnly && {
          label: o.repeats ? "Delete every time" : "Delete",
          icon: "trash",
          danger: true,
          run: () => deleteOccurrence(o, true),
        },
      ),
    );
  }

  function dayMenu(e: MouseEvent, day: Date, at?: Date) {
    openMenu(e, [
      { label: "New event", icon: "plus", shortcut: "C", run: () => newEvent(at ?? withHour(day, 9)) },
      { label: "New all-day event", icon: "calendar", run: () => newEvent(day, true) },
      "separator",
      { label: "Show this day", icon: "list", run: () => show(day) },
    ]);
  }

  function withHour(day: Date, hour: number, minute = 0): Date {
    return new Date(day.getFullYear(), day.getMonth(), day.getDate(), hour, minute);
  }

  /** Click on an empty half hour of the time grid: an hour-long event there. */
  function slotClick(e: MouseEvent, day: Date) {
    const box = (e.currentTarget as HTMLElement).getBoundingClientRect();
    const minutes = Math.floor(((e.clientY - box.top) / HOUR_PX) * 2) * 30;
    newEvent(withHour(day, Math.floor(minutes / 60), minutes % 60));
  }

  function slotMenu(e: MouseEvent, day: Date) {
    const box = (e.currentTarget as HTMLElement).getBoundingClientRect();
    const minutes = Math.floor(((e.clientY - box.top) / HOUR_PX) * 2) * 30;
    dayMenu(e, day, withHour(day, Math.floor(minutes / 60), minutes % 60));
  }

  /** The timed events of a day, placed in the grid. */
  function placed(day: Date) {
    const dayStart = day.getTime();
    const dayEnd = addDays(day, 1).getTime();
    return columns(
      occurrences
        .filter((o) => !long(o) && onDay(o, day))
        .map((o) => {
          const [s, e] = span(o);
          const start = Math.max(s.getTime(), dayStart);
          // Short events still get room for their title.
          const end = Math.max(Math.min(e.getTime(), dayEnd), start + 20 * 60_000);
          return { o, start, end };
        }),
    ).map((p) => ({
      ...p,
      top: ((p.start - dayStart) / 3_600_000) * HOUR_PX,
      height: ((p.end - p.start) / 3_600_000) * HOUR_PX,
    }));
  }

  function whenText(o: Occurrence): string {
    if (o.allDay) return "All day";
    const [s, e] = span(o);
    return `${timeText(s)} – ${timeText(e)}`;
  }

  function tip(o: Occurrence): string {
    return [o.title || "(no title)", whenText(o), o.location, o.repeats ? "Repeats" : ""].filter(Boolean).join("\n");
  }

  const monthRows = $derived(Array.from({ length: 6 }, (_, r) => days.slice(r * 7, r * 7 + 7)));
  const nowTop = $derived(((now.getTime() - startOfDay(now).getTime()) / 3_600_000) * HOUR_PX);

  // Calendars.

  async function saveCalendar(calendar: Calendar) {
    try {
      await api.saveCalendar(calendar);
    } catch (err) {
      toast(errorText(err), "error");
    }
  }

  async function addCalendar() {
    const name = newCalendarName.trim();
    addingCalendar = false;
    newCalendarName = "";
    if (!name) return;
    const free = COLORS.find(([c]) => !calendars.some((k) => k.color === c));
    const color = (free ?? COLORS[calendars.length % COLORS.length])[0];
    await saveCalendar({ id: 0, name, color, account: null, visible: true });
  }

  async function finishRename(calendar: Calendar) {
    const name = renameText.trim();
    renaming = null;
    if (name && name !== calendar.name) await saveCalendar({ ...calendar, name });
  }

  async function removeCalendar(calendar: Calendar) {
    try {
      await api.deleteCalendar(calendar.id);
      toast(`Deleted the calendar "${calendar.name}".`);
    } catch (err) {
      toast(errorText(err), "error");
    }
  }

  async function startRename(calendar: Calendar) {
    renaming = calendar.id;
    renameText = calendar.name;
    await tick();
    document.querySelector<HTMLInputElement>(".calendars input.rename")?.select();
  }

  function calendarMenu(e: MouseEvent, calendar: Calendar) {
    const colors: MenuItem[] = COLORS.map(([color, name]) => ({
      label: color === calendar.color ? `${name} ✓` : name,
      swatch: color,
      run: () => saveCalendar({ ...calendar, color }),
    }));
    const local = calendar.source == null;
    openMenu(
      e,
      items(
        local && { label: "Rename", icon: "edit", run: () => startRename(calendar) },
        { label: "Colour", icon: "palette", items: colors },
        {
          label: "Show only this calendar",
          icon: "calendar",
          run: async () => {
            for (const c of calendars) if (c.visible !== (c.id === calendar.id)) await saveCalendar({ ...c, visible: c.id === calendar.id });
          },
        },
        "separator",
        local && localCalendars.length > 1 && {
          label: "Delete calendar",
          icon: "trash",
          danger: true,
          run: () => removeCalendar(calendar),
        },
      ),
    );
  }

  // Online calendars.

  /** The add button: a calendar here, or an online account's. */
  function addMenu(e: MouseEvent) {
    toggleMenuUnder(
      e.currentTarget as Element,
      items(
        { label: "New calendar on this computer", icon: "plus", run: () => (addingCalendar = true) },
        online.providers.length > 0 && "separator",
        online.providers.includes("google") && {
          label: "Connect Google Calendar…",
          icon: "cloud",
          run: () => connect("google"),
        },
        online.providers.includes("microsoft") && {
          label: "Connect Outlook or Hotmail…",
          icon: "cloud",
          run: () => connect("microsoft"),
        },
      ),
    );
  }

  async function connect(provider: OAuthProvider, id: number | null = null) {
    connecting = provider;
    try {
      const account = await api.connectCalendar(provider, id);
      toast(`Syncing the calendars of ${account.email}.`);
    } catch (err) {
      const text = errorText(err);
      if (!text.includes("cancelled")) toast(text, "error");
    } finally {
      connecting = null;
    }
  }

  async function disconnect(account: CalendarAccount) {
    try {
      await api.disconnectCalendar(account.id);
      toast(`Disconnected ${account.email}. Its calendars are still online.`);
    } catch (err) {
      toast(errorText(err), "error");
    }
  }

  function accountMenu(e: MouseEvent, account: CalendarAccount) {
    openMenu(
      e,
      items(
        { label: "Sync now", icon: "refresh", run: () => api.syncCalendars() },
        account.needsSignIn && { label: "Sign in again", icon: "sign-in", run: () => connect(account.provider, account.id) },
        "separator",
        { label: "Disconnect", icon: "remove-account", danger: true, run: () => disconnect(account) },
      ),
    );
  }

  function syncedText(): string {
    const times = online.accounts.map((a) => a.syncedAt).filter((t): t is number => t != null);
    if (!times.length) return "Not synced yet.";
    const at = new Date(Math.min(...times) * 1000);
    const minutes = Math.round((now.getTime() - at.getTime()) / 60_000);
    if (minutes < 1) return "Synced just now.";
    if (minutes < 60) return `Synced ${minutes} min ago.`;
    return `Synced at ${timeText(at)}.`;
  }

  function typing(e: KeyboardEvent): boolean {
    const t = e.target as HTMLElement | null;
    return !!t && (t.isContentEditable || ["INPUT", "TEXTAREA", "SELECT"].includes(t.tagName));
  }

  function shortcut(e: KeyboardEvent) {
    if (!active || blocked || editing || e.defaultPrevented || e.ctrlKey || e.altKey || e.metaKey || typing(e)) return;
    const key = e.key.toLowerCase();
    if (key === "t") today();
    else if (key === "j" || key === "n" || e.key === "ArrowRight") go(1);
    else if (key === "k" || key === "p" || e.key === "ArrowLeft") go(-1);
    else if (key === "m") mode = "month";
    else if (key === "w") mode = "week";
    else if (key === "d") mode = "day";
    else if (key === "c") newEvent();
    else return;
    e.preventDefault();
  }
</script>

<svelte:window onkeydown={shortcut} />

<div class="calendar">
  <aside class="side">
    <button class="primary new" onclick={() => newEvent()} title="New event (C)"><Icon name="plus" /> New event</button>

    <div class="mini" aria-label="Pick a day">
      <div class="mini-head">
        <span>{mini.toLocaleDateString(undefined, { month: "long", year: "numeric" })}</span>
        <button class="icon-only" aria-label="Previous month" onclick={() => (mini = new Date(mini.getFullYear(), mini.getMonth() - 1, 1))}><Icon name="chevron-left" size={14} /></button>
        <button class="icon-only" aria-label="Next month" onclick={() => (mini = new Date(mini.getFullYear(), mini.getMonth() + 1, 1))}><Icon name="chevron-right" size={14} /></button>
      </div>
      <div class="mini-grid">
        {#each weekdayNames as name (name)}<span class="mini-wd">{name.slice(0, 2)}</span>{/each}
        {#each miniDays as day (day.getTime())}
          <button
            class="mini-day"
            class:other={day.getMonth() !== mini.getMonth()}
            class:today={sameDay(day, now)}
            class:chosen={days.some((d) => sameDay(d, day)) && mode !== "month"}
            aria-label={day.toLocaleDateString(undefined, { dateStyle: "full" })}
            onclick={() => show(day, mode === "month" ? "day" : mode)}>{day.getDate()}</button>
        {/each}
      </div>
    </div>

    {#snippet row(c: Calendar)}
      <li oncontextmenu={(e) => calendarMenu(e, c)}>
        {#if renaming === c.id}
          <span class="swatch" style:background={c.color}></span>
          <!-- svelte-ignore a11y_autofocus -->
          <input
            class="rename"
            aria-label="Calendar name"
            bind:value={renameText}
            onblur={() => finishRename(c)}
            onkeydown={(e) => {
              if (e.key === "Enter") finishRename(c);
              else if (e.key === "Escape") renaming = null;
            }}
          />
        {:else}
          <label title={c.visible ? "Shown" : "Hidden"}>
            <input type="checkbox" checked={c.visible} onchange={() => saveCalendar({ ...c, visible: !c.visible })} style:--swatch={c.color} />
            <span class="name">{c.name}</span>
            {#if c.readOnly}<span class="lock" title="Can only be looked at"><Icon name="lock" size={12} /></span>{/if}
          </label>
          <button class="icon-only more" aria-label="Options for {c.name}" onclick={(e) => calendarMenu(e, c)}><Icon name="more" size={14} /></button>
        {/if}
      </li>
    {/snippet}

    <div class="calendars">
      <div class="heading">
        <span>{online.accounts.length ? "On this computer" : "Calendars"}</span>
        <button class="icon-only" aria-label="Add a calendar" title="Add a calendar" onclick={addMenu}><Icon name="plus" size={14} /></button>
      </div>
      <ul>
        {#each localCalendars as c (c.id)}
          {@render row(c)}
        {/each}
        {#if addingCalendar}
          <li>
            <!-- svelte-ignore a11y_autofocus -->
            <input
              class="rename"
              placeholder="Calendar name"
              aria-label="New calendar name"
              autofocus
              bind:value={newCalendarName}
              onblur={addCalendar}
              onkeydown={(e) => {
                if (e.key === "Enter") addCalendar();
                else if (e.key === "Escape") {
                  newCalendarName = "";
                  addingCalendar = false;
                }
              }}
            />
          </li>
        {/if}
      </ul>
    </div>

    {#each online.accounts as a (a.id)}
      <div class="calendars">
        <div class="heading" oncontextmenu={(e) => (e.preventDefault(), accountMenu(e, a))} role="group" aria-label={a.email}>
          <span>{SERVICE[a.provider]}</span>
          <button class="icon-only" aria-label="Options for {a.email}" title="Options for {a.email}" onclick={(e) => accountMenu(e, a)}><Icon name="more" size={14} /></button>
        </div>
        <p class="account quiet" title={a.email}>{a.email}</p>
        {#if a.error}
          <p class="account-error">
            {a.needsSignIn ? "The sign-in has expired." : a.error}
            {#if a.needsSignIn}<button class="link" onclick={() => connect(a.provider, a.id)}>Sign in again</button>{/if}
          </p>
        {/if}
        <ul>
          {#each calendars.filter((c) => c.source === a.id) as c (c.id)}
            {@render row(c)}
          {/each}
        </ul>
      </div>
    {/each}

    <div class="quiet local">
      {#if connecting}
        <p>Finish signing in to {SERVICE[connecting]} in your browser.</p>
        <button class="link" onclick={() => api.cancelSignIn()}>Cancel</button>
      {:else if online.syncing}
        <p>Syncing…</p>
      {:else if online.accounts.length}
        <p>{syncedText()}</p>
      {:else}
        <p>Kept on this computer.</p>
        {#if online.providers.length}
          <button class="link" onclick={addMenu}>Sync with Google or Outlook</button>
        {/if}
      {/if}
    </div>
  </aside>

  <section class="main">
    <header>
      <button onclick={today} title="Today (T)">Today</button>
      <button class="icon-only" aria-label="Previous" title="Previous (K or ←)" onclick={() => go(-1)}><Icon name="chevron-left" size={18} /></button>
      <button class="icon-only" aria-label="Next" title="Next (J or →)" onclick={() => go(1)}><Icon name="chevron-right" size={18} /></button>
      <h1>{title}</h1>
      <span class="grow"></span>
      <div class="modes" role="group" aria-label="View">
        {#each [["day", "Day", "D"], ["week", "Week", "W"], ["month", "Month", "M"]] as [m, label, key] (m)}
          <button class:current={mode === m} aria-pressed={mode === m} title="{label} ({key})" onclick={() => (mode = m as CalendarMode)}>{label}</button>
        {/each}
      </div>
    </header>
    {#if error}<p class="error pad">{error}</p>{/if}

    {#if mode === "month"}
      <div class="month">
        <div class="wd-row">
          {#each weekdayNames as name (name)}<span>{name}</span>{/each}
        </div>
        <div class="weeks">
          {#each monthRows as row, r (r)}
            <div class="week-row">
              {#each row as day (day.getTime())}
                {@const todays = occurrences.filter((o) => onDay(o, day))}
                <!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_static_element_interactions -->
                <div
                  class="cell"
                  class:other={day.getMonth() !== anchor.getMonth()}
                  class:today={sameDay(day, now)}
                  ondblclick={() => newEvent(withHour(day, 9))}
                  oncontextmenu={(e) => dayMenu(e, day)}
                >
                  <button class="date" onclick={() => show(day)} aria-label="Show {day.toLocaleDateString(undefined, { dateStyle: 'full' })}">
                    {day.getDate() === 1 ? day.toLocaleDateString(undefined, { day: "numeric", month: "short" }) : day.getDate()}
                  </button>
                  {#each todays.slice(0, todays.length > 4 ? 3 : 4) as o (`${o.event}:${o.starts}`)}
                    <button
                      class="chip"
                      class:filled={long(o)}
                      class:cancelled={o.cancelled}
                      class:declined={o.response === "declined"}
                      style:--c={colorOf.get(o.calendar)}
                      title={tip(o)}
                      onclick={(e) => (e.stopPropagation(), open(o))}
                      ondblclick={(e) => e.stopPropagation()}
                      oncontextmenu={(e) => eventMenu(e, o)}
                    >
                      {#if !long(o)}<span class="dot"></span><span class="time">{timeText(span(o)[0])}</span>{/if}
                      <span class="text">{o.title || "(no title)"}</span>
                    </button>
                  {/each}
                  {#if todays.length > 4}
                    <button class="more-link" onclick={(e) => (e.stopPropagation(), show(day))}>{todays.length - 3} more</button>
                  {/if}
                </div>
              {/each}
            </div>
          {/each}
        </div>
      </div>
    {:else}
      <div class="timegrid" style:--days={days.length}>
        <div class="head-row">
          <span class="gutter"></span>
          {#each days as day (day.getTime())}
            <button class="day-head" class:today={sameDay(day, now)} onclick={() => show(day)}>
              <span class="wd">{day.toLocaleDateString(undefined, { weekday: "short" })}</span>
              <span class="num">{day.getDate()}</span>
            </button>
          {/each}
        </div>
        <div class="allday-row">
          <span class="gutter quiet">All day</span>
          {#each days as day (day.getTime())}
            <!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_static_element_interactions -->
            <div class="allday" ondblclick={() => newEvent(day, true)} oncontextmenu={(e) => dayMenu(e, day)}>
              {#each occurrences.filter((o) => long(o) && onDay(o, day)) as o (`${o.event}:${o.starts}`)}
                <button
                  class="chip filled"
                  class:cancelled={o.cancelled}
                  class:declined={o.response === "declined"}
                  style:--c={colorOf.get(o.calendar)}
                  title={tip(o)}
                  onclick={() => open(o)}
                  ondblclick={(e) => e.stopPropagation()}
                  oncontextmenu={(e) => eventMenu(e, o)}><span class="text">{o.title || "(no title)"}</span></button>
              {/each}
            </div>
          {/each}
        </div>
        <div class="scroll" use:morning>
          <div class="hours">
            <div class="gutter">
              {#each Array.from({ length: 24 }, (_, h) => h) as h (h)}
                <span class="hour" style:top="{h * HOUR_PX}px">{h === 0 ? "" : timeText(withHour(now, h))}</span>
              {/each}
            </div>
            {#each days as day (day.getTime())}
              <!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_static_element_interactions -->
              <div class="column" class:today={sameDay(day, now)} onclick={(e) => slotClick(e, day)} oncontextmenu={(e) => slotMenu(e, day)}>
                {#each placed(day) as p (`${p.o.event}:${p.o.starts}`)}
                  <button
                    class="block"
                    class:cancelled={p.o.cancelled}
                    class:declined={p.o.response === "declined"}
                    class:short={p.height < 36}
                    style:--c={colorOf.get(p.o.calendar)}
                    style:top="{p.top}px"
                    style:height="{p.height - 2}px"
                    style:left="calc({(p.col / p.cols) * 100}% + 2px)"
                    style:width="calc({100 / p.cols}% - 4px)"
                    title={tip(p.o)}
                    onclick={(e) => (e.stopPropagation(), open(p.o))}
                    oncontextmenu={(e) => (e.stopPropagation(), eventMenu(e, p.o))}
                  >
                    <span class="text">{p.o.title || "(no title)"}</span>
                    <span class="time">{timeText(span(p.o)[0])}{p.o.location ? ` · ${p.o.location}` : ""}</span>
                  </button>
                {/each}
                {#if sameDay(day, now)}<span class="now" style:top="{nowTop}px"></span>{/if}
              </div>
            {/each}
          </div>
        </div>
      </div>
    {/if}
  </section>
</div>

{#if editing}
  <EventDialog event={editing.event} occurrence={editing.occurrence} {calendars} onClose={() => (editing = null)} />
{/if}

<style>
  .calendar {
    display: grid;
    grid-template-columns: 232px minmax(0, 1fr);
    grid-template-rows: minmax(0, 1fr);
    height: 100%;
    min-width: 0;
  }
  .side {
    background: var(--bg-side);
    border-right: 1px solid var(--border);
    padding: 12px;
    display: flex;
    flex-direction: column;
    gap: 16px;
    overflow-y: auto;
  }
  .new {
    justify-content: center;
  }
  .mini-head {
    display: flex;
    align-items: center;
    gap: 2px;
    font-weight: 600;
    font-size: 13px;
    margin-bottom: 4px;
  }
  .mini-head span {
    flex: 1;
  }
  .mini-grid {
    display: grid;
    grid-template-columns: repeat(7, 1fr);
    text-align: center;
    font-size: 12px;
  }
  .mini-wd {
    color: var(--text-quiet);
    padding: 2px 0;
  }
  .mini-day {
    border: none;
    background: none;
    padding: 0;
    height: 26px;
    margin: 1px;
    border-radius: 4px;
    font-size: 12px;
  }
  .mini-day.other {
    color: var(--text-quiet);
    opacity: 0.6;
  }
  .mini-day.chosen {
    background: var(--bg-selected);
  }
  .mini-day.today {
    background: var(--accent);
    color: var(--accent-text);
    font-weight: 600;
  }
  .heading {
    display: flex;
    align-items: center;
    font-size: 12px;
    font-weight: 600;
    text-transform: uppercase;
    letter-spacing: 0.04em;
    color: var(--text-quiet);
  }
  .heading span {
    flex: 1;
  }
  .calendars ul {
    list-style: none;
    margin: 4px 0 0;
    padding: 0;
  }
  .calendars li {
    display: flex;
    align-items: center;
    gap: 6px;
    min-height: 30px;
    border-radius: 6px;
    padding: 0 2px 0 4px;
  }
  .calendars li:hover {
    background: var(--bg-hover);
  }
  .calendars label {
    flex: 1;
    display: flex;
    align-items: center;
    gap: 8px;
    min-width: 0;
    cursor: pointer;
  }
  .calendars input[type="checkbox"] {
    width: 16px;
    height: 16px;
    margin: 0;
    accent-color: var(--swatch);
    flex: none;
  }
  .calendars .name {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .calendars .more {
    opacity: 0;
  }
  .calendars li:hover .more,
  .calendars .more:focus-visible {
    opacity: 1;
  }
  .rename {
    padding: 4px 6px;
  }
  .swatch {
    width: 14px;
    height: 14px;
    border-radius: 4px;
    flex: none;
  }
  .lock {
    display: inline-flex;
    color: var(--text-quiet);
    flex: none;
  }
  .account {
    margin: 2px 0 0;
    font-size: 12px;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .account-error {
    margin: 6px 0 0;
    font-size: 12px;
    color: var(--danger);
  }
  .link {
    border: none;
    background: none;
    padding: 0;
    color: var(--accent-ink);
    font-size: 12px;
    cursor: pointer;
    text-decoration: underline;
  }
  .local {
    margin: auto 0 0;
    font-size: 12px;
    display: flex;
    flex-wrap: wrap;
    gap: 4px 8px;
    align-items: baseline;
  }
  .local p {
    margin: 0;
  }

  .main {
    display: flex;
    flex-direction: column;
    min-width: 0;
    min-height: 0;
  }
  header {
    display: flex;
    align-items: center;
    gap: 6px;
    padding: 10px 16px;
    border-bottom: 1px solid var(--border);
  }
  h1 {
    font-size: 18px;
    margin: 0 0 0 8px;
    font-weight: 600;
    white-space: nowrap;
  }
  .grow {
    flex: 1;
  }
  .modes {
    display: flex;
    border: 1px solid var(--border);
    border-radius: var(--radius);
    overflow: hidden;
  }
  .modes button {
    border: none;
    border-radius: 0;
    padding: 5px 12px;
    background: none;
  }
  .modes button + button {
    border-left: 1px solid var(--border);
  }
  .modes button.current {
    background: var(--bg-selected);
    color: var(--accent-ink);
    font-weight: 600;
  }
  .pad {
    padding: 0 16px;
  }

  /* Month */
  .month {
    flex: 1;
    display: flex;
    flex-direction: column;
    min-height: 0;
  }
  .wd-row {
    display: grid;
    grid-template-columns: repeat(7, minmax(0, 1fr));
    font-size: 12px;
    color: var(--text-quiet);
    border-bottom: 1px solid var(--border);
  }
  .wd-row span {
    padding: 6px 8px;
  }
  .weeks {
    flex: 1;
    display: grid;
    grid-template-rows: repeat(6, minmax(0, 1fr));
  }
  .week-row {
    display: grid;
    grid-template-columns: repeat(7, minmax(0, 1fr));
    border-bottom: 1px solid var(--border);
    min-height: 0;
  }
  .cell {
    border-right: 1px solid var(--border);
    padding: 2px 4px;
    display: flex;
    flex-direction: column;
    gap: 2px;
    min-width: 0;
    overflow: hidden;
  }
  .cell:last-child {
    border-right: none;
  }
  .cell.other {
    background: color-mix(in srgb, var(--bg-side) 60%, transparent);
  }
  .cell.other .date {
    color: var(--text-quiet);
  }
  .date {
    align-self: flex-start;
    border: none;
    background: none;
    padding: 2px 6px;
    border-radius: 4px;
    font-size: 12px;
  }
  .cell.today .date,
  .day-head.today .num {
    background: var(--accent);
    color: var(--accent-text);
    font-weight: 600;
  }
  .chip {
    display: flex;
    align-items: center;
    gap: 4px;
    width: 100%;
    border: none;
    border-radius: 4px;
    padding: 1px 6px;
    font-size: 12px;
    text-align: left;
    background: none;
    min-width: 0;
  }
  .chip:hover {
    background: var(--bg-hover);
  }
  .chip.filled {
    background: color-mix(in srgb, var(--c) 22%, var(--bg));
    border-left: 3px solid var(--c);
  }
  .chip .dot {
    width: 8px;
    height: 8px;
    border-radius: 50%;
    background: var(--c);
    flex: none;
  }
  .chip .time {
    color: var(--text-quiet);
    flex: none;
  }
  .text {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .cancelled .text {
    text-decoration: line-through;
  }
  .declined {
    opacity: 0.55;
  }
  .more-link {
    border: none;
    background: none;
    padding: 0 6px;
    font-size: 12px;
    color: var(--text-quiet);
    text-align: left;
  }

  /* Week and day */
  .timegrid {
    flex: 1;
    display: flex;
    flex-direction: column;
    min-height: 0;
    --gutter: 64px;
  }
  .head-row,
  .allday-row,
  .hours {
    display: grid;
    grid-template-columns: var(--gutter) repeat(var(--days), minmax(0, 1fr));
  }
  /* Keep room for a scroll bar like the hours below, so the columns line up. */
  .head-row,
  .allday-row {
    overflow-y: hidden;
    scrollbar-gutter: stable;
  }
  .day-head {
    border: none;
    border-radius: 0;
    background: none;
    display: flex;
    align-items: baseline;
    justify-content: center;
    gap: 6px;
    padding: 6px 0;
  }
  .day-head .wd {
    color: var(--text-quiet);
    font-size: 12px;
  }
  .day-head .num {
    font-size: 16px;
    padding: 0 6px;
    border-radius: 4px;
  }
  .allday-row {
    border-bottom: 1px solid var(--border);
    min-height: 28px;
  }
  .allday-row .gutter {
    font-size: 11px;
    text-align: right;
    padding: 6px 8px 0 0;
  }
  .allday {
    border-left: 1px solid var(--border);
    padding: 2px;
    display: grid;
    gap: 2px;
    align-content: start;
    min-width: 0;
  }
  .scroll {
    flex: 1;
    overflow-y: auto;
    scrollbar-gutter: stable;
  }
  .hours {
    position: relative;
    height: calc(24 * 48px);
  }
  .hours .gutter {
    position: relative;
  }
  .hour {
    position: absolute;
    right: 8px;
    transform: translateY(-50%);
    font-size: 11px;
    color: var(--text-quiet);
    white-space: nowrap;
  }
  .column {
    position: relative;
    border-left: 1px solid var(--border);
    background-image: linear-gradient(var(--border) 1px, transparent 1px);
    background-size: 100% 48px;
    cursor: default;
  }
  .column.today {
    background-color: color-mix(in srgb, var(--bg-selected) 35%, transparent);
  }
  .block {
    position: absolute;
    display: flex;
    flex-direction: column;
    align-items: flex-start;
    gap: 0;
    padding: 3px 6px;
    border: none;
    border-left: 3px solid var(--c);
    border-radius: 4px;
    background: color-mix(in srgb, var(--c) 22%, var(--bg));
    font-size: 12px;
    line-height: 1.3;
    text-align: left;
    overflow: hidden;
    min-width: 0;
  }
  .block:hover {
    background: color-mix(in srgb, var(--c) 32%, var(--bg));
  }
  .block .text {
    font-weight: 600;
    max-width: 100%;
  }
  .block .time {
    color: var(--text-quiet);
    max-width: 100%;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .block.short {
    flex-direction: row;
    gap: 6px;
    padding-top: 1px;
  }
  .now {
    position: absolute;
    left: -1px;
    right: 0;
    height: 2px;
    background: var(--danger);
    pointer-events: none;
    z-index: 2;
  }
  .now::before {
    content: "";
    position: absolute;
    left: -5px;
    top: -4px;
    width: 10px;
    height: 10px;
    border-radius: 50%;
    background: var(--danger);
  }
</style>
