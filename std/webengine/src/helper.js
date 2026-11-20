// SPDX-License-Identifier: Apache-2.0
// NEOWEBE's page helper (browser WB5c), injected into every page as a user script: what a long-press hit, word
// selection with handles, and find in page. Servo 0.8 has no caretPositionFromPoint, so characters are hit-tested
// with Range rectangles. Points come in device pixels of the page area and go back the same way.
(() => {
  if (window.__neoweb) return;
  const dpr = () => window.devicePixelRatio || 1;
  const css = (x, y) => [x / dpr(), y / dpr()];

  // the text node and offset under (x, y) css px, or null
  function caretAt(x, y) {
    const el = document.elementFromPoint(x, y);
    if (!el) return null;
    const walk = document.createTreeWalker(el, NodeFilter.SHOW_TEXT);
    const range = document.createRange();
    let best = null, bestD = Infinity;
    for (let n = walk.nextNode(); n; n = walk.nextNode()) {
      for (let i = 0; i < n.length; i++) {
        range.setStart(n, i);
        range.setEnd(n, i + 1);
        const r = range.getBoundingClientRect();
        if (!r.width && !r.height) continue;
        if (y >= r.top && y <= r.bottom && x >= r.left && x <= r.right)
          return { node: n, offset: x < (r.left + r.right) / 2 ? i : i + 1 };
        const d = Math.abs((r.top + r.bottom) / 2 - y) * 4 + Math.abs((r.left + r.right) / 2 - x);
        if (d < bestD) {
          bestD = d;
          best = { node: n, offset: x < (r.left + r.right) / 2 ? i : i + 1 };
        }
      }
    }
    return best;
  }

  const word = /[\p{L}\p{N}_'’-]/u;
  // the selection's ends as [ax, ay, bx, by, lineHeight] device px (the ends' tops), or [] when nothing is selected
  function ends() {
    const s = getSelection();
    if (!s.rangeCount || s.isCollapsed) return [];
    const rects = s.getRangeAt(0).getClientRects();
    if (!rects.length) return [];
    const a = rects[0], b = rects[rects.length - 1];
    return [a.left * dpr(), a.top * dpr(), b.right * dpr(), b.top * dpr(), (b.bottom - b.top) * dpr()];
  }

  let matches = [], current = -1, query = '';
  function findAll(q) {
    matches = [];
    if (!q) return;
    const needle = q.toLocaleLowerCase();
    const walk = document.createTreeWalker(document.body, NodeFilter.SHOW_TEXT);
    for (let n = walk.nextNode(); n; n = walk.nextNode()) {
      const p = n.parentElement;
      if (p && /^(SCRIPT|STYLE|NOSCRIPT)$/.test(p.tagName)) continue;
      const hay = n.data.toLocaleLowerCase();
      for (let i = hay.indexOf(needle); i >= 0; i = hay.indexOf(needle, i + needle.length))
        matches.push([n, i, i + q.length]);
      if (matches.length >= 1000) break; // shortcut: a cap for huge pages
    }
  }

  window.__neoweb = {
    // a long-press at (x, y): a link's [1, title, url, top], an image's [2, alt, src, top], or a word selected
    // ([3, ...ends]), else [0]
    press(x, y) {
      [x, y] = css(x, y);
      const el = document.elementFromPoint(x, y);
      const a = el && el.closest('a[href]');
      if (a) return [1, (a.textContent || '').trim().slice(0, 200), a.href, a.getBoundingClientRect().bottom * dpr()];
      const img = el && el.closest('img[src]');
      if (img) return [2, img.alt || '', img.src, img.getBoundingClientRect().bottom * dpr()];
      const c = caretAt(x, y);
      if (!c) return [0];
      const t = c.node.data;
      let s = Math.min(c.offset, t.length - 1), e = s;
      if (!word.test(t[s] || '')) return [0];
      while (s > 0 && word.test(t[s - 1])) s--;
      while (e < t.length && word.test(t[e])) e++;
      getSelection().setBaseAndExtent(c.node, s, c.node, e);
      return [3, ...ends()];
    },
    // move a selection end (0 the start, 1 the end) to (x, y)
    drag(which, x, y) {
      [x, y] = css(x, y);
      const s = getSelection(), c = caretAt(x, y);
      if (!s.rangeCount || !c) return ends();
      const r = s.getRangeAt(0);
      if (which === 0) s.setBaseAndExtent(c.node, c.offset, r.endContainer, r.endOffset);
      else s.setBaseAndExtent(r.startContainer, r.startOffset, c.node, c.offset);
      return ends();
    },
    selectAll() {
      getSelection().selectAllChildren(document.body);
      return ends();
    },
    text() {
      return getSelection().toString();
    },
    clear() {
      getSelection().removeAllRanges();
      return [];
    },
    // find `q` (dir 0: from the top, 1 next, -1 previous; '' stops): [index (1-based), count, ...ends]
    find(q, dir) {
      if (q !== query || dir === 0) {
        query = q;
        findAll(q);
        current = -1;
      }
      if (!matches.length) {
        getSelection().removeAllRanges();
        return [0, 0];
      }
      current = (current + (dir < 0 ? -1 : 1) + matches.length) % matches.length;
      const [n, s, e] = matches[current];
      getSelection().setBaseAndExtent(n, s, n, e);
      n.parentElement && n.parentElement.scrollIntoView({ block: 'center' });
      return [current + 1, matches.length, ...ends()];
    },
  };
})();
