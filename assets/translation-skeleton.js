// Measure the source with the WebView's own wrapping, font and Unicode layout.
window.ltguiSkeletons ??= new Map();
const existing = window.ltguiSkeletons.get(skeletonId);
if (existing) {
    existing.refresh();
} else {
    const skeleton = document.getElementById(skeletonId);
    const input = document.getElementById('translator-input');
    if (skeleton && input) {
        const mirror = document.createElement('div');
        mirror.setAttribute('aria-hidden', 'true');
        Object.assign(mirror.style, {
            position: 'fixed', left: '0', top: '0',
            visibility: 'hidden', pointerEvents: 'none', padding: '0',
            border: '0', margin: '0', boxSizing: 'content-box',
        });
        document.body.appendChild(mirror);
        let frame = 0;
        let disposed = false;
        const dispose = () => {
            if (disposed) return;
            disposed = true;
            cancelAnimationFrame(frame);
            observer.disconnect();
            input.removeEventListener('input', refresh);
            mirror.remove();
            window.ltguiSkeletons.delete(skeletonId);
        };
        const measure = () => {
            frame = 0;
            if (!skeleton.isConnected || !input.isConnected) return dispose();
            const style = getComputedStyle(input);
            const width = input.clientWidth - parseFloat(style.paddingLeft) - parseFloat(style.paddingRight);
            if (width <= 0) return;
            for (const property of ['font', 'fontFamily', 'fontSize', 'fontWeight', 'fontStyle',
                'fontStretch', 'fontKerning', 'fontFeatureSettings', 'fontVariationSettings',
                'lineHeight', 'letterSpacing', 'wordSpacing', 'tabSize', 'textIndent',
                'textTransform', 'direction', 'textAlign', 'wordBreak', 'overflowWrap', 'whiteSpace']) {
                mirror.style[property] = style[property];
            }
            mirror.style.width = `${width}px`;
            mirror.textContent = input.value + '\u200b'; // Include a trailing empty line.
            const bounds = mirror.getBoundingClientRect();
            const lineHeight = parseFloat(style.lineHeight) || parseFloat(style.fontSize) * 1.7;
            const total = Math.max(1, Math.round(bounds.height / lineHeight));
            const visible = Math.max(1, Math.floor(skeleton.clientHeight / lineHeight));
            const count = Math.min(total, visible, 100);
            const lines = Array.from({length: count}, () => ({left: Infinity, right: -Infinity}));
            const range = document.createRange();
            range.selectNodeContents(mirror);
            for (const rect of range.getClientRects()) {
                const index = Math.round((rect.top - bounds.top) / lineHeight);
                if (index >= 0 && index < count && rect.width > 0) {
                    lines[index].left = Math.min(lines[index].left, rect.left);
                    lines[index].right = Math.max(lines[index].right, rect.right);
                }
            }
            skeleton.style.setProperty('--skeleton-line-height', `${lineHeight}px`);
            const fragment = document.createDocumentFragment();
            for (const line of lines) {
                const bar = document.createElement('span');
                const length = Number.isFinite(line.left) ? line.right - line.left : 0;
                bar.style.inlineSize = `${length > 0 ? Math.max(2, Math.min(100, length / width * 100)) : 0}%`;
                fragment.appendChild(bar);
            }
            skeleton.replaceChildren(fragment);
        };
        function refresh() {
            if (!disposed && !frame) frame = requestAnimationFrame(measure);
        }
        const observer = new ResizeObserver(refresh);
        observer.observe(input);
        observer.observe(skeleton);
        input.addEventListener('input', refresh);
        window.ltguiSkeletons.set(skeletonId, {refresh, dispose});
        document.fonts.ready.then(refresh);
        refresh();
    }
}
