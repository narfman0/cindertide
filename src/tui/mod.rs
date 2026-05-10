//! TUI client — runs on the main thread of the `cindertide-play` binary
//! and talks to the embedded headless server via BRP on localhost:15703.

pub mod state;
pub mod render;

use std::time::{Duration, Instant};

use crossterm::event::{self, Event, KeyEventKind};
use ratatui::backend::CrosstermBackend;
use ratatui::Terminal;
use std::io;

use state::TuiApp;

pub const POLL_INTERVAL: Duration = Duration::from_millis(250);
pub const TICK_INTERVAL: Duration = Duration::from_millis(50);

/// Main TUI loop. Returns Ok when the user quits cleanly.
pub fn run() -> io::Result<()> {
    crossterm::terminal::enable_raw_mode()?;
    let mut stdout = io::stdout();
    crossterm::execute!(
        stdout,
        crossterm::terminal::EnterAlternateScreen,
        crossterm::event::EnableMouseCapture
    )?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let result = run_inner(&mut terminal);

    crossterm::terminal::disable_raw_mode()?;
    crossterm::execute!(
        terminal.backend_mut(),
        crossterm::terminal::LeaveAlternateScreen,
        crossterm::event::DisableMouseCapture
    )?;
    terminal.show_cursor()?;
    result
}

fn run_inner<B: ratatui::backend::Backend>(
    terminal: &mut Terminal<B>,
) -> io::Result<()> {
    let mut app = TuiApp::new();
    let mut last_poll = Instant::now() - POLL_INTERVAL;

    loop {
        if app.should_quit {
            return Ok(());
        }

        if last_poll.elapsed() >= POLL_INTERVAL {
            app.poll();
            last_poll = Instant::now();
        }

        terminal.draw(|f| render::draw(f, &app))?;

        if event::poll(TICK_INTERVAL)? {
            if let Event::Key(key) = event::read()? {
                if key.kind == KeyEventKind::Press {
                    app.handle_key(key.code);
                }
            }
        }
    }
}

/// Quick helper used by callers that want to know whether the BRP server
/// is reachable yet. Used at startup to wait for the server thread.
pub fn server_ready() -> bool {
    let client = reqwest::blocking::Client::builder()
        .timeout(Duration::from_millis(200))
        .build()
        .ok();
    let Some(c) = client else { return false; };
    c.post("http://127.0.0.1:15703")
        .json(&serde_json::json!({
            "jsonrpc": "2.0",
            "method": "bevy/list",
            "id": 1
        }))
        .send()
        .ok()
        .map(|r| r.status().is_success())
        .unwrap_or(false)
}

/// Block (with a deadline) until the BRP server is reachable.
pub fn wait_for_server(max_wait: Duration) -> bool {
    let start = Instant::now();
    while start.elapsed() < max_wait {
        if server_ready() {
            return true;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    false
}

/// Send a one-shot BRP method call. Returns the parsed JSON response.
pub fn call(method: &str, params: serde_json::Value) -> Option<serde_json::Value> {
    let client = reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(2))
        .build()
        .ok()?;
    let body = serde_json::json!({
        "jsonrpc": "2.0",
        "method": method,
        "id": 1,
        "params": params,
    });
    let resp = client
        .post("http://127.0.0.1:15703")
        .json(&body)
        .send()
        .ok()?;
    resp.json().ok()
}
