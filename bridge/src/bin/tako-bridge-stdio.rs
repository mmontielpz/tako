//! Development and test harness: serves the bridge over stdin/stdout, one JSON object per line.
//! Used by the UI test-suite to drive the REAL engine. It is not part of the shipped app.
//!
//! Usage: tako-bridge-stdio <data-dir> [test-gateway-port|t]
//! Request:  {"id": 1, "cmd": "open_project", "args": {...}}
//! Response: {"id": 1, "ok": ...} or {"id": 1, "err": {"code","message","details"}}

use serde_json::Value;
use std::io::{BufRead, Write};
use tako_bridge::{json_err, json_ok, Bridge, BridgeError};

fn main() {
    let Some(dir) = std::env::args().nth(1) else {
        eprintln!("usage: tako-bridge-stdio <data-dir>");
        std::process::exit(2);
    };
    // An explicit port isolates the development harness from any real gateway.
    let opened = match std::env::args().nth(2) {
        Some(mode) if mode == "t" => {
            Bridge::open_with_client(&dir, tako_bridge::client::TakoBackendClient::default())
        }
        Some(port) => Bridge::open_with_client(
            &dir,
            tako_bridge::client::TakoBackendClient::unauthenticated(
                "127.0.0.1",
                port.parse().expect("test gateway port must be a u16"),
            ),
        ),
        None => Bridge::open(&dir),
    };
    let bridge = match opened {
        Ok(b) => b,
        Err(e) => {
            eprintln!("cannot open bridge: {}", e.message);
            std::process::exit(1);
        }
    };
    let stdout = std::io::stdout();
    for line in std::io::stdin().lock().lines() {
        let Ok(line) = line else { break };
        if line.trim().is_empty() {
            continue;
        }
        let reply = match serde_json::from_str::<Value>(&line) {
            Ok(req) => {
                let id = req.get("id").cloned().unwrap_or(Value::Null);
                let cmd = req
                    .get("cmd")
                    .and_then(|c| c.as_str())
                    .unwrap_or("")
                    .to_string();
                let args = req.get("args").cloned().unwrap_or(Value::Null);
                let result = if cmd.starts_with("research_")
                    || tako_bridge::research::planning::handles(&cmd)
                {
                    tako_bridge::research::validate_json(line.as_bytes())
                        .and_then(|_| bridge.dispatch(&cmd, args))
                } else {
                    bridge.dispatch(&cmd, args)
                };
                match result {
                    Ok(v) => json_ok(&id, v),
                    Err(e) => json_err(&id, &e),
                }
            }
            Err(e) => json_err(
                &Value::Null,
                &BridgeError {
                    code: "invalid_input".into(),
                    message: e.to_string(),
                    details: vec![],
                },
            ),
        };
        let mut out = stdout.lock();
        let _ = writeln!(out, "{reply}");
        let _ = out.flush();
    }
}
