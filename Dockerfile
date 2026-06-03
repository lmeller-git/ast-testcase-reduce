FROM theosotr/sqlite3-reducer:latest

USER root

RUN apt-get update && apt-get install -y \
    build-essential \
    curl \
    && rm -rf /var/lib/apt/lists/*

RUN curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y && \
    curl -LsSf https://astral.sh/uv/install.sh | sh

ENV PATH="/root/.cargo/bin:/root/.local/bin:${PATH}" \
    UV_LINK_MODE=copy \
    MATURIN_RELEASE=true \
    PYTHONPATH=/app/python

WORKDIR /app

COPY reducer.sh /usr/bin/reducer
RUN chmod +x /usr/bin/reducer

COPY pyproject.toml uv.lock README.md ./
RUN --mount=type=cache,target=/root/.cache/uv \
    uv sync --frozen --no-install-project --no-dev

COPY ./extern ./extern
RUN --mount=type=cache,target=/root/.cache/uv \
    --mount=type=cache,target=/root/.cargo/registry \
    uv sync --frozen

COPY . .

ENV UV_NO_SYNC=1

VOLUME ["/app/docker_out"]

ENTRYPOINT ["/usr/bin/reducer"]
