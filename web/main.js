// Hive UI: renders the board as SVG and talks to the engine worker.

const R = 36; // hex radius (center to corner) in world units
const SQ3 = Math.sqrt(3);
const SVGNS = 'http://www.w3.org/2000/svg';
const AI_DELAY_MS = 350;

const BUG_NAMES = { Q: '여왕벌', S: '거미', B: '딱정벌레', G: '메뚜기', A: '개미' };
const BUG_ORDER = ['Q', 'S', 'B', 'G', 'A'];
const COLOR_KO = { White: '백', Black: '흑' };

const $ = (id) => document.getElementById(id);
const svg = $('board');
const world = $('world');
const cellsLayer = $('cells');
const targetsLayer = $('targets');

// ------------------------------------------------------------------ worker

const worker = new Worker(new URL('./worker.js', import.meta.url), { type: 'module' });
let nextId = 1;
const pending = new Map();
worker.onmessage = (e) => {
  const { id, ok, result, error } = e.data;
  const p = pending.get(id);
  pending.delete(id);
  if (ok) p.resolve(result);
  else p.reject(new Error(error));
};
worker.onerror = (e) => setStatus(`엔진 오류: ${e.message || e}`);
function call(cmd, args) {
  const id = nextId++;
  return new Promise((resolve, reject) => {
    pending.set(id, { resolve, reject });
    worker.postMessage({ id, cmd, args });
  });
}

// ------------------------------------------------------------------- state

let snap = null; // { state, moves }
let human = 'White';
let selected = null; // { piece, from: [q,r] | null }
let busy = false; // waiting on the engine
let view = { x: 0, y: 0, k: 1 };

const pieceColor = (name) => (name[0] === 'w' ? 'White' : 'Black');
const bugOf = (name) => name[1];
const humanTurn = () => snap && snap.state.toMove === human && !isOver();
const isOver = () => ['WhiteWins', 'BlackWins', 'Draw'].includes(snap.state.status);

function hexToPixel(q, r) {
  return [R * SQ3 * (q + r / 2), R * 1.5 * r];
}

function hexPoints(cx, cy, rad) {
  const pts = [];
  for (let i = 0; i < 6; i++) {
    const a = (Math.PI / 180) * (60 * i - 30);
    pts.push(`${(cx + rad * Math.cos(a)).toFixed(2)},${(cy + rad * Math.sin(a)).toFixed(2)}`);
  }
  return pts.join(' ');
}

function el(tag, attrs = {}, parent) {
  const n = document.createElementNS(SVGNS, tag);
  for (const [k, v] of Object.entries(attrs)) n.setAttribute(k, v);
  if (parent) parent.appendChild(n);
  return n;
}

// --------------------------------------------------------------- rendering

function render() {
  if (!snap) return;
  renderBoard();
  renderHands();
  renderLog();
  renderStatus();
  $('pass').disabled = !(humanTurn() && snap.moves.length === 1 && snap.moves[0].uhp === 'pass');
  $('undo').disabled = busy || snap.state.ply === 0;
}

function renderBoard() {
  cellsLayer.replaceChildren();
  targetsLayer.replaceChildren();
  const { cells, lastFrom, lastTo } = snap.state;
  const same = (a, q, r) => a && a[0] === q && a[1] === r;

  if (lastFrom && !cells.some((c) => same(lastFrom, c.q, c.r))) {
    const [x, y] = hexToPixel(lastFrom[0], lastFrom[1]);
    el('polygon', { points: hexPoints(x, y, R - 3), class: 'last-from' }, cellsLayer);
  }

  for (const c of cells) {
    const [x, y] = hexToPixel(c.q, c.r);
    const top = c.stack[c.stack.length - 1];
    const g = el('g', { class: 'cell', 'data-q': c.q, 'data-r': c.r }, cellsLayer);
    // Stack shadow layers hint at height.
    for (let i = c.stack.length - 1; i > 0; i--) {
      el('polygon', { points: hexPoints(x + i * 3, y + i * 3, R - 2), class: `under ${pieceColor(c.stack[i - 1])}` }, g);
    }
    const isSel = selected && selected.from && same(selected.from, c.q, c.r);
    const cls = ['piece', pieceColor(top), `bug-${bugOf(top)}`];
    if (isSel) cls.push('selected');
    if (same(lastTo, c.q, c.r)) cls.push('last');
    el('polygon', { points: hexPoints(x, y, R - 2), class: cls.join(' ') }, g);
    el('circle', { cx: x, cy: y, r: R * 0.42, class: `badge bug-${bugOf(top)}` }, g);
    const t = el('text', { x, y: y + 1, class: `label ${pieceColor(top)}` }, g);
    t.textContent = top.slice(1);
    if (c.stack.length > 1) {
      el('circle', { cx: x + R * 0.58, cy: y - R * 0.5, r: 10, class: 'height' }, g);
      const h = el('text', { x: x + R * 0.58, y: y - R * 0.5 + 1, class: 'height-text' }, g);
      h.textContent = c.stack.length;
    }
    const title = el('title', {}, g);
    title.textContent = c.stack.join(' → ');
  }

  if (selected) {
    for (const m of movesOfSelected()) {
      const [q, r] = m.to;
      const [x, y] = hexToPixel(q, r);
      const occupied = cells.some((c) => c.q === q && c.r === r);
      el('polygon', {
        points: hexPoints(x, y, R - 4),
        class: occupied ? 'target over' : 'target',
        'data-q': q,
        'data-r': r,
        'data-target': '1',
      }, targetsLayer);
    }
  }
  applyView();
  keepInView();
}

// Refit when the hive grows past the visible area (e.g. after an AI move).
function keepInView() {
  if (pointers.size > 0 || snap.state.cells.length === 0) return;
  const rect = svg.getBoundingClientRect();
  const b = cellsLayer.getBoundingClientRect();
  const m = 4;
  if (b.left < rect.left - m || b.top < rect.top - m || b.right > rect.right + m || b.bottom > rect.bottom + m) fit();
}

function renderHands() {
  for (const color of ['White', 'Black']) {
    const box = document.querySelector(`#hand-${color} .pieces`);
    box.replaceChildren();
    const hand = snap.state.hands[color];
    for (const bug of BUG_ORDER) {
      const left = hand.filter((p) => bugOf(p) === bug);
      if (left.length === 0) continue;
      const next = left[0];
      const b = document.createElement('button');
      b.className = `hand-piece ${color} bug-${bug}`;
      const placeable = color === human && humanTurn() && snap.moves.some((m) => m.piece === next && !m.from);
      b.disabled = !placeable;
      if (color !== human) b.classList.add('opponent');
      if (selected && selected.piece === next && !selected.from) b.classList.add('selected');
      b.innerHTML = `<span class="glyph">${bug}</span><span class="name">${BUG_NAMES[bug]}</span><span class="count">×${left.length}</span>`;
      b.addEventListener('click', () => selectPiece(next, null));
      box.appendChild(b);
    }
    if (hand.length === 0) box.textContent = '없음';
  }
}

function renderLog() {
  const ol = $('moves');
  ol.replaceChildren();
  const h = snap.state.history;
  for (let i = 0; i < h.length; i += 2) {
    const li = document.createElement('li');
    li.innerHTML = `<span class="n"></span><span class="w"></span><span class="b"></span>`;
    li.children[0].textContent = `${i / 2 + 1}.`;
    li.children[1].textContent = h[i];
    li.children[2].textContent = h[i + 1] || '';
    ol.appendChild(li);
  }
  ol.parentElement.scrollTop = ol.parentElement.scrollHeight;
}

function renderStatus() {
  const s = snap.state;
  let text;
  if (s.status === 'WhiteWins' || s.status === 'BlackWins') {
    const w = s.status === 'WhiteWins' ? 'White' : 'Black';
    text = `${COLOR_KO[w]} 승리 — ${w === human ? '이겼다' : '졌다'}`;
  } else if (s.status === 'Draw') {
    text = '무승부 (양쪽 여왕벌 동시 포위)';
  } else {
    const who = s.toMove === human ? '내 차례' : 'AI 차례';
    text = `${who} · ${COLOR_KO[s.toMove]} ${s.turn}수째`;
    if (s.toMove === human && snap.moves.length === 1 && snap.moves[0].uhp === 'pass') text += ' · 둘 곳이 없어 패스해야 한다';
    else if (s.toMove === human && s.turn === 4 && snap.state.hands[human].includes(human[0].toLowerCase() + 'Q')) text += ' · 이번에 여왕벌을 놓아야 한다';
  }
  setStatus(text);
}

function setStatus(t) {
  $('status').textContent = t;
}

// --------------------------------------------------------------- selection

function movesOfSelected() {
  if (!selected) return [];
  return snap.moves.filter((m) => m.piece === selected.piece && (selected.from ? m.from && m.from[0] === selected.from[0] && m.from[1] === selected.from[1] : !m.from));
}

function selectPiece(piece, from) {
  if (!humanTurn() || busy) return;
  if (selected && selected.piece === piece) selected = null;
  else selected = { piece, from };
  if (selected && movesOfSelected().length === 0) selected = null;
  render();
}

function onBoardTap(target) {
  if (!snap || !humanTurn() || busy) return;
  const node = target.closest('[data-q]');
  if (!node) {
    selected = null;
    render();
    return;
  }
  const q = +node.dataset.q;
  const r = +node.dataset.r;
  if (node.dataset.target) {
    const m = movesOfSelected().find((mv) => mv.to[0] === q && mv.to[1] === r);
    if (m) {
      play(m.uhp);
      return;
    }
  }
  const cell = snap.state.cells.find((c) => c.q === q && c.r === r);
  if (cell) {
    const top = cell.stack[cell.stack.length - 1];
    if (pieceColor(top) === human) {
      selectPiece(top, [q, r]);
      return;
    }
  }
  selected = null;
  render();
}

// ----------------------------------------------------------------- actions

async function play(uhp) {
  selected = null;
  busy = true;
  try {
    snap = await call('play', { move: uhp });
  } catch (e) {
    setStatus(`둘 수 없는 수: ${e.message}`);
  }
  busy = false;
  render();
  maybeAiMove();
}

// Bumped by new game / undo so a pending AI move from an older position is dropped.
let generation = 0;

async function maybeAiMove() {
  if (!snap || isOver() || snap.state.toMove === human) return;
  const gen = generation;
  busy = true;
  render();
  await new Promise((res) => setTimeout(res, AI_DELAY_MS));
  if (gen !== generation) return;
  const next = await call('random');
  if (gen !== generation) return;
  snap = next;
  busy = false;
  render();
}

async function newGame() {
  generation++;
  human = $('color').value;
  selected = null;
  busy = true;
  snap = await call('new', { tournament: $('tournament').checked });
  busy = false;
  view = { x: 0, y: 0, k: 1 };
  render();
  fit();
  maybeAiMove();
}

async function undo() {
  if (busy || !snap || snap.state.ply === 0) return;
  selected = null;
  const n = Math.min(snap.state.toMove === human ? 2 : 1, snap.state.ply);
  generation++;
  busy = true;
  snap = await call('undo', { n });
  busy = false;
  render();
  maybeAiMove();
}

$('new').addEventListener('click', newGame);
$('undo').addEventListener('click', undo);
$('pass').addEventListener('click', () => play('pass'));
$('fit').addEventListener('click', fit);
$('copy').addEventListener('click', async () => {
  if (!snap) return;
  try {
    await navigator.clipboard.writeText(snap.state.gameString);
    setStatus('기보(UHP GameString)를 복사했다');
  } catch {
    prompt('기보', snap.state.gameString);
  }
});

// --------------------------------------------------------- pan / zoom view

function applyView() {
  world.setAttribute('transform', `translate(${view.x} ${view.y}) scale(${view.k})`);
}

function fit() {
  const rect = svg.getBoundingClientRect();
  const cells = snap ? snap.state.cells : [];
  const pts = cells.map((c) => hexToPixel(c.q, c.r));
  if (pts.length === 0) pts.push([0, 0]);
  const xs = pts.map((p) => p[0]);
  const ys = pts.map((p) => p[1]);
  const pad = R * 3;
  const w = Math.max(...xs) - Math.min(...xs) + pad * 2;
  const h = Math.max(...ys) - Math.min(...ys) + pad * 2;
  const k = Math.min(1.6, Math.max(0.3, Math.min(rect.width / w, rect.height / h)));
  const cx = (Math.max(...xs) + Math.min(...xs)) / 2;
  const cy = (Math.max(...ys) + Math.min(...ys)) / 2;
  view = { k, x: rect.width / 2 - cx * k, y: rect.height / 2 - cy * k };
  applyView();
}

function zoomAt(px, py, factor) {
  const k = Math.min(3, Math.max(0.25, view.k * factor));
  const f = k / view.k;
  view = { k, x: px - (px - view.x) * f, y: py - (py - view.y) * f };
  applyView();
}

const pointers = new Map();
let gesture = null; // { moved, startX, startY, viewX, viewY, pinchDist, pinchK }

function localPoint(e) {
  const rect = svg.getBoundingClientRect();
  return [e.clientX - rect.left, e.clientY - rect.top];
}

svg.addEventListener('pointerdown', (e) => {
  svg.setPointerCapture(e.pointerId);
  pointers.set(e.pointerId, localPoint(e));
  if (pointers.size === 1) {
    const [x, y] = localPoint(e);
    gesture = { moved: false, startX: x, startY: y, viewX: view.x, viewY: view.y, target: e.target };
  } else if (pointers.size === 2) {
    const [a, b] = [...pointers.values()];
    gesture = {
      moved: true,
      pinchDist: Math.hypot(a[0] - b[0], a[1] - b[1]),
      mid: [(a[0] + b[0]) / 2, (a[1] + b[1]) / 2],
      view: { ...view },
    };
  }
});

svg.addEventListener('pointermove', (e) => {
  if (!pointers.has(e.pointerId) || !gesture) return;
  pointers.set(e.pointerId, localPoint(e));
  if (pointers.size === 1 && gesture.startX !== undefined) {
    const [x, y] = localPoint(e);
    const dx = x - gesture.startX;
    const dy = y - gesture.startY;
    if (!gesture.moved && Math.hypot(dx, dy) > 8) gesture.moved = true;
    if (gesture.moved) {
      view.x = gesture.viewX + dx;
      view.y = gesture.viewY + dy;
      applyView();
    }
  } else if (pointers.size === 2 && gesture.pinchDist) {
    const [a, b] = [...pointers.values()];
    const dist = Math.hypot(a[0] - b[0], a[1] - b[1]);
    const mid = [(a[0] + b[0]) / 2, (a[1] + b[1]) / 2];
    const k = Math.min(3, Math.max(0.25, gesture.view.k * (dist / gesture.pinchDist)));
    const f = k / gesture.view.k;
    view = {
      k,
      x: mid[0] - (gesture.mid[0] - gesture.view.x) * f,
      y: mid[1] - (gesture.mid[1] - gesture.view.y) * f,
    };
    applyView();
  }
});

function endPointer(e) {
  if (!pointers.has(e.pointerId)) return;
  pointers.delete(e.pointerId);
  if (pointers.size === 0) {
    if (gesture && !gesture.moved && e.type === 'pointerup') onBoardTap(gesture.target);
    gesture = null;
  } else if (pointers.size === 1) {
    // Pinch ended with one finger still down: continue as a pan, never a tap.
    const [x, y] = [...pointers.values()][0];
    gesture = { moved: true, startX: x, startY: y, viewX: view.x, viewY: view.y };
  }
}
svg.addEventListener('pointerup', endPointer);
svg.addEventListener('pointercancel', endPointer);

svg.addEventListener('wheel', (e) => {
  e.preventDefault();
  const [x, y] = localPoint(e);
  zoomAt(x, y, Math.exp(-e.deltaY * 0.0015));
}, { passive: false });

window.addEventListener('keydown', (e) => {
  if (e.key === 'Escape') {
    selected = null;
    render();
  } else if ((e.ctrlKey || e.metaKey) && e.key === 'z') {
    e.preventDefault();
    undo();
  }
});

let resizeTimer = null;
window.addEventListener('resize', () => {
  clearTimeout(resizeTimer);
  resizeTimer = setTimeout(fit, 150);
});

newGame().catch((e) => setStatus(`엔진을 불러오지 못했다: ${e.message}`));
