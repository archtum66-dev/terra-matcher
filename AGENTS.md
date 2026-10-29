# Rules for coding agents in this repository

- Work only inside this folder. Do not read, search or list anything outside it.
- No network access in tests. Tests must pass offline.
- No keys, seeds, tokens or API keys. No `.env` files. No Mainnet transactions.
- Quote requests to NEAR Intents (1Click) are sent with `"dry": true` only.
- Rust 2021, stable toolchain. Dependencies only if needed; pin them (`Cargo.lock` is committed).
- Run `cargo fmt`, `cargo clippy --all-targets -- -D warnings` and `cargo test` when `cargo` is available. Report the real result. If `cargo` is not available, say so and do not claim tests passed.
- Do not run `git commit`, `git push` or change branches. The wrapper script does that.
- Interface and rules: `docs/SPEC.md`. Do not change its meaning without saying so in your report.
- Names in code, comments, docs and tests are English.
