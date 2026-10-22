/** Keeps the mouse wheel from scrolling what is behind an open dialog.
 *
 * WebView2 can hand a wheel over a dialog's backdrop, or the end of a
 * dialog's own scrolling, to the message list underneath. While a dialog is
 * open, a wheel may only scroll something inside a dialog or a menu. */
export function guardModalScroll() {
  window.addEventListener(
    "wheel",
    (event) => {
      if (event.ctrlKey || !document.querySelector('[aria-modal="true"]')) return;
      const scroller = scrollerFor(event.target, event.deltaX, event.deltaY);
      if (!scroller?.closest('[aria-modal="true"], [role="menu"]')) event.preventDefault();
    },
    // Not passive, or it couldn't cancel the scroll.
    { passive: false, capture: true },
  );
}

/** The element a wheel at `target` would scroll, if any. */
function scrollerFor(target: EventTarget | null, dx: number, dy: number): Element | null {
  for (let el = target instanceof Element ? target : null; el; el = el.parentElement) {
    const style = getComputedStyle(el);
    const y = /auto|scroll/.test(style.overflowY) && el.scrollHeight > el.clientHeight;
    const x = /auto|scroll/.test(style.overflowX) && el.scrollWidth > el.clientWidth;
    if (y && dy && (dy < 0 ? el.scrollTop > 0 : el.scrollTop + el.clientHeight < el.scrollHeight - 1)) return el;
    if (x && dx && (dx < 0 ? el.scrollLeft > 0 : el.scrollLeft + el.clientWidth < el.scrollWidth - 1)) return el;
  }
  return null;
}
