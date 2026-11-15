// Keep the popup within the viewport and preserve input focus when choosing an option.
// The official Combobox handles filtering, selection, and keyboard navigation.
requestAnimationFrame(() => {
  const trigger = document.getElementById(triggerId);
  const root = trigger?.closest('.dx-select');
  const popup = root?.querySelector('.dx-select-popup');
  if (!popup || popup.dataset.state !== 'open') return;
  const position = () => {
    if (!popup.isConnected || popup.dataset.state !== 'open') { cleanup(); return; }
    const rect = trigger.getBoundingClientRect();
    const margin = 12, gap = 5, cap = 320;
    const below = innerHeight - rect.bottom - gap - margin;
    const above = rect.top - gap - margin;
    const upward = below < Math.min(cap, popup.scrollHeight) && above > below;
    const height = Math.max(0, Math.min(cap, upward ? above : below));
    const width = Math.min(rect.width, innerWidth - margin * 2);
    Object.assign(popup.style, {
      width: `${width}px`, maxHeight: `${height}px`,
      left: `${Math.max(margin, Math.min(rect.left, innerWidth - width - margin))}px`,
      top: upward ? 'auto' : `${Math.max(margin, rect.bottom + gap)}px`,
      bottom: upward ? `${Math.max(margin, innerHeight - rect.top + gap)}px` : 'auto',
    });
    popup.dataset.positioned = 'true';
  };
  const close = () => { cleanup(); dioxus.send('close'); };
  const pointer = event => {
    if (!root.contains(event.target)) close();
    else if (event.target.closest('[role="option"]')) event.preventDefault();
  };
  const keyboard = event => {
    if (event.key === 'Escape') { event.preventDefault(); close(); trigger.focus(); }
  };
  const scroll = event => { if (!popup.contains(event.target)) position(); };
  const cleanup = () => {
    document.removeEventListener('pointerdown', pointer, true);
    document.removeEventListener('keydown', keyboard, true);
    document.removeEventListener('scroll', scroll, true);
    window.removeEventListener('resize', position);
    observer.disconnect();
  };
  const observer = new MutationObserver(() => {
    if (!popup.isConnected || popup.dataset.state !== 'open') cleanup();
  });
  observer.observe(root, {subtree:true, childList:true, attributes:true, attributeFilter:['data-state']});
  document.addEventListener('pointerdown', pointer, true);
  document.addEventListener('keydown', keyboard, true);
  document.addEventListener('scroll', scroll, true);
  window.addEventListener('resize', position);
  position();
  popup.querySelector('input')?.focus({preventScroll:true});
});
