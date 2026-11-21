// Drag the board sideways with the mouse, like Trello. Touch already scrolls natively.
// A drag that moved does not open the card it started on.
(() => {
  const THRESHOLD = 5;
  let drag = null;

  document.addEventListener("pointerdown", (e) => {
    if (e.pointerType !== "mouse" || e.button !== 0) return;
    const columns = e.target.closest && e.target.closest(".columns");
    if (!columns) return;
    drag = { columns, x: e.clientX, left: columns.scrollLeft, moved: false };
  });

  document.addEventListener("pointermove", (e) => {
    if (!drag) return;
    const dx = e.clientX - drag.x;
    if (!drag.moved && Math.abs(dx) < THRESHOLD) return;
    if (!drag.moved) {
      drag.moved = true;
      drag.columns.classList.add("dragging");
    }
    drag.columns.scrollLeft = drag.left - dx;
  });

  const end = () => {
    if (!drag) return;
    if (drag.moved) {
      drag.columns.classList.remove("dragging");
      const swallow = (e) => {
        e.stopPropagation();
        e.preventDefault();
      };
      window.addEventListener("click", swallow, { capture: true, once: true });
      setTimeout(() => window.removeEventListener("click", swallow, { capture: true }), 0);
    }
    drag = null;
  };
  document.addEventListener("pointerup", end);
  document.addEventListener("pointercancel", end);
})();
