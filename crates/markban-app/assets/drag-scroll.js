// Drag the board sideways with the mouse, like Trello. On release the board eases to the
// nearest column edge, taking the throw into account.
// Touch keeps native scrolling. A drag that moved does not open the card it started on.
(() => {
  const THRESHOLD = 5;
  const THROW = 200; // ms of momentum projected ahead on release
  let drag = null;
  let glide = 0;

  const stopGlide = () => {
    cancelAnimationFrame(glide);
    glide = 0;
  };

  document.addEventListener("pointerdown", (e) => {
    if (e.pointerType !== "mouse" || e.button !== 0) return;
    const columns = e.target.closest && e.target.closest(".columns");
    if (!columns) return;
    stopGlide();
    drag = { columns, x: e.clientX, left: columns.scrollLeft, moved: false, v: 0, lastX: e.clientX, lastT: e.timeStamp };
  });

  document.addEventListener("pointermove", (e) => {
    if (!drag) return;
    const dx = e.clientX - drag.x;
    if (!drag.moved && Math.abs(dx) < THRESHOLD) return;
    if (!drag.moved) {
      drag.moved = true;
      drag.columns.classList.add("dragging");
    }
    const dt = Math.max(1, e.timeStamp - drag.lastT);
    // Smoothed velocity in px per ms, positive when the content moves left.
    drag.v = 0.8 * ((drag.lastX - e.clientX) / dt) + 0.2 * drag.v;
    drag.lastX = e.clientX;
    drag.lastT = e.timeStamp;
    drag.columns.scrollLeft = drag.left - dx;
  });

  const end = (e) => {
    if (!drag) return;
    const { columns, moved } = drag;
    // A pause before release means no throw.
    const v = e.timeStamp - drag.lastT > 80 ? 0 : drag.v;
    drag = null;
    if (!moved) return;
    columns.classList.remove("dragging");
    const swallow = (ev) => {
      ev.stopPropagation();
      ev.preventDefault();
    };
    window.addEventListener("click", swallow, { capture: true, once: true });
    setTimeout(() => window.removeEventListener("click", swallow, { capture: true }), 0);
    // Ease to the column edge nearest to where the throw would have ended.
    const from = columns.scrollLeft;
    const projected = from + v * THROW;
    const base = columns.getBoundingClientRect().left - from;
    let to = from;
    let best = Infinity;
    for (const col of columns.querySelectorAll(".column")) {
      const edge = col.getBoundingClientRect().left - base;
      if (Math.abs(edge - projected) < best) {
        best = Math.abs(edge - projected);
        to = edge;
      }
    }
    to = Math.max(0, Math.min(to, columns.scrollWidth - columns.clientWidth));
    const duration = Math.min(450, 200 + Math.abs(to - from) * 0.5);
    const start = performance.now();
    const step = (now) => {
      const t = Math.min(1, (now - start) / duration);
      columns.scrollLeft = from + (to - from) * (1 - Math.pow(1 - t, 3));
      glide = t < 1 ? requestAnimationFrame(step) : 0;
    };
    glide = requestAnimationFrame(step);
  };
  document.addEventListener("pointerup", end);
  document.addEventListener("pointercancel", end);
  document.addEventListener("wheel", stopGlide, { passive: true });
})();
