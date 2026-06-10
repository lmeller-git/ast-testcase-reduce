import json
import shutil
import subprocess
import time
from pathlib import Path

import sqlglot


QUERY_COUNT = 20
OUTPUT_PATH = Path("python/benchmarks/benchmark_data.json")
WORK_DIR = Path("python/benchmarks/tmp")


def count_sql_tokens(path: Path) -> int:
    with open(path) as f:
        query = f.read()
    tokenizer = sqlglot.Tokenizer()
    tokens = tokenizer.tokenize(query)
    return len(tokens)


def reducer_command(query_id: int, query_path: Path) -> list[str]:
    query_dir = Path("queries") / f"query{query_id}"
    return [
        "uv",
        "run",
        "python/reduce/main.py",
        "--query",
        str(query_path),
        "--test",
        str(query_dir / "test.sh"),
    ]


def collect_one(query_id: int) -> tuple[int, int, float]:
    query_dir = Path("queries") / f"query{query_id}"
    original_query_path = query_dir / "original_test.sql"
    benchmark_query_path = WORK_DIR / f"query{query_id}.sql"

    original_tokens = count_sql_tokens(original_query_path)

    WORK_DIR.mkdir(parents=True, exist_ok=True)
    shutil.copyfile(original_query_path, benchmark_query_path)

    print(f"Reducing query{query_id}...")
    start = time.perf_counter()
    result = subprocess.run(reducer_command(query_id, benchmark_query_path), check=False)
    wall_clock_seconds = time.perf_counter() - start

    if result.returncode != 0:
        print(f"Warning: reducer exited with code {result.returncode} for query{query_id}.")

    final_tokens = count_sql_tokens(benchmark_query_path)

    print(
        f"query{query_id}: original_tokens={original_tokens}, "
        f"final_tokens={final_tokens}, wall_clock_seconds={wall_clock_seconds:.3f}"
    )

    return original_tokens, final_tokens, wall_clock_seconds


def main() -> None:
    original_tokens: list[int] = []
    final_tokens: list[int] = []
    wall_clock_seconds: list[float] = []

    for query_id in range(1, QUERY_COUNT + 1):
        original, final, seconds = collect_one(query_id)
        original_tokens.append(original)
        final_tokens.append(final)
        wall_clock_seconds.append(seconds)

    data = {
        "original_tokens": original_tokens,
        "final_tokens": final_tokens,
        "wall_clock_seconds": wall_clock_seconds,
    }

    OUTPUT_PATH.parent.mkdir(parents=True, exist_ok=True)
    OUTPUT_PATH.write_text(json.dumps(data, indent=2), encoding="utf-8")
    print(f"Wrote {OUTPUT_PATH}")


if __name__ == "__main__":
    main()
