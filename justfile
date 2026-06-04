clean:
    uv clean
    rm -f extern/ramis/python/lib_ramis/*.so
    rm -rf extern/ramis/target
    rm -rf target/maturin

build:
    uv sync
    CARGO_TARGET_DIR=target/maturin uv run maturin develop

test-rust: build
    cargo test -p lib-tr --no-default-features --locked --all-targets
    cargo test --exclude lib-tr --workspace --locked --all-features --all-targets

test-py: build
    uv run pytest

test: test-rust test-py

run: build
    uv run python/reduce/main.py

run-1: build
    uv run python/reduce/main.py --query queries/query1/original_test.sql --test queries/query1/test.sh
