build:
    uv sync

test-rust: build
    cargo test -p lib-tr --no-default-features --locked --all-targets
    cargo test --exclude lib-tr --workspace --locked --all-features --all-targets

test-py: build
    uv run pytest

test: test-rust test-py

run: build
    uv run python/reduce/main.py
