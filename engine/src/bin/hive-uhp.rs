//! UHP engine over stdin/stdout, for hosts such as MzingaViewer or match
//! runners. Usage: `hive-uhp`, then type UHP commands.

use std::io::{self, BufRead, Write};

use hive_engine::uhp_engine::UhpEngine;

fn main() {
    let mut engine = UhpEngine::new();
    let stdout = io::stdout();
    let mut out = stdout.lock();
    writeln!(out, "{}", engine.greeting()).ok();
    out.flush().ok();
    for line in io::stdin().lock().lines() {
        let Ok(line) = line else { break };
        if line.trim() == "exit" {
            break;
        }
        let reply = engine.handle(&line);
        if !reply.is_empty() {
            writeln!(out, "{reply}").ok();
            out.flush().ok();
        }
    }
}
