// Keep keyboard interactions immediate, including when they interrupt pointer motion.
(() => {
  const root = document.documentElement;
  if (root.dataset.inputModality) return;
  root.dataset.inputModality = 'pointer';
  const setModality = modality => {
    if (root.dataset.inputModality !== modality) root.dataset.inputModality = modality;
  };
  document.addEventListener('pointerdown', () => {
    setModality('pointer');
  }, true);
  document.addEventListener('keydown', () => {
    setModality('keyboard');
  }, true);
})();
