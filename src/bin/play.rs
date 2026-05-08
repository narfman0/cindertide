//! `cindertide-play` — single-binary launcher: spawns the headless server
//! on a background thread and runs the ratatui TUI on the main thread.

use std::time::Duration;

fn main() -> std::io::Result<()> {
    // Server runs in a background thread; it owns the Bevy App and blocks
    // on its event loop until AppExit fires.
    let server_handle = std::thread::spawn(|| {
        cindertide::run_server();
    });

    // Wait for BRP to come up before letting the TUI start polling.
    if !cindertide::tui::wait_for_server(Duration::from_secs(10)) {
        eprintln!("server failed to start within 10s");
        return Ok(());
    }

    let result = cindertide::tui::run();

    // Tell the server to exit.
    let _ = cindertide::tui::call("game/exit", serde_json::json!({}));
    let _ = server_handle.join();
    result
}
