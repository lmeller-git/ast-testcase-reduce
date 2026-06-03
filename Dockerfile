FROM theosotr/sqlite3-reducer:latest AS chef

USER root

RUN apt-get update && apt-get install -y \
    build-essential \
    clang \
    curl \
    python3 \
    && rm -rf /var/lib/apt/lists/*

RUN curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y && \
    curl -LsSf https://astral.sh/uv/install.sh | sh && \
    curl -LsSf https://just.systems/install.sh | bash -s -- --to /usr/local/bin

ENV PATH="/root/.cargo/bin:${PATH}"
ENV PATH="/root/.local/bin:${PATH}"
ENV UV_LINK_MODE=copy
ENV MATURIN_RELEASE=true

FROM chef AS planner
WORKDIR /app

COPY reducer.sh /usr/bin/reducer
RUN chmod +x /usr/bin/reducer

COPY pyproject.toml uv.lock README.md ./
RUN --mount=type=cache,target=/root/.cache/uv \
    uv sync --frozen --no-install-project --no-dev

COPY ./extern ./extern
RUN --mount=type=cache,target=/root/.cargo/registry \
    --mount=type=cache,target=/root/.cargo/git \
    --mount=type=cache,target=/app/target \
    uv sync --frozen


COPY . .

ENV UV_NO_SYNC=1
ENV PYTHONPATH=/app/python

VOLUME ["/app/docker_out"]
ENTRYPOINT []
