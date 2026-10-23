<script lang="ts">
  // A received attachment shown in the reading pane, in place of the
  // message: pictures, PDFs and text files. Everything else can be saved
  // from here. Its bar takes the place of the message's actions.
  import { onDestroy, onMount, tick, untrack } from "svelte";
  import type { RenderTask } from "pdfjs-dist";
  import Icon from "../ui/Icon.svelte";
  import { api, errorText, type Attachment } from "../api";
  import { fileSize } from "../format";
  import { openPdf, type OpenPdf } from "./pdf";
  import { toast } from "../shell/toast.svelte";
  import { mediaType, stepOfButton, stepOfKey, typingOrDialog, viewKind } from "./viewer";

  interface Props {
    /** The message the attachments belong to. */
    message: number;
    attachments: Attachment[];
    start: number;
    /** Back to the message; `index` is the attachment that was showing. */
    onClose: (index: number) => void;
  }
  let { message, attachments, start, onClose }: Props = $props();

  const STEPS = [0.25, 0.33, 0.5, 0.67, 0.75, 0.9, 1, 1.1, 1.25, 1.5, 1.75, 2, 2.5, 3, 4, 5];
  /** Fit to the pane: pictures whole, PDFs to its width. */
  const FIT = 0;
  /** CSS pixels per PDF point: at 100% a page shows at its printed size. */
  const PT = 96 / 72;
  /** Fitting a PDF to a wide window stops here; lines get too long to read. */
  const MAX_FIT_PDF = 1.25;
  /** Space around a fitted picture or page. */
  const MARGIN = 48;

  let index = $state(untrack(() => start));
  const current = $derived(attachments[index]);
  const kind = $derived(current ? viewKind(current) : null);

  let loading = $state(false);
  let error = $state("");
  let saving = $state(false);
  let zoom = $state(FIT);
  let rotation = $state(0);

  let imageUrl = $state("");
  let natural = $state({ width: 0, height: 0 });
  let text = $state("");
  let opened = $state.raw<OpenPdf | null>(null);
  const pdf = $derived(opened?.doc ?? null);
  /** Page sizes at 100%, unrotated. */
  let pages = $state.raw<{ width: number; height: number }[]>([]);
  let pageNow = $state(1);

  let stage = $state<HTMLDivElement>();
  let stageSize = $state({ width: 0, height: 0 });
  let viewer = $state<HTMLElement>();
  let request = 0;

  function clear() {
    if (imageUrl) URL.revokeObjectURL(imageUrl);
    imageUrl = "";
    natural = { width: 0, height: 0 };
    text = "";
    opened?.close();
    opened = null;
    pages = [];
    pageNow = 1;
    rendered.clear();
  }

  async function show(i: number) {
    const mine = ++request;
    clear();
    error = "";
    zoom = FIT;
    rotation = 0;
    const a = attachments[i];
    const how = viewKind(a);
    if (!how) {
      loading = false;
      return;
    }
    loading = true;
    try {
      const data = await api.attachmentContents(message, i);
      if (mine !== request) return;
      if (how === "image") {
        imageUrl = URL.createObjectURL(new Blob([data], { type: mediaType(a) }));
      } else if (how === "text") {
        text = new TextDecoder("utf-8").decode(data);
      } else {
        const file = await openPdf(data);
        if (mine !== request) {
          file.close();
          return;
        }
        const doc = file.doc;
        const sizes = [];
        for (let n = 1; n <= doc.numPages; n++) {
          const viewport = (await doc.getPage(n)).getViewport({ scale: 1 });
          sizes.push({ width: viewport.width, height: viewport.height });
        }
        if (mine !== request) {
          file.close();
          return;
        }
        opened = file;
        pages = sizes;
      }
    } catch (err) {
      if (mine === request) error = kind === "pdf" ? "This PDF could not be opened." : errorText(err);
    } finally {
      if (mine === request) loading = false;
    }
  }

  $effect(() => {
    const i = index;
    untrack(() => show(i));
  });

  onMount(() => {
    viewer?.focus();
    const observer = new ResizeObserver(() => {
      if (stage) stageSize = { width: stage.clientWidth, height: stage.clientHeight };
    });
    if (stage) observer.observe(stage);
    return () => observer.disconnect();
  });

  onDestroy(() => {
    request++;
    clear();
    pageObserver?.disconnect();
  });

  const turned = $derived(rotation % 180 !== 0);

  /** The scale "fit" means right now. */
  const fitScale = $derived.by(() => {
    const room = { width: stageSize.width - MARGIN, height: stageSize.height - MARGIN };
    if (room.width <= 0 || room.height <= 0) return 1;
    if (kind === "image" && natural.width) {
      const w = turned ? natural.height : natural.width;
      const h = turned ? natural.width : natural.height;
      return Math.min(1, room.width / w, room.height / h);
    }
    if (kind === "pdf" && pages.length) {
      const widest = Math.max(...pages.map((p) => (turned ? p.height : p.width))) * PT;
      return Math.min(MAX_FIT_PDF, room.width / widest);
    }
    return 1;
  });
  const scale = $derived(zoom === FIT ? fitScale : zoom);

  function zoomBy(direction: 1 | -1) {
    const now = scale;
    const next =
      direction > 0 ? STEPS.find((s) => s > now + 0.001) : [...STEPS].reverse().find((s) => s < now - 0.001);
    if (next) zoomTo(next);
  }

  /** Zooms, keeping what is in the middle of the view there. */
  async function zoomTo(next: number) {
    const el = stage;
    if (!el) {
      zoom = next;
      return;
    }
    const x = (el.scrollLeft + el.clientWidth / 2) / Math.max(1, el.scrollWidth);
    const y = (el.scrollTop + el.clientHeight / 2) / Math.max(1, el.scrollHeight);
    zoom = next;
    await tick();
    el.scrollLeft = x * el.scrollWidth - el.clientWidth / 2;
    el.scrollTop = y * el.scrollHeight - el.clientHeight / 2;
  }

  function rotate() {
    rotation = (rotation + 90) % 360;
  }

  function go(step: number) {
    const next = index + step;
    if (next >= 0 && next < attachments.length) index = next;
  }

  async function save() {
    saving = true;
    try {
      if (await api.saveAttachment(message, index)) toast(`Saved ${current.name}`);
    } catch (err) {
      toast(errorText(err), "error");
    } finally {
      saving = false;
    }
  }

  function close() {
    onClose(index);
  }

  function keydown(e: KeyboardEvent) {
    if (typingOrDialog(e)) return;
    if (stepOfKey(e) === "back") {
      e.preventDefault();
      e.stopPropagation();
      close();
      return;
    }
    if (e.altKey) return;
    const key = e.key;
    const plain = !e.ctrlKey && !e.metaKey;
    let handled = true;
    if (key === "Escape" && plain) close();
    else if ((key === "+" || key === "=") && plain && kind && kind !== "text") zoomBy(1);
    else if (key === "-" && plain && kind && kind !== "text") zoomBy(-1);
    else if (key === "0" && plain) zoomTo(FIT);
    else if (key === "r" && plain && (kind === "image" || kind === "pdf")) rotate();
    else if (key === "ArrowLeft" && plain && attachments.length > 1) go(-1);
    else if (key === "ArrowRight" && plain && attachments.length > 1) go(1);
    else if ((key === "s" || key === "S") && !plain) save();
    else handled = false;
    // Keys the viewer uses don't also reach the mail shortcuts (R would reply).
    if (handled) {
      e.preventDefault();
      e.stopPropagation();
    }
  }

  /** The mouse's back button goes back to the message, as in a browser. */
  function mouseup(e: MouseEvent) {
    if (stepOfButton(e) !== "back" || document.querySelector('[aria-modal="true"]')) return;
    e.preventDefault();
    e.stopPropagation();
    close();
  }

  function wheel(e: WheelEvent) {
    if (!e.ctrlKey || !kind || kind === "text") return;
    e.preventDefault();
    zoomBy(e.deltaY < 0 ? 1 : -1);
  }

  // PDF pages are drawn when they scroll into view, again after zooming.
  let pageObserver: IntersectionObserver | null = null;
  const visible = new Set<number>();
  /** What each page was last drawn at, "scale/rotation". */
  const rendered = new Map<number, string>();
  const canvases = new Map<number, HTMLCanvasElement>();
  const tasks = new Map<number, RenderTask>();

  async function draw(n: number) {
    const doc = pdf;
    const canvas = canvases.get(n);
    const at = `${scale}/${rotation}`;
    if (!doc || !canvas || rendered.get(n) === at) return;
    rendered.set(n, at);
    tasks.get(n)?.cancel();
    const page = await doc.getPage(n + 1);
    const ratio = window.devicePixelRatio || 1;
    const viewport = page.getViewport({ scale: scale * PT * ratio, rotation });
    // Drawn off screen, then copied, so the old drawing stays until the new one is ready.
    const off = document.createElement("canvas");
    off.width = Math.floor(viewport.width);
    off.height = Math.floor(viewport.height);
    const task = page.render({ canvas: off, viewport });
    tasks.set(n, task);
    try {
      await task.promise;
    } catch {
      return; // Cancelled by a newer drawing, or closed.
    }
    if (rendered.get(n) !== at) return;
    canvas.width = off.width;
    canvas.height = off.height;
    canvas.getContext("2d")?.drawImage(off, 0, 0);
  }

  $effect(() => {
    // Redraw the pages on screen when the scale or rotation changes.
    void scale;
    void rotation;
    untrack(() => visible.forEach((n) => draw(n)));
  });

  function observePages(_node: HTMLElement) {
    pageObserver = new IntersectionObserver(
      (entries) => {
        for (const entry of entries) {
          const n = Number((entry.target as HTMLElement).dataset.page);
          if (entry.isIntersecting) {
            visible.add(n);
            draw(n);
          } else visible.delete(n);
        }
      },
      { root: stage, rootMargin: "400px 0px" },
    );
    // The pages' own actions ran first.
    for (const canvas of canvases.values()) pageObserver.observe(canvas);
    return {
      destroy() {
        pageObserver?.disconnect();
        pageObserver = null;
        visible.clear();
      },
    };
  }

  function pageSlot(node: HTMLCanvasElement, n: number) {
    canvases.set(n, node);
    pageObserver?.observe(node);
    return {
      destroy() {
        pageObserver?.unobserve(node);
        canvases.delete(n);
        tasks.get(n)?.cancel();
        tasks.delete(n);
      },
    };
  }

  /** The page in the middle of the view, for "3 / 12". */
  function scrolled() {
    if (!stage || kind !== "pdf") return;
    const middle = stage.scrollTop + stage.clientHeight / 2;
    let n = 1;
    for (const [i, canvas] of canvases) {
      if (canvas.offsetTop <= middle) n = Math.max(n, i + 1);
    }
    pageNow = n;
  }

  function pageBox(p: { width: number; height: number }) {
    const w = (turned ? p.height : p.width) * scale * PT;
    const h = (turned ? p.width : p.height) * scale * PT;
    return `width:${Math.floor(w)}px;height:${Math.floor(h)}px`;
  }

  const imageBox = $derived.by(() => {
    const w = natural.width * scale;
    const h = natural.height * scale;
    return {
      outer: `width:${turned ? h : w}px;height:${turned ? w : h}px`,
      inner: `width:${w}px;height:${h}px;transform:translate(-50%,-50%) rotate(${rotation}deg)`,
    };
  });
</script>

<svelte:window onkeydowncapture={keydown} onmouseupcapture={mouseup} />

<section class="viewer" aria-label={current?.name ?? "Attachment"} tabindex="-1" bind:this={viewer}>
  <div class="actions" role="toolbar" aria-label="Attachment actions">
    <button onclick={close} title="Back to the message (Esc, Alt+←, mouse back button)"><Icon name="chevron-left" />Back</button>
    {#if attachments.length > 1}
      <span class="group">
        <button class="square" title="Previous attachment (←)" aria-label="Previous attachment" disabled={index === 0} onclick={() => go(-1)}>
          <Icon name="chevron-left" />
        </button>
        <button
          class="square"
          title="Next attachment (→)"
          aria-label="Next attachment"
          disabled={index === attachments.length - 1}
          onclick={() => go(1)}
        >
          <Icon name="chevron-right" />
        </button>
      </span>
    {/if}
    <span class="gap"></span>
    {#if kind === "image" || kind === "pdf"}
      <span class="group">
        <button class="square" title="Zoom out (−)" aria-label="Zoom out" onclick={() => zoomBy(-1)}><Icon name="zoom-out" /></button>
        <button class="zoom" title="Fit (0)" onclick={() => zoomTo(FIT)}>{Math.round(scale * 100)}%</button>
        <button class="square" title="Zoom in (+)" aria-label="Zoom in" onclick={() => zoomBy(1)}><Icon name="zoom-in" /></button>
      </span>
      <button title="Fit to the pane (0)" aria-label="Fit" aria-pressed={zoom === FIT} onclick={() => zoomTo(FIT)}>
        <Icon name="fit" /><span class="text">Fit</span>
      </button>
      <button title="Rotate (R)" aria-label="Rotate" onclick={rotate}><Icon name="rotate" /><span class="text">Rotate</span></button>
    {/if}
    <button title="Save (Ctrl+S)" aria-label="Save" disabled={saving} onclick={save}><Icon name="save" /><span class="text">Save</span></button>
  </div>
  <header>
    <Icon name={kind === "image" ? "image" : kind ? "template" : "file"} />
    <h1 title={current?.name}>{current?.name}</h1>
    <span class="quiet facts">
      {current ? fileSize(current.size) : ""}
      {#if kind === "pdf" && pages.length}· Page {pageNow} of {pages.length}{/if}
      {#if attachments.length > 1}· Attachment {index + 1} of {attachments.length}{/if}
    </span>
  </header>

  <div
    class="stage"
    class:text={kind === "text"}
    bind:this={stage}
    onwheel={wheel}
    onscroll={scrolled}
    role="presentation"
  >
    {#if loading}
      <div class="message"><span class="spinner"></span></div>
    {:else if error}
      <div class="message">
        <Icon name="alert" size={28} />
        <p>{error}</p>
        <button onclick={save}><Icon name="save" />Save instead</button>
      </div>
    {:else if !kind}
      <div class="message">
        <Icon name="file" size={40} />
        <p><strong>{current.name}</strong></p>
        <p class="quiet">Novero Mail can't show this kind of file. Save it to open it in another app.</p>
        <button class="primary" disabled={saving} onclick={save}><Icon name="save" />Save…</button>
      </div>
    {:else if kind === "image" && imageUrl}
      <div class="center">
        <div class="picture" style={natural.width ? imageBox.outer : ""}>
          <img
            src={imageUrl}
            alt={current.name}
            style={natural.width ? imageBox.inner : "visibility:hidden"}
            onload={(e) => {
              const img = e.currentTarget as HTMLImageElement;
              natural = { width: img.naturalWidth || 300, height: img.naturalHeight || 300 };
            }}
            onerror={() => (error = "This picture could not be shown.")}
          />
        </div>
      </div>
    {:else if kind === "pdf" && pdf}
      <div class="pages" use:observePages>
        {#each pages as p, n (n)}
          <canvas class="page" data-page={n} style={pageBox(p)} use:pageSlot={n} aria-label="Page {n + 1}"></canvas>
        {/each}
      </div>
    {:else if kind === "text"}
      <pre>{text}</pre>
    {/if}
  </div>
</section>

<style>
  .viewer {
    height: 100%;
    display: flex;
    flex-direction: column;
    min-width: 0;
    outline: none;
    container-type: inline-size;
  }
  /* The same bar as the message's own actions, in its place. */
  .actions {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 6px;
    padding: 10px 24px;
    border-bottom: 1px solid var(--border);
    background: var(--bg);
  }
  .actions button {
    width: auto;
    padding: 5px 10px;
    font-size: 13px;
  }
  .actions button.square {
    padding: 5px 7px;
  }
  .actions button[aria-pressed="true"] {
    color: var(--accent-ink);
    border-color: var(--accent-ink);
  }
  .gap {
    flex: 1;
  }
  /* Buttons that belong together sit edge to edge. */
  .group {
    display: inline-flex;
  }
  .group button {
    border-radius: 0;
  }
  .group button + button {
    margin-left: -1px;
  }
  .group button:first-child {
    border-radius: 6px 0 0 6px;
  }
  .group button:last-child {
    border-radius: 0 6px 6px 0;
  }
  .zoom {
    min-width: 56px;
    font-variant-numeric: tabular-nums;
  }
  @container (max-width: 540px) {
    .actions .text {
      display: none;
    }
  }
  header {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 10px 24px;
    border-bottom: 1px solid var(--border);
    min-width: 0;
  }
  h1 {
    margin: 0;
    font-size: 15px;
    font-weight: 600;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    min-width: 0;
  }
  .facts {
    font-size: 12px;
    white-space: nowrap;
    font-variant-numeric: tabular-nums;
  }
  .stage {
    flex: 1;
    min-height: 0;
    overflow: auto;
    background: var(--viewer-bg);
  }
  .center {
    min-width: 100%;
    min-height: 100%;
    display: flex;
    align-items: center;
    justify-content: center;
    padding: 24px;
    box-sizing: border-box;
    width: max-content;
    height: max-content;
  }
  .picture {
    position: relative;
    flex: none;
  }
  .picture img {
    position: absolute;
    left: 50%;
    top: 50%;
    box-shadow: 0 4px 24px rgba(0, 0, 0, 0.3);
    /* Transparent pictures on a checkerboard, as image viewers show them. */
    background: repeating-conic-gradient(#d9d9d9 0% 25%, #ffffff 0% 50%) 50% / 16px 16px;
  }
  .pages {
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 16px;
    padding: 24px;
    width: max-content;
    min-width: 100%;
    box-sizing: border-box;
  }
  .page {
    display: block;
    background: #fff;
    box-shadow: 0 2px 12px rgba(0, 0, 0, 0.25);
    flex: none;
  }
  .stage.text {
    background: var(--bg);
  }
  pre {
    margin: 0;
    padding: 20px 24px;
    font: 13px/1.5 ui-monospace, "Cascadia Mono", Consolas, monospace;
    white-space: pre-wrap;
    word-break: break-word;
    user-select: text;
  }
  .message {
    height: 100%;
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    gap: 8px;
    text-align: center;
    padding: 24px;
    box-sizing: border-box;
  }
  .message p {
    margin: 0;
    max-width: 420px;
  }
  .message button {
    margin-top: 8px;
    display: inline-flex;
    align-items: center;
    gap: 6px;
  }
</style>
