// Engine worker: owns the WASM game and answers {id, cmd, args} requests.
import init, { HiveGame } from './pkg/hive_engine.js';

const ready = init();
let game = null;

function snapshot(extra = {}) {
  return {
    state: JSON.parse(game.state()),
    moves: JSON.parse(game.validMoves()),
    ...extra,
  };
}

function replace(g) {
  if (game) game.free();
  game = g;
}

const handlers = {
  new({ tournament }) {
    replace(new HiveGame(tournament));
    return snapshot();
  },
  load({ gameString, tournament }) {
    replace(HiveGame.load(gameString, tournament));
    return snapshot();
  },
  play({ move }) {
    game.play(move);
    return snapshot({ played: move });
  },
  undo({ n }) {
    const undone = game.undo(n);
    return snapshot({ undone });
  },
  // Random AI: uniform choice among legal moves.
  random() {
    const moves = JSON.parse(game.validMoves());
    if (moves.length === 0) return snapshot();
    const m = moves[Math.floor(Math.random() * moves.length)];
    game.play(m.uhp);
    return snapshot({ played: m.uhp });
  },
  state() {
    return snapshot();
  },
};

self.onmessage = async (e) => {
  const { id, cmd, args } = e.data;
  try {
    await ready;
    const result = handlers[cmd](args || {});
    self.postMessage({ id, ok: true, result });
  } catch (err) {
    self.postMessage({ id, ok: false, error: String((err && err.message) || err) });
  }
};
