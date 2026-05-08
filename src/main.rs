//! Headless server entry point. The full app build lives in `lib.rs` so it
//! can also be embedded by `cindertide-play` (the TUI binary).

fn main() {
    cindertide::run_server();
}
