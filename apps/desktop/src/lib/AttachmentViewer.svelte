<script lang="ts">
  // A received attachment shown inside the app: pictures, PDFs and text
  // files. Everything else can be saved from here.
  import { onDestroy, onMount, tick, untrack } from "svelte";
  import type { RenderTask } from "pdfjs-dist";
  import Icon from "./Icon.svelte";
  import { api, errorText, type Attachment } from "./api";
  import { fileSize } from "./format";
  import { openPdf, type OpenPdf } from "./pdf";
  import { toast } from "./toast.svelte";
  import { mediaType, viewKind } from "./viewer";

  interface Props {
    /** The message the attachments belong to. */
    message: number;
    attachments: Attachment[];
    start: number;
    onClose: () => void;
  }
  let { message, attachments, start, onClose }: Props = $props();

  const STEPS = [0.25, 0.33, 0.5, 0.67, 0.75, 0.9, 1, 1.1, 1.25, 1.5, 1.75, 2, 2.5, 3, 4, 5];
  /** Fit to the window: pictures whole, PDFs to its width. */
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
  let viewer = $state<HTMLDivElement>();
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

  function keydown(e: KeyboardEvent) {
    const key = e.key;
    let handled = true;
    if (key === "Escape") onClose();
    else if ((key === "+" || key === "=") && kind && kind !== "text") zoomBy(1);
    else if (key === "-" && kind && kind !== "text") zoomBy(-1);
    else if (key === "0") zoomTo(FIT);
    else if (key === "r" && (kind === "image" || kind === "pdf")) rotate();
    else if (key === "ArrowLeft" && attachments.length > 1) go(-1);
    else if (key === "ArrowRight" && attachments.length > 1) go(1);
    else if ((key === "s" || key === "S") && (e.ctrlKey || e.metaKey)) save();
    else handled = false;
    if (handled) e.preventDefault();
    // The mail behind stays put: no shortcut reaches the message list.
    if (key !== "Tab") e.stopPropagation();
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

<svelte:window onkeydowncapture={keydown} />

<div class="viewer" role="dialog" aria-modal="true" aria-label={current?.name ?? "Attachment"} tabindex="-1" bind:this={viewer}>
  <header>
    <div class="name">
      <Icon name={kind === "image" ? "image" : kind ? "template" : "file"} />
      <span class="title" title={current?.name}>{current?.name}</span>
      <span class="quiet">{current ? fileSize(current.size) : ""}</span>
      {#if attachments.length > 1}
        <span class="quiet count">{index + 1} of {attachments.length}</span>
      {/if}
    </div>
    <div class="tools">
      {#if kind === "pdf" && pages.length}
        <span class="quiet pageno">Page {pageNow} / {pages.length}</span>
      {/if}
      {#if kind === "image" || kind === "pdf"}
        <button class="icon" title="Zoom out (−)" aria-label="Zoom out" onclick={() => zoomBy(-1)}>
          <Icon name="zoom-out" />
        </button>
        <button class="zoom" title="Fit to window (0)" onclick={() => zoomTo(FIT)}>{Math.round(scale * 100)}%</button>
        <button class="icon" title="Zoom in (+)" aria-label="Zoom in" onclick={() => zoomBy(1)}>
          <Icon name="zoom-in" />
        </button>
        <button class="icon" title="Fit to window (0)" aria-label="Fit to window" aria-pressed={zoom === FIT} onclick={() => zoomTo(FIT)}>
          <Icon name="fit" />
        </button>
        <button class="icon" title="Rotate (R)" aria-label="Rotate" onclick={rotate}>
          <Icon name="rotate" />
        </button>
        <span class="divider"></span>
      {/if}
      <button class="icon" title="Save (Ctrl+S)" aria-label="Save" disabled={saving} onclick={save}>
        <Icon name="save" />
      </button>
      <button class="icon" title="Close (Esc)" aria-label="Close" onclick={onClose}>
        <Icon name="close" />
      </button>
    </div>
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

  {#if attachments.length > 1}
    <button class="step prev" aria-label="Previous attachment" title="Previous (←)" disabled={index === 0} onclick={() => go(-1)}>
      <Icon name="chevron-left" size={22} />
    </button>
    <button
      class="step next"
      aria-label="Next attachment"
      title="Next (→)"
      disabled={index === attachments.length - 1}
      onclick={() => go(1)}
    >
      <Icon name="chevron-right" size={22} />
    </button>
  {/if}
</div>

<style>
  .viewer {
    position: fixed;
    inset: 0;
    z-index: 16;
    display: flex;
    flex-direction: column;
    background: var(--viewer-bg);
    color: var(--text);
    outline: none;
  }
  /* The window buttons stay usable while an attachment is open. */
  :global(:root.titlebar) .viewer {
    top: var(--titlebar-height);
  }
  header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 12px;
    padding: 6px 10px 6px 16px;
    background: var(--bg-side);
    border-bottom: 1px solid var(--border);
    min-height: 44px;
  }
  .name {
    display: flex;
    align-items: center;
    gap: 8px;
    min-width: 0;
    font-size: 14px;
  }
  .title {
    font-weight: 600;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .name .quiet {
    font-size: 12px;
    white-space: nowrap;
  }
  .count::before {
    content: "·";
    margin-right: 8px;
  }
  .tools {
    display: flex;
    align-items: center;
    gap: 2px;
    flex: none;
  }
  .pageno {
    font-size: 12px;
    margin-right: 8px;
    font-variant-numeric: tabular-nums;
  }
  .icon {
    border: none;
    background: none;
    padding: 6px;
    display: grid;
    place-items: center;
    border-radius: 6px;
  }
  .icon:hover:not(:disabled),
  .zoom:hover {
    background: var(--bg-hover);
  }
  .icon[aria-pressed="true"] {
    color: var(--accent-ink);
  }
  .zoom {
    border: none;
    background: none;
    min-width: 52px;
    font-size: 12px;
    font-variant-numeric: tabular-nums;
    padding: 6px 4px;
    border-radius: 6px;
  }
  .divider {
    width: 1px;
    height: 20px;
    background: var(--border);
    margin: 0 6px;
  }
  .stage {
    flex: 1;
    overflow: auto;
    position: relative;
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
    box-shadow: 0 4px 24px rgba(0, 0, 0, 0.35);
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
    box-shadow: 0 2px 12px rgba(0, 0, 0, 0.3);
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
  .step {
    position: absolute;
    top: 50%;
    width: 40px;
    height: 40px;
    border-radius: 50%;
    display: grid;
    place-items: center;
    padding: 0;
    background: var(--bg);
    border: 1px solid var(--border);
    box-shadow: 0 2px 10px rgba(0, 0, 0, 0.25);
    opacity: 0.85;
  }
  .step:hover:not(:disabled) {
    opacity: 1;
  }
  .step:disabled {
    opacity: 0.3;
  }
  .prev {
    left: 16px;
  }
  .next {
    right: 16px;
  }
</style>
