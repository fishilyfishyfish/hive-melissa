//! UHP engine command handling.

use hive_engine::uhp_engine::UhpEngine;

fn run(e: &mut UhpEngine, cmd: &str) -> Vec<String> {
    let out = e.handle(cmd);
    let lines: Vec<String> = out.lines().map(String::from).collect();
    assert_eq!(lines.last().map(String::as_str), Some("ok"), "{cmd}: {out}");
    lines[..lines.len() - 1].to_vec()
}

#[test]
fn info_and_new_game() {
    let mut e = UhpEngine::new();
    let info = run(&mut e, "info");
    assert!(info[0].starts_with("id HiveMelissa v"));
    assert_eq!(run(&mut e, "newgame"), ["Base;NotStarted;White[1]"]);
    assert_eq!(run(&mut e, "newgame Base"), ["Base;NotStarted;White[1]"]);
    assert_eq!(run(&mut e, "validmoves"), ["wS1;wB1;wG1;wA1"]);
}

#[test]
fn play_undo_and_game_strings() {
    let mut e = UhpEngine::new();
    run(&mut e, "newgame");
    assert_eq!(run(&mut e, "play wS1"), ["Base;InProgress;Black[1];wS1"]);
    assert_eq!(
        run(&mut e, "play bS1 wS1-"),
        ["Base;InProgress;White[2];wS1;bS1 wS1-"]
    );
    assert_eq!(run(&mut e, "undo"), ["Base;InProgress;Black[1];wS1"]);
    assert_eq!(run(&mut e, "undo 1"), ["Base;NotStarted;White[1]"]);
    assert!(run(&mut e, "undo")[0].starts_with("err"));
    let gs = "Base;InProgress;White[3];wS1;bS1 wS1-;wQ -wS1;bQ bS1-";
    assert_eq!(run(&mut e, &format!("newgame {gs}")), [gs]);
}

#[test]
fn errors_and_invalid_moves() {
    let mut e = UhpEngine::new();
    assert!(run(&mut e, "play wS1")[0].starts_with("err"), "no game yet");
    run(&mut e, "newgame");
    assert!(
        run(&mut e, "play wQ")[0].starts_with("invalidmove"),
        "tournament rule"
    );
    assert!(run(&mut e, "pass")[0].starts_with("invalidmove"));
    assert!(run(&mut e, "frobnicate")[0].starts_with("err"));
    assert!(run(&mut e, "newgame Base+MLP")[0].starts_with("err"));
    assert!(run(&mut e, "bestmove sideways")[0].starts_with("err"));
}

#[test]
fn bestmove_returns_a_legal_move() {
    let mut e = UhpEngine::new();
    run(
        &mut e,
        "newgame Base;InProgress;White[3];wS1;bS1 wS1-;wQ -wS1;bQ bS1-",
    );
    let valid = run(&mut e, "validmoves")[0].clone();
    let valid: Vec<&str> = valid.split(';').collect();
    for cmd in ["bestmove depth 2", "bestmove time 00:00:00.2", "bestmove"] {
        let m = run(&mut e, cmd)[0].clone();
        assert!(valid.contains(&m.as_str()), "{cmd} -> {m}");
    }
}

#[test]
fn options() {
    let mut e = UhpEngine::new();
    assert_eq!(run(&mut e, "options").len(), 2);
    assert_eq!(
        run(&mut e, "options set TimeMs 250"),
        ["TimeMs;int;250;1000;1;3600000"]
    );
    assert_eq!(
        run(&mut e, "options get TimeMs"),
        ["TimeMs;int;250;1000;1;3600000"]
    );
    assert!(run(&mut e, "options set TimeMs 0")[0].starts_with("err"));
    assert!(run(&mut e, "options get Nope")[0].starts_with("err"));
}
