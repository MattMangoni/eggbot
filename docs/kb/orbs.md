# Amp orbs

Amp orbs are Linux (Debian 12) sandboxes. `.agents/setup` prepares one for this repo.

- eggbot cannot build natively on Linux: `objc2` refuses non-Apple targets ("only works on Apple platforms").
- It can be checked for macOS from Linux: `cargo check`/`cargo clippy --target aarch64-apple-darwin` work with only `rustup target add aarch64-apple-darwin`, no apt packages and no Apple SDK.
- Trap: `gpui-kit` turns on `runtime_shaders`, so `gpui-pre-apple` does `include_str!("$OUT_DIR/stitched_shaders.metal")`, but its `build.rs` only writes that file on a macOS host. Setup finds the out dir with `--message-format=json` and writes an empty stub; enough for check, never linked.
- Setup sets `[build] target = "aarch64-apple-darwin"` in the orb's `~/.cargo/config.toml`, so the AGENTS.md `cargo fmt` and `cargo clippy` commands work as written.
- `cargo test`, `cargo build`, `cargo run` and `scripts/bundle.sh` cannot run in an orb (no macOS linker or Metal); CI on macOS runs them.
- Cold setup is about 90 s (most of it the first clippy); a warm rerun is about 3 s.
