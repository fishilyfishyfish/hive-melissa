// Hive UI: renders the board as SVG and talks to the engine worker.

const R = 36; // hex radius (center to corner) in world units
const SQ3 = Math.sqrt(3);
const SVGNS = 'http://www.w3.org/2000/svg';
const AI_DELAY_MS = 250;

const LEVELS = {
  random: { kind: 'random' },
  easy: { kind: 'search', timeMs: 1e9, maxDepth: 1 },
  normal: { kind: 'search', timeMs: 300, maxDepth: 64 },
  hard: { kind: 'search', timeMs: 1500, maxDepth: 64 },
  max: { kind: 'search', timeMs: 5000, maxDepth: 64 },
};
const WIN = 100000;

const BUG_NAMES = { Q: '여왕벌', S: '거미', B: '딱정벌레', G: '메뚜기', A: '개미' };
const BUG_ORDER = ['Q', 'S', 'B', 'G', 'A'];
const COLOR_KO = { White: '백', Black: '흑' };

const $ = (id) => document.getElementById(id);
const svg = $('board');
const world = $('world');
const cellsLayer = $('cells');
const targetsLayer = $('targets');

// ------------------------------------------------------------------ worker

// Returns `call(cmd, args) -> Promise` for a fresh engine worker.
function startWorker() {
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
  const callFn = (cmd, args) => {
    const id = nextId++;
    return new Promise((resolve, reject) => {
      pending.set(id, { resolve, reject });
      worker.postMessage({ id, cmd, args });
    });
  };
  // Stops a long computation; pending calls fail with "cancelled".
  callFn.terminate = () => {
    worker.terminate();
    for (const p of pending.values()) p.reject(new Error('cancelled'));
    pending.clear();
  };
  return callFn;
}

// The game worker plays; analysis runs in its own worker so long analyses
// never block moves.
const call = startWorker();
const anaCall = startWorker();

// ------------------------------------------------------------------- state

let snap = null; // { state, moves }
let human = 'White';
let selected = null; // { piece, from: [q,r] | null }
let busy = false; // waiting on the engine
let view = { x: 0, y: 0, k: 1 };

let ana = null; // analysis session, see "analysis" section
let study = null; // study board session, see "study" section

// State shown on the board: the game, or the analyzed position.
function boardState() {
  if (!ana) return snap.state;
  const k = ana.showBest ? ana.ply - 1 : ana.ply;
  return ana.states.get(k) || snap.state;
}

const pieceColor = (name) => (name[0] === 'w' ? 'White' : 'Black');
const bugOf = (name) => name[1];
// The side the person plays now: both sides on the study board.
const me = () => (study ? snap.state.toMove : human);
const humanTurn = () => snap && snap.state.toMove === me() && !isOver();
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
  document.body.classList.toggle('analysis', !!ana);
  document.body.classList.toggle('study', !!study);
  renderBoard();
  renderHands();
  if (ana) {
    renderAnalysis();
  } else if (study) {
    renderStudy();
  } else {
    renderLog();
    renderStatus();
  }
  $('pass').disabled = !(humanTurn() && snap.moves.length === 1 && snap.moves[0].uhp === 'pass');
  $('undo').disabled = (busy && !thinking) || snap.state.ply === 0;
  $('hint').disabled = !humanTurn() || busy || !!ana || !!study;
  if (ana) {
    $('pass').disabled = true;
    $('undo').disabled = true;
  }
  $('analyze').classList.toggle('active', !!ana);
  $('study-btn').classList.toggle('active', !!study);
}

function renderBoard() {
  cellsLayer.replaceChildren();
  targetsLayer.replaceChildren();
  let { cells, lastFrom, lastTo } = boardState();
  const same = (a, q, r) => a && a[0] === q && a[1] === r;
  // "Better move" view: show that move's from/to instead of the last move.
  const best = ana && ana.showBest ? ana.reports[ana.ply - 1] : null;
  if (best) {
    lastFrom = best.bestFrom;
    lastTo = null;
  }
  // Study board: a candidate shown on the board.
  const preview = study && !ana ? study.preview : null;

  if (cells.length === 0) {
    el('polygon', { points: hexPoints(0, 0, R - 4), class: 'origin' }, cellsLayer);
  }

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
    const isSel = (selected && selected.from && same(selected.from, c.q, c.r))
      || (best && same(best.bestFrom, c.q, c.r))
      || (preview && same(preview.from, c.q, c.r));
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

  if (best && best.bestTo) {
    const [x, y] = hexToPixel(best.bestTo[0], best.bestTo[1]);
    el('polygon', { points: hexPoints(x, y, R - 4), class: 'target hint' }, targetsLayer);
  }
  if (preview && preview.to) {
    const [x, y] = hexToPixel(preview.to[0], preview.to[1]);
    el('polygon', { points: hexPoints(x, y, R - 4), class: 'target hint' }, targetsLayer);
    const t = el('text', { x, y: y + 1, class: 'ghost-label' }, targetsLayer);
    t.textContent = preview.piece ? preview.piece.slice(1) : '';
  }
  if (selected && !ana) {
    for (const m of movesOfSelected()) {
      const [q, r] = m.to;
      const [x, y] = hexToPixel(q, r);
      const occupied = cells.some((c) => c.q === q && c.r === r);
      const isHint = hint && hint.to && hint.to[0] === q && hint.to[1] === r && hint.piece === selected.piece;
      el('polygon', {
        points: hexPoints(x, y, R - 4),
        class: `${occupied ? 'target over' : 'target'}${isHint ? ' hint' : ''}`,
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
// Also covers highlighted target cells, so a legal cell is never off-screen.
function keepInView() {
  if (pointers.size > 0) return;
  const rect = svg.getBoundingClientRect();
  if (rect.width === 0 || rect.height === 0) return;
  const m = 4;
  for (const layer of [cellsLayer, targetsLayer]) {
    if (!layer.firstChild) continue;
    const b = layer.getBoundingClientRect();
    if (b.left < rect.left - m || b.top < rect.top - m || b.right > rect.right + m || b.bottom > rect.bottom + m) {
      fit();
      return;
    }
  }
}

function renderHands() {
  for (const color of ['White', 'Black']) {
    const box = document.querySelector(`#hand-${color} .pieces`);
    box.replaceChildren();
    const hand = boardState().hands[color];
    for (const bug of BUG_ORDER) {
      const left = hand.filter((p) => bugOf(p) === bug);
      if (left.length === 0) continue;
      const next = left[0];
      const b = document.createElement('button');
      b.className = `hand-piece ${color} bug-${bug}`;
      const placeable = !ana && color === me() && humanTurn() && snap.moves.some((m) => m.piece === next && !m.from);
      b.disabled = !placeable;
      if (color !== me()) b.classList.add('opponent');
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
    const who = s.toMove === human ? '내 차례' : thinking ? 'AI 생각 중…' : 'AI 차례';
    text = `${who} · ${COLOR_KO[s.toMove]} ${s.turn}수째`;
    if (hint) text += ` · 힌트: ${hint.uhp} (깊이 ${hint.depth}, ${describeScore(hint.score)})`;
    else if (lastAi && s.toMove === human) text += ` · AI 깊이 ${lastAi.depth}, 평가 ${describeScore(lastAi.score)}`;
    if (s.toMove === human && snap.moves.length === 1 && snap.moves[0].uhp === 'pass') text += ' · 둘 곳이 없어 패스해야 한다';
    else if (s.toMove === human && s.turn === 4 && snap.state.hands[human].includes(human[0].toLowerCase() + 'Q')) text += ' · 이번에 여왕벌을 놓아야 한다';
    else if (s.toMove === human && s.ply < 2 && !selected) text += ' · 아래 손패에서 놓을 말을 고른다';
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
  if (study) study.preview = null;
  if (selected && movesOfSelected().length === 0) selected = null;
  render();
}

function onBoardTap(target) {
  if (!snap || ana || !humanTurn() || busy) return;
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
    if (pieceColor(top) === me()) {
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
  hint = null;
  busy = true;
  try {
    snap = await call('play', { move: uhp });
    if (study) studyPlayed(uhp);
  } catch (e) {
    setStatus(`둘 수 없는 수: ${e.message}`);
  }
  busy = false;
  render();
  if (study) refreshStudy();
  maybeAiMove();
}

// Bumped by new game / undo so a pending AI move from an older position is dropped.
let generation = 0;
let thinking = false;
let lastAi = null; // last search result of the AI
let hint = null; // { uhp, from, to }

function describeScore(score) {
  if (score > WIN - 1000) return `${WIN - score}수 안에 승리`;
  if (score < -WIN + 1000) return `${WIN + score}수 안에 패배`;
  return `${score > 0 ? '+' : ''}${score}`;
}

async function maybeAiMove() {
  if (!snap || study || isOver() || snap.state.toMove === human) return;
  const gen = generation;
  busy = true;
  thinking = true;
  render();
  await new Promise((res) => setTimeout(res, AI_DELAY_MS));
  if (gen !== generation) return;
  const level = LEVELS[$('level').value];
  let choice;
  try {
    choice = await call('think', { level });
  } catch (e) {
    setStatus(`AI 오류: ${e.message}`);
    thinking = busy = false;
    return;
  }
  if (gen !== generation) return;
  thinking = false;
  lastAi = choice.depth !== undefined ? choice : null;
  snap = await call('play', { move: choice.uhp });
  if (gen !== generation) return;
  busy = false;
  render();
}

async function showHint() {
  if (!humanTurn() || busy) return;
  const gen = generation;
  busy = true;
  setStatus('힌트 계산 중…');
  const r = await call('think', { level: LEVELS.hard });
  busy = false;
  if (gen !== generation) return;
  const m = snap.moves.find((mv) => mv.uhp === r.uhp);
  hint = m ? { ...m, score: r.score, depth: r.depth } : null;
  selected = m && m.piece ? { piece: m.piece, from: m.from } : null;
  render();
}

async function newGame() {
  closeAnalysis();
  closeStudy();
  generation++;
  thinking = false;
  lastAi = null;
  hint = null;
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
  if (study) {
    studyBack();
    return;
  }
  // Undo may interrupt the AI while it thinks (its answer is then dropped).
  if ((busy && !thinking) || !snap || snap.state.ply === 0) return;
  selected = null;
  hint = null;
  lastAi = null;
  const n = Math.min(snap.state.toMove === human ? 2 : 1, snap.state.ply);
  generation++;
  thinking = false;
  busy = true;
  snap = await call('undo', { n });
  busy = false;
  render();
  maybeAiMove();
}

$('new').addEventListener('click', newGame);
$('undo').addEventListener('click', undo);
$('pass').addEventListener('click', () => play('pass'));
$('hint').addEventListener('click', showHint);
$('fit').addEventListener('click', fit);
// Move list is collapsible (collapsed by default on phones via CSS).
document.querySelector('.log h2').addEventListener('click', (e) => {
  if (e.target.id !== 'copy') document.querySelector('.log').classList.toggle('open');
});
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
  // Not laid out yet (or hidden): the ResizeObserver below refits later.
  if (rect.width === 0 || rect.height === 0) return;
  const cells = snap ? boardState().cells : [];
  const pts = cells.map((c) => hexToPixel(c.q, c.r));
  for (const m of selected && !ana ? movesOfSelected() : []) pts.push(hexToPixel(m.to[0], m.to[1]));
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

// Refit whenever the board area changes size (first layout, rotation,
// mobile address bar showing/hiding, panel reflow).
let resizeTimer = null;
const refitSoon = () => {
  clearTimeout(resizeTimer);
  resizeTimer = setTimeout(fit, 100);
};
window.addEventListener('resize', refitSoon);
if (typeof ResizeObserver !== 'undefined') new ResizeObserver(refitSoon).observe(svg);


// ---------------------------------------------------------------- analysis
//
// A session analyzes a list of UHP moves move by move in the analysis
// worker. `ply` k means "the position after k moves"; reports[k - 1]
// explains move k.

const QUALITY_MARK = {
  best: '', good: '', fine: '',
  inaccuracy: '?!', mistake: '?', blunder: '??', 'missed-win': '??',
};
let anaGen = 0;

const gameStringOf = (moves, k) => (k === 0 ? 'Base' : `Base;${moves.slice(0, k).join(';')}`);

async function ensureState(k) {
  if (!ana || ana.states.has(k)) return;
  const st = await anaCall('position', { gameString: gameStringOf(ana.moves, k), tournament: ana.tournament });
  if (ana) ana.states.set(k, st);
}

async function openAnalysis(moves, tournament) {
  closeStudy();
  const gen = ++anaGen;
  selected = null;
  hint = null;
  ana = {
    gen, moves, tournament, ply: moves.length, reports: [], states: new Map(), showBest: false,
    timeMs: +$('ana-speed').value,
  };
  $('analysis').hidden = false;
  await ensureState(ana.ply);
  render();
  fit();
  runAnalysis(gen);
}

function closeAnalysis() {
  anaGen++;
  ana = null;
  $('analysis').hidden = true;
}

async function runAnalysis(gen) {
  for (let i = 0; i < ana.moves.length; i++) {
    if (gen !== anaGen) return;
    if (ana.reports[i]) continue;
    let r;
    try {
      r = await anaCall('analyze', {
        gameString: gameStringOf(ana.moves, i), move: ana.moves[i], timeMs: ana.timeMs, tournament: ana.tournament,
      });
    } catch (e) {
      if (gen === anaGen) $('ana-progress').textContent = `분석 오류: ${e.message}`;
      return;
    }
    if (gen !== anaGen) return;
    ana.reports[i] = r;
    renderAnalysis();
  }
}

async function goTo(k) {
  if (!ana) return;
  k = Math.max(0, Math.min(ana.moves.length, k));
  ana.ply = k;
  ana.showBest = false;
  await ensureState(k);
  render();
  keepInView();
}

function node(tag, cls, text) {
  const n = document.createElement(tag);
  if (cls) n.className = cls;
  if (text !== undefined) n.textContent = text;
  return n;
}

function renderAnalysis() {
  if (!ana) return;
  const n = ana.moves.length;
  const done = ana.reports.filter(Boolean).length;
  $('ana-progress').textContent = done < n ? `분석 중… ${done} / ${n}수` : `분석 완료 (${n}수)`;
  $('ana-pos').textContent = ana.ply === 0 ? '시작' : `${ana.ply} / ${n}수`;
  renderGraph();
  renderCard();
  renderAnaMoves();
  const r = ana.reports[ana.ply - 1];
  if (ana.showBest && r) setStatus(`${ana.ply}수째 대신 더 좋은 수: ${r.bestDesc}`);
  else if (r) setStatus(`${ana.ply}수 뒤 · ${r.standing}`);
  else if (ana.ply === 0) setStatus('분석: 시작 국면');
  else setStatus(`${ana.ply}수 뒤 · 분석 중…`);
}

function renderGraph() {
  const g = $('ana-graph');
  g.replaceChildren();
  const n = Math.max(ana.moves.length, 1);
  const x = (k) => (k / n) * 300;
  const y = (a) => 50 - a * 46;
  el('line', { x1: 0, y1: 50, x2: 300, y2: 50, class: 'axis' }, g);
  const pts = [[x(0), y(0)]];
  ana.reports.forEach((r, i) => { if (r) pts.push([x(i + 1), y(r.advantage)]); });
  if (pts.length > 1) {
    const line = pts.map((p) => p.map((v) => v.toFixed(1)).join(',')).join(' ');
    el('polygon', { points: `${x(0)},50 ${line} ${pts[pts.length - 1][0].toFixed(1)},50`, class: 'area' }, g);
    el('polyline', { points: line, class: 'curve' }, g);
  }
  ana.reports.forEach((r, i) => {
    if (r && QUALITY_MARK[r.quality]) {
      el('circle', { cx: x(i + 1), cy: y(r.advantage), r: 2.6, class: `dot q-${r.quality}` }, g);
    }
  });
  el('line', { x1: x(ana.ply), y1: 0, x2: x(ana.ply), y2: 100, class: 'cursor' }, g);
}

function renderCard() {
  const card = $('ana-card');
  card.replaceChildren();
  if (ana.ply === 0) {
    card.append(node('p', 'ana-hint', '시작 국면이다. ▶ 버튼이나 아래 기보를 눌러 수를 하나씩 본다.'));
    return;
  }
  const r = ana.reports[ana.ply - 1];
  const moveNo = `${ana.ply}수째`;
  if (!r) {
    card.append(node('p', 'ana-move', `${moveNo}: ${ana.moves[ana.ply - 1]}`));
    card.append(node('p', 'ana-hint', '이 수는 아직 분석 중이다…'));
    return;
  }
  const head = node('div', 'ana-card-head');
  head.append(node('span', `q-badge q-${r.quality}`, r.qualityKo));
  head.append(node('span', 'ana-move', `${moveNo} ${COLOR_KO[r.mover]}: ${r.playedDesc}`));
  card.append(head);
  card.append(node('p', 'ana-summary', r.summary));
  const ul = node('ul', 'ana-reasons');
  for (const t of r.reasons) ul.append(node('li', '', t));
  card.append(ul);
  if (r.showBest) {
    const box = node('div', 'ana-best');
    box.append(node('h3', '', '더 좋은 수'));
    box.append(node('p', 'ana-move', r.bestDesc));
    const why = node('ul', 'ana-reasons');
    for (const t of r.bestReasons) why.append(node('li', '', t));
    box.append(why);
    if (r.pv.length > 1) {
      box.append(node('p', 'ana-pv-title', '예상 수순'));
      const ol = node('ol', 'ana-pv');
      for (const t of r.pv.slice(0, 6)) ol.append(node('li', '', t));
      box.append(ol);
    }
    const b = node('button', 'small', ana.showBest ? '실제 둔 수로 돌아가기' : '판에서 보기');
    b.addEventListener('click', async () => {
      ana.showBest = !ana.showBest;
      await ensureState(ana.ply - 1);
      render();
      keepInView();
    });
    box.append(b);
    card.append(box);
  } else if (r.best !== r.played) {
    card.append(node('p', 'ana-alt', `엔진의 1순위는 ${r.bestDesc}였지만 차이는 작다.`));
  }
}

function renderAnaMoves() {
  const ol = $('ana-moves');
  ol.replaceChildren();
  for (let i = 0; i < ana.moves.length; i += 2) {
    const li = node('li');
    li.append(node('span', 'n', `${i / 2 + 1}.`));
    for (const j of [i, i + 1]) {
      if (j >= ana.moves.length) break;
      const r = ana.reports[j];
      const b = node('button', `mv${ana.ply === j + 1 ? ' current' : ''}${r ? ` q-${r.quality}` : ' pending'}`);
      b.textContent = ana.moves[j] + (r ? QUALITY_MARK[r.quality] : '');
      b.addEventListener('click', () => goTo(j + 1));
      li.append(b);
    }
    ol.append(li);
  }
  // Scroll only the move list (not the whole panel) to the current move.
  const cur = ol.querySelector('.current');
  if (cur) {
    const top = cur.offsetTop - ol.offsetTop;
    if (top < ol.scrollTop || top + cur.offsetHeight > ol.scrollTop + ol.clientHeight) {
      ol.scrollTop = top - ol.clientHeight / 2;
    }
  }
}

$('analyze').addEventListener('click', () => {
  if (ana) {
    closeAnalysis();
    render();
    fit();
  } else if (snap) {
    openAnalysis(snap.state.history.slice(), snap.state.tournament);
  }
});
$('ana-close').addEventListener('click', () => { closeAnalysis(); render(); fit(); });
$('ana-first').addEventListener('click', () => goTo(0));
$('ana-prev').addEventListener('click', () => goTo(ana.ply - 1));
$('ana-next').addEventListener('click', () => goTo(ana.ply + 1));
$('ana-last').addEventListener('click', () => goTo(ana.moves.length));
$('ana-speed').addEventListener('change', () => {
  if (!ana) return;
  ana.timeMs = +$('ana-speed').value;
  ana.reports = [];
  ana.gen = ++anaGen;
  render();
  runAnalysis(ana.gen);
});
$('ana-graph').addEventListener('click', (e) => {
  if (!ana) return;
  const rect = $('ana-graph').getBoundingClientRect();
  goTo(Math.round(((e.clientX - rect.left) / rect.width) * ana.moves.length));
});
window.addEventListener('keydown', (e) => {
  if (!ana || e.target.closest('input, select, textarea')) return;
  if (e.key === 'ArrowLeft') goTo(ana.ply - 1);
  else if (e.key === 'ArrowRight') goTo(ana.ply + 1);
});

// Moves of a UHP GameString ("Base;InProgress;White[3];wS1;...") or of a
// plain ';'-separated move list.
function parseGameText(text) {
  const parts = text.trim().split(';').map((x) => x.trim()).filter(Boolean);
  if (parts[0] === 'Base') parts.shift();
  if (parts.length && /^(NotStarted|InProgress|Draw|WhiteWins|BlackWins)$/.test(parts[0])) parts.shift();
  if (parts.length && /^(White|Black)\[\d+\]$/.test(parts[0])) parts.shift();
  return parts;
}

// Import: a UHP GameString ("Base;InProgress;White[3];wS1;bS1 wS1-;...")
// or just the moves separated by ';'.
$('ana-import').addEventListener('click', async () => {
  const text = prompt('기보(UHP GameString)를 붙여 넣는다.\n예: Base;InProgress;White[3];wS1;bS1 wS1-;wQ -wS1;bQ bS1-');
  if (!text) return;
  const parts = parseGameText(text);
  const tournament = ana ? ana.tournament : true;
  try {
    // Validates every move; throws with the offending move otherwise.
    await anaCall('position', { gameString: gameStringOf(parts, parts.length), tournament });
  } catch (e) {
    alert(`기보를 읽지 못했다: ${e.message}`);
    return;
  }
  await openAnalysis(parts, tournament);
});

$('ana-export').addEventListener('click', async () => {
  if (!ana) return;
  await ensureState(ana.moves.length);
  const gs = ana.states.get(ana.moves.length).gameString;
  const blob = new Blob([gs + '\n'], { type: 'text/plain' });
  const a = document.createElement('a');
  a.href = URL.createObjectURL(blob);
  a.download = `hive-${new Date().toISOString().slice(0, 10)}.txt`;
  a.click();
  setTimeout(() => URL.revokeObjectURL(a.href), 1000);
  try { await navigator.clipboard.writeText(gs); } catch { /* download is enough */ }
});

// Continue playing from the shown position, as the side to move.
$('ana-continue').addEventListener('click', async () => {
  if (!ana) return;
  const gameString = gameStringOf(ana.moves, ana.ply);
  const tournament = ana.tournament;
  closeAnalysis();
  generation++;
  busy = true;
  try {
    snap = await call('load', { gameString, tournament });
  } catch (e) {
    busy = false;
    setStatus(`불러오지 못했다: ${e.message}`);
    return;
  }
  busy = false;
  human = snap.state.toMove;
  $('color').value = human;
  lastAi = null;
  selected = null;
  render();
  fit();
});

// ------------------------------------------------------------------- study
//
// Study board: the person plays both sides from any position (an empty
// board by default) and sees the engine's best candidate moves, each with
// plain-language reasons. Candidates are cached per position (key = moves
// so far); a dedicated worker computes them and is restarted when the
// position changes mid-computation.

const STUDY_COUNT = 5;
let studyCall = null;
let studyBusyKey = null; // position the study worker is working on
let studyGen = 0;

const posKey = (moves) => moves.join(';');

function studyWorker() {
  if (!studyCall) studyCall = startWorker();
  return studyCall;
}

// Drops the computation in flight (a new position makes it useless).
function cancelStudyWork() {
  if (studyCall && studyBusyKey !== null) {
    studyCall.terminate();
    studyCall = null;
  }
  studyBusyKey = null;
}

async function openStudy() {
  if (snap && snap.state.ply > 0 && !isOver() && !ana
    && !confirm('진행 중인 대국을 접고 빈 판에서 탐구를 시작할까?')) return;
  closeAnalysis();
  generation++; // drop a pending AI move
  thinking = false;
  hint = null;
  lastAi = null;
  selected = null;
  study = {
    timeMs: +$('study-speed').value,
    cache: new Map(), // key -> candidates result
    judged: new Map(), // key of position after a move -> move report
    redo: [], // moves taken back, last = next to replay
    preview: null,
    error: null,
  };
  $('study').hidden = false;
  busy = true;
  snap = await call('new', { tournament: $('tournament').checked });
  busy = false;
  render();
  fit();
  refreshStudy();
}

function closeStudy() {
  if (!study) return;
  studyGen++;
  cancelStudyWork();
  study = null;
  $('study').hidden = true;
}

// Keeps the redo list when the move played is the one that was taken back.
function studyPlayed(uhp) {
  study.preview = null;
  const next = study.redo[study.redo.length - 1];
  if (next === uhp) study.redo.pop();
  else study.redo = [];
}

async function studyBack() {
  if (!study || busy || snap.state.ply === 0) return;
  const last = snap.state.history[snap.state.ply - 1];
  busy = true;
  selected = null;
  study.preview = null;
  snap = await call('undo', { n: 1 });
  study.redo.push(last);
  busy = false;
  render();
  refreshStudy();
}

async function studyForward() {
  if (!study || busy || study.redo.length === 0) return;
  await play(study.redo[study.redo.length - 1]);
}

async function studyReset() {
  if (!study || busy || snap.state.ply === 0) return;
  const moves = snap.state.history.slice();
  busy = true;
  selected = null;
  study.preview = null;
  snap = await call('new', { tournament: snap.state.tournament });
  study.redo = study.redo.concat(moves.reverse());
  busy = false;
  render();
  fit();
  refreshStudy();
}

// Computes candidates for the shown position, then judges the move that led
// here if it was not one of the previous position's candidates.
async function refreshStudy() {
  if (!study) return;
  const gen = studyGen;
  const moves = snap.state.history.slice();
  const key = posKey(moves);
  const tournament = snap.state.tournament;
  study.error = null;
  if (studyBusyKey !== null && studyBusyKey !== key) cancelStudyWork();
  if (studyBusyKey === key) return; // already on it
  if (!study.cache.has(key)) {
    studyBusyKey = key;
    renderStudy();
    let res;
    try {
      res = await studyWorker()('candidates', {
        gameString: gameStringOf(moves, moves.length), timeMs: study.timeMs, count: STUDY_COUNT, tournament,
      });
    } catch (e) {
      if (e.message === 'cancelled') return;
      if (studyBusyKey === key) studyBusyKey = null;
      if (study && gen === studyGen) {
        study.error = e.message;
        renderStudy();
      }
      return;
    }
    if (studyBusyKey === key) studyBusyKey = null;
    if (!study || gen !== studyGen) return;
    study.cache.set(key, res);
    if (posKey(snap.state.history) !== key) return; // moved on meanwhile
    renderStudy();
  }
  // Judge the last move when the candidates of the previous position do not
  // already cover it.
  if (moves.length === 0 || study.judged.has(key)) return;
  const last = moves[moves.length - 1];
  const prev = study.cache.get(posKey(moves.slice(0, -1)));
  if (prev && prev.list.some((c) => c.same.includes(last))) return;
  studyBusyKey = key;
  let rep;
  try {
    rep = await studyWorker()('analyze', {
      gameString: gameStringOf(moves, moves.length - 1), move: last, timeMs: study.timeMs / 2, tournament,
    });
  } catch (e) {
    if (studyBusyKey === key) studyBusyKey = null;
    return;
  }
  if (studyBusyKey === key) studyBusyKey = null;
  if (!study || gen !== studyGen) return;
  study.judged.set(key, rep);
  if (posKey(snap.state.history) === key) renderStudy();
}

function renderStudy() {
  if (!study) return;
  const s = snap.state;
  const moves = s.history;
  const key = posKey(moves);
  const res = study.cache.get(key);
  $('study-pos').textContent = s.ply === 0 ? '빈 판' : `${s.ply}수 뒤`;
  $('study-back').disabled = busy || s.ply === 0;
  $('study-reset').disabled = busy || s.ply === 0;
  $('study-fwd').disabled = busy || study.redo.length === 0;

  // Status line.
  let text;
  if (s.status === 'WhiteWins' || s.status === 'BlackWins') {
    text = `탐구 · ${COLOR_KO[s.status === 'WhiteWins' ? 'White' : 'Black']} 승리`;
  } else if (s.status === 'Draw') {
    text = '탐구 · 무승부';
  } else {
    text = `탐구 · ${COLOR_KO[s.toMove]} 차례 (${s.turn}수째) · 흑백 모두 직접 둔다`;
    if (study.preview) text += ` · 보기: ${study.preview.desc}`;
  }
  setStatus(text);

  // Standing and notes.
  const st = $('study-standing');
  st.replaceChildren();
  if (res && res.list.length) {
    st.append(node('span', 'study-label', '형세'));
    st.append(node('span', '', res.standing));
    const bar = node('div', 'study-bar');
    const fill = node('div', 'study-bar-fill');
    fill.style.width = `${((res.advantage + 1) / 2) * 100}%`;
    bar.append(fill);
    st.append(bar);
  } else if (study.error) {
    st.append(node('span', '', `계산 오류: ${study.error}`));
  } else if (!isOver()) {
    st.append(node('span', 'ana-hint', '후보 수 계산 중…'));
  }
  const notes = $('study-notes');
  notes.replaceChildren();
  for (const t of res ? res.notes : []) notes.append(node('li', '', t));

  renderStudyJudge(moves);

  // Candidates.
  const ol = $('study-cands');
  ol.replaceChildren();
  $('study-count').textContent = res && res.legal
    ? `(둘 수 있는 수 ${res.legal}개${res.distinct < res.legal ? `, 모양이 같은 수를 묶으면 ${res.distinct}가지` : ''})`
    : '';
  if (isOver()) {
    ol.append(node('li', 'ana-hint', '게임이 끝났다. ◀로 되돌리거나 ⏮로 처음부터 다시 둔다.'));
    return;
  }
  if (!res) return;
  res.list.forEach((c, i) => ol.append(candidateItem(c, i)));
}

function candidateItem(c, i) {
  const li = node('li', `study-cand${study.preview && study.preview.uhp === c.uhp ? ' previewing' : ''}`);
  const head = node('div', 'ana-card-head');
  head.append(node('span', 'study-rank', `${i + 1}`));
  head.append(node('span', 'ana-move', c.desc));
  li.append(head);
  const tags = node('div', 'study-tags');
  tags.append(node('span', `q-badge ${i === 0 ? 'q-best' : gapClass(c)}`, c.gap));
  tags.append(node('span', 'study-after', `둔 뒤: ${c.standing}`));
  li.append(tags);
  if (c.same.length > 1) li.append(node('p', 'ana-hint', `돌리거나 뒤집으면 같은 모양이 되는 수 ${c.same.length}개를 하나로 묶었다.`));

  const ul = node('ul', 'ana-reasons');
  for (const t of c.reasons) ul.append(node('li', '', t));
  li.append(ul);
  if (c.plan) {
    const box = node('div', 'study-plan');
    box.append(node('p', 'study-plan-title', `상대가 응수한 뒤 다음 자기 수 (예상): ${c.plan.desc}`));
    const pl = node('ul', 'ana-reasons');
    for (const t of c.plan.reasons) pl.append(node('li', '', t));
    box.append(pl);
    li.append(box);
  }
  if (c.vsBest.length) {
    const box = node('div', 'ana-best');
    box.append(node('h3', '', '1순위 수가 더 나은 점'));
    const vl = node('ul', 'ana-reasons');
    for (const t of c.vsBest) vl.append(node('li', '', t));
    box.append(vl);
    li.append(box);
  }
  if (c.pv.length > 1 || c.note) {
    const d = document.createElement('details');
    d.append(node('summary', 'ana-pv-title', c.pv.length > 1 ? '예상 수순' : '이 말은'));
    if (c.note) d.append(node('p', 'ana-hint', c.note));
    if (c.pv.length > 1) {
      const pv = node('ol', 'ana-pv');
      for (const t of c.pv.slice(0, 8)) pv.append(node('li', '', t));
      d.append(pv);
    }
    li.append(d);
  }
  const actions = node('div', 'study-cand-actions');
  const show = node('button', 'small', study.preview && study.preview.uhp === c.uhp ? '보기 끄기' : '판에서 보기');
  show.addEventListener('click', () => {
    study.preview = study.preview && study.preview.uhp === c.uhp ? null : c;
    selected = null;
    render();
    keepInView();
  });
  const playBtn = node('button', 'small primary', '이 수 두기');
  playBtn.disabled = busy;
  playBtn.addEventListener('click', () => { if (!busy) play(c.uhp); });
  actions.append(show, playBtn);
  li.append(actions);
  return li;
}

function gapClass(c) {
  if (/진다/.test(c.gap)) return 'q-blunder';
  if (/크게/.test(c.gap)) return 'q-mistake';
  if (/분명히/.test(c.gap)) return 'q-inaccuracy';
  if (/이길|같다|아주 조금/.test(c.gap)) return 'q-good';
  return 'q-fine';
}

// "Last move" card: its rank among the previous candidates, or a full
// judgment when it was not one of them.
function renderStudyJudge(moves) {
  const box = $('study-judge');
  box.replaceChildren();
  if (moves.length === 0) {
    box.hidden = true;
    return;
  }
  box.hidden = false;
  const last = moves[moves.length - 1];
  const mover = moves.length % 2 === 1 ? 'White' : 'Black';
  const prev = study.cache.get(posKey(moves.slice(0, -1)));
  const idx = prev ? prev.list.findIndex((c) => c.same.includes(last)) : -1;
  const head = node('div', 'ana-card-head');
  box.append(head);
  if (idx >= 0) {
    const c = prev.list[idx];
    head.append(node('span', `q-badge ${idx === 0 ? 'q-best' : gapClass(c)}`, idx === 0 ? '엔진 1순위' : `후보 ${idx + 1}순위`));
    head.append(node('span', 'ana-move', `방금 둔 수 (${moves.length}수째 ${COLOR_KO[mover]}): ${c.desc}`));
    if (idx > 0) box.append(node('p', 'ana-summary', `${c.gap}.`));
    return;
  }
  const r = study.judged.get(posKey(moves));
  if (!r) {
    head.append(node('span', 'ana-move', `방금 둔 수 (${moves.length}수째 ${COLOR_KO[mover]}): ${last}`));
    box.append(node('p', 'ana-hint', '이 수를 판정하는 중…'));
    return;
  }
  head.append(node('span', `q-badge q-${r.quality}`, r.qualityKo));
  head.append(node('span', 'ana-move', `방금 둔 수 (${moves.length}수째 ${COLOR_KO[mover]}): ${r.playedDesc}`));
  box.append(node('p', 'ana-summary', r.summary));
  const ul = node('ul', 'ana-reasons');
  for (const t of r.reasons) ul.append(node('li', '', t));
  box.append(ul);
  if (r.showBest) {
    const b = node('div', 'ana-best');
    b.append(node('h3', '', '더 좋은 수'));
    b.append(node('p', 'ana-move', r.bestDesc));
    const why = node('ul', 'ana-reasons');
    for (const t of r.bestReasons) why.append(node('li', '', t));
    b.append(why);
    box.append(b);
  }
}

$('study-btn').addEventListener('click', () => {
  if (study) {
    closeStudy();
    render();
    fit();
  } else {
    openStudy().catch((e) => setStatus(`탐구를 시작하지 못했다: ${e.message}`));
  }
});
$('study-close').addEventListener('click', () => { closeStudy(); render(); fit(); });
$('study-back').addEventListener('click', studyBack);
$('study-fwd').addEventListener('click', studyForward);
$('study-reset').addEventListener('click', studyReset);
$('study-speed').addEventListener('change', () => {
  if (!study) return;
  study.timeMs = +$('study-speed').value;
  cancelStudyWork();
  study.cache.clear();
  study.judged.clear();
  render();
  refreshStudy();
});
window.addEventListener('keydown', (e) => {
  if (!study || e.target.closest('input, select, textarea')) return;
  if (e.key === 'ArrowLeft') studyBack();
  else if (e.key === 'ArrowRight') studyForward();
});

// Leave the study board and play the AI from here, as the side to move.
$('study-ai').addEventListener('click', () => {
  if (!study) return;
  human = snap.state.toMove;
  $('color').value = human;
  closeStudy();
  generation++;
  render();
  fit();
});

$('study-import').addEventListener('click', async () => {
  if (!study) return;
  const text = prompt('기보(UHP GameString)를 붙여 넣는다. 그 국면부터 탐구한다.');
  if (!text) return;
  const parts = parseGameText(text);
  busy = true;
  try {
    snap = await call('load', { gameString: gameStringOf(parts, parts.length), tournament: snap.state.tournament });
  } catch (e) {
    busy = false;
    alert(`기보를 읽지 못했다: ${e.message}`);
    return;
  }
  busy = false;
  study.redo = [];
  study.preview = null;
  selected = null;
  render();
  fit();
  refreshStudy();
});

$('study-copy').addEventListener('click', async () => {
  try {
    await navigator.clipboard.writeText(snap.state.gameString);
    setStatus('기보(UHP GameString)를 복사했다');
  } catch {
    prompt('기보', snap.state.gameString);
  }
});

newGame().catch((e) => setStatus(`엔진을 불러오지 못했다: ${e.message}`));
