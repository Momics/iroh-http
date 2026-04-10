# Contributing to iroh-http

Thank you for your interest in contributing!

## Development setup

### Prerequisites

- Rust 1.77+ (`rustup update stable`)
- Node.js 18+ (for Node.js adapter)
- Deno 2+ (for Deno adapter)
- Tauri CLI v2 (for Tauri plugin)

### Build

```sh
# Check all Rust crates
cargo check --workspace

# Check Tauri plugin (separate workspace)
cd packages/iroh-http-tauri && cargo check

# TypeScript
npm install
npm run typecheck
```

## Code style

- Rust: `cargo fmt` + `cargo clippy`
- TypeScript: standard formatting

## Submitting changes

1. Fork the repository
2. Create a feature branch: `git checkout -b feature/my-change`
3. Make your changes with tests
4. Run `cargo check --workspace` to verify Rust compiles
5. Submit a pull request

## License

By contributing, you agree that your contributions will be licensed under
MIT OR Apache-2.0.
