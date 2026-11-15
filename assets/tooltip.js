(() => {
  const tip = document.getElementById(tooltipId);
  const selector = typeof tooltipAnchorSelector === 'string' ? tooltipAnchorSelector : `[data-tooltip-anchor="${tooltipId}"]`;
  const anchor = document.querySelector(selector);
  if (!tip || !anchor) { dioxus.send(null); return; }
  let frame;
  let ended = false;
  let previous = '';
  const dismiss = () => {
    if (ended) return;
    ended = true;
    tip.style.visibility = 'hidden';
    cancelAnimationFrame(frame);
    document.removeEventListener('pointerdown', dismiss, true);
    document.removeEventListener('visibilitychange', visibility);
    window.removeEventListener('blur', dismiss);
    dioxus.send(null);
  };
  const visibility = () => { if (document.hidden) dismiss(); };
  document.addEventListener('pointerdown', dismiss, true);
  document.addEventListener('visibilitychange', visibility);
  window.addEventListener('blur', dismiss);
  const position = () => {
    if (ended) return;
    if (!tip.isConnected || !anchor.isConnected || tip.dataset.state === 'closed') { dismiss(); return; }
    const keyboardFocus = anchor.matches(':focus-visible') || (anchor.contains(document.activeElement) && document.activeElement?.matches(':focus-visible'));
    if (!anchor.matches(':hover') && !keyboardFocus) { dismiss(); return; }
    const rect = anchor.getBoundingClientRect();
    if (rect.bottom <= 0 || rect.top >= innerHeight || rect.right <= 0 || rect.left >= innerWidth) { dismiss(); return; }
    const size = tip.getBoundingClientRect();
    const margin = 8, gap = 8;
    const preview = tip.classList.contains('instance-tooltip');
    let left, top;
    if (preview) {
      left = rect.right + gap + size.width <= innerWidth - margin ? rect.right + gap : rect.left - size.width - gap;
      top = rect.top + (rect.height - size.height) / 2;
      tip.dataset.side = left >= rect.right ? 'right' : 'left';
    } else {
      left = rect.left + (rect.width - size.width) / 2;
      const above = rect.top - size.height - gap;
      top = above >= margin ? above : rect.bottom + gap;
      tip.dataset.side = above >= margin ? 'top' : 'bottom';
    }
    left = Math.max(margin, Math.min(left, innerWidth - size.width - margin));
    top = Math.max(margin, Math.min(top, innerHeight - size.height - margin));
    const placement = `${left},${top}`;
    if (placement !== previous) {
      tip.style.left = `${left}px`;
      tip.style.top = `${top}px`;
      previous = placement;
    }
    tip.style.visibility = 'visible';
    frame = requestAnimationFrame(position);
  };
  frame = requestAnimationFrame(position);
})();
