default:
    @just --list

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

run-1 n:
    uv run python/reduce/main.py --query queries/query{{n}}/original_test.sql --test queries/query{{n}}/test.sh

build-docker:
    docker build -t testcase-reduce .

docker-it: build-docker
    docker run -it --init --rm --entrypoint bash -v docker_out testcase-reduce

docker n : build-docker
    docker run --init --rm -v docker_out testcase-reduce --query queries/query{{n}}/original_test.sql --test queries/query{{n}}/test.sh

lint:
    uv run ruff check python
    uv run ruff format
    uv run ruff format --check python
    uv run pyright
