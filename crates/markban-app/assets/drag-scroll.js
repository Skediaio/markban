// Drag the board sideways with the mouse, like Trello, with a short glide after release.
// Touch keeps native scrolling. A drag that moved does not open the card it started on.
(() => {
  const THRESHOLD = 5;
  const FRICTION = 0.92; // velocity kept per 16 ms frame
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
    // A pause before release means no glide.
    let v = e.timeStamp - drag.lastT > 80 ? 0 : drag.v;
    drag = null;
    if (!moved) return;
    columns.classList.remove("dragging");
    const swallow = (ev) => {
      ev.stopPropagation();
      ev.preventDefault();
    };
    window.addEventListener("click", swallow, { capture: true, once: true });
    setTimeout(() => window.removeEventListener("click", swallow, { capture: true }), 0);
    let last = performance.now();
    const step = (now) => {
      const dt = now - last;
      last = now;
      columns.scrollLeft += v * dt;
      v *= Math.pow(FRICTION, dt / 16);
      glide = Math.abs(v) > 0.02 ? requestAnimationFrame(step) : 0;
    };
    if (Math.abs(v) > 0.05) glide = requestAnimationFrame(step);
  };
  document.addEventListener("pointerup", end);
  document.addEventListener("pointercancel", end);
  document.addEventListener("wheel", stopGlide, { passive: true });
})();
