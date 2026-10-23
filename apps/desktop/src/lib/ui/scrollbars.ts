/** Scroll bars that float over the content.
 *
 * Native scroll bars take room from the box they scroll, so a list that grows
 * long enough to scroll gets narrower and everything on its right edge moves.
 * app.css hides them, and this draws a thumb of our own on top instead: one
 * per scrolling box, in a fixed layer above the page. A thumb shows while the
 * pointer is over its box, and for a moment after the box scrolls; it can be
 * dragged like a normal one. Works for any box with overflow auto or scroll,
 * textareas included, with no markup of its own. */

/** Thickness of a thumb, including its transparent edge. */
const SIZE = 10;
/** Shortest a thumb gets, so a very long list still has one to grab. */
const MIN_LENGTH = 32;
/** How long a thumb stays after its box last scrolled. */
const LINGER_MS = 1000;

type Axis = "y" | "x";

interface Bar {
  box: Element;
  thumbs: Record<Axis, HTMLDivElement>;
  /** Until when it shows after a scroll, in performance.now() time. */
  until: number;
  /** When it last had a reason to show. */
  seen: number;
  dragging: boolean;
}

const bars = new Map<Element, Bar>();
/** The scrolling boxes under the pointer, innermost first. */
let hovered: Element[] = [];
let frame = 0;

export function floatScrollbars() {
  document.addEventListener(
    "pointermove",
    (event) => {
      // Over a thumb, keep its box hovered so the thumb doesn't fade away.
      if ((event.target as Element | null)?.classList?.contains("scroll-thumb")) return;
      hovered = scrollersAt(event.target);
      redraw();
    },
    { passive: true, capture: true },
  );
  // Leaving the window, or moving into a message's frame.
  document.addEventListener("pointerout", (event) => {
    if (!event.relatedTarget) {
      hovered = [];
      redraw();
    }
  });
  document.addEventListener(
    "scroll",
    (event) => {
      const box = event.target;
      if (!(box instanceof Element) || !scrolls(box)) return;
      barFor(box).until = performance.now() + LINGER_MS;
      redraw();
    },
    { passive: true, capture: true },
  );
  window.addEventListener("resize", redraw);
}

/** The boxes that scroll along some axis, from `target` outwards. */
function scrollersAt(target: EventTarget | null): Element[] {
  const found: Element[] = [];
  for (let el = target instanceof Element ? target : null; el; el = el.parentElement) {
    if (el !== document.documentElement && el !== document.body && scrolls(el)) found.push(el);
  }
  return found;
}

function scrolls(el: Element): boolean {
  return canScroll(el, "y") || canScroll(el, "x");
}

function canScroll(el: Element, axis: Axis): boolean {
  const style = getComputedStyle(el);
  const overflow = axis === "y" ? style.overflowY : style.overflowX;
  if (!/auto|scroll/.test(overflow)) return false;
  return axis === "y" ? el.scrollHeight > el.clientHeight + 1 : el.scrollWidth > el.clientWidth + 1;
}

function barFor(box: Element): Bar {
  let bar = bars.get(box);
  if (!bar) {
    bar = { box, thumbs: { y: makeThumb(box, "y"), x: makeThumb(box, "x") }, until: 0, seen: performance.now(), dragging: false };
    bars.set(box, bar);
  }
  return bar;
}

function makeThumb(box: Element, axis: Axis): HTMLDivElement {
  const thumb = document.createElement("div");
  thumb.className = "scroll-thumb";
  thumb.setAttribute("aria-hidden", "true");
  thumb.addEventListener("pointerdown", (event) => startDrag(event, box, axis, thumb));
  // The thumb sits outside its box, so pass the wheel on.
  thumb.addEventListener(
    "wheel",
    (event) => {
      event.preventDefault();
      box.scrollBy({ left: event.deltaX, top: event.deltaY });
    },
    { passive: false },
  );
  document.body.append(thumb);
  return thumb;
}

function startDrag(event: PointerEvent, box: Element, axis: Axis, thumb: HTMLDivElement) {
  if (event.button !== 0) return;
  event.preventDefault();
  event.stopPropagation();
  const bar = barFor(box);
  const start = axis === "y" ? event.clientY : event.clientX;
  const from = axis === "y" ? box.scrollTop : box.scrollLeft;
  const view = axis === "y" ? box.clientHeight : box.clientWidth;
  const content = axis === "y" ? box.scrollHeight : box.scrollWidth;
  const length = thumbLength(view, content);
  // Content pixels per pixel of thumb travel.
  const rate = (content - view) / Math.max(1, view - length);
  bar.dragging = true;
  thumb.classList.add("dragging");
  thumb.setPointerCapture(event.pointerId);
  const move = (e: PointerEvent) => {
    const to = from + ((axis === "y" ? e.clientY : e.clientX) - start) * rate;
    if (axis === "y") box.scrollTop = to;
    else box.scrollLeft = to;
  };
  const end = () => {
    bar.dragging = false;
    bar.until = performance.now() + LINGER_MS;
    thumb.classList.remove("dragging");
    thumb.removeEventListener("pointermove", move);
    thumb.removeEventListener("pointerup", end);
    thumb.removeEventListener("pointercancel", end);
    redraw();
  };
  thumb.addEventListener("pointermove", move);
  thumb.addEventListener("pointerup", end);
  thumb.addEventListener("pointercancel", end);
}

function thumbLength(view: number, content: number): number {
  return Math.min(view, Math.max(MIN_LENGTH, (view * view) / content));
}

function redraw() {
  if (!frame) frame = requestAnimationFrame(draw);
}

/** Places every thumb; keeps going while any shows, so they follow content
 * that grows or moves without a scroll. */
function draw() {
  frame = 0;
  for (const box of hovered) barFor(box);
  const now = performance.now();
  let showing = false;
  for (const [box, bar] of bars) {
    if (!box.isConnected) {
      bar.thumbs.y.remove();
      bar.thumbs.x.remove();
      bars.delete(box);
      continue;
    }
    const wanted = bar.dragging || now < bar.until || hovered.includes(box);
    if (wanted) bar.seen = now;
    const yShown = wanted && place(bar, "y");
    const xShown = wanted && place(bar, "x");
    bar.thumbs.y.classList.toggle("shown", yShown);
    bar.thumbs.x.classList.toggle("shown", xShown);
    showing ||= yShown || xShown;
    // Forget boxes left alone for a while; their thumbs have faded by then.
    if (now - bar.seen > 2000) {
      bar.thumbs.y.remove();
      bar.thumbs.x.remove();
      bars.delete(box);
    }
  }
  if (showing) redraw();
}

/** Moves a thumb over its box; false when that axis doesn't scroll. */
function place(bar: Bar, axis: Axis): boolean {
  const box = bar.box;
  if (!canScroll(box, axis)) return false;
  const rect = box.getBoundingClientRect();
  if (!rect.width || !rect.height) return false;
  const style = bar.thumbs[axis].style;
  const top = rect.top + box.clientTop;
  const left = rect.left + box.clientLeft;
  if (axis === "y") {
    const view = box.clientHeight;
    const length = thumbLength(view, box.scrollHeight);
    const travel = (view - length) * (box.scrollTop / (box.scrollHeight - view));
    style.left = `${left + box.clientWidth - SIZE}px`;
    style.top = `${top + travel}px`;
    style.width = `${SIZE}px`;
    style.height = `${length}px`;
  } else {
    const view = box.clientWidth;
    const length = thumbLength(view, box.scrollWidth);
    const travel = (view - length) * (box.scrollLeft / (box.scrollWidth - view));
    style.left = `${left + travel}px`;
    style.top = `${top + box.clientHeight - SIZE}px`;
    style.width = `${length}px`;
    style.height = `${SIZE}px`;
  }
  return true;
}
