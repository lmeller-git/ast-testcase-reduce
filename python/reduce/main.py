import asyncio
import uvloop
import argparse
import os
import tempfile
import sqlglot

from lib_ramis import BFScheduler, ReductionStepResult


def add(n1: int, n2: int) -> int:
    return n1 + n2


shared_context = {"initial_query": "", "best_query": ""}
cached = {}


def algorithm(trace: list[bool]) -> str:
    initial_sql = shared_context.get("initial_query", "")
    if not initial_sql:
        return ""

    n_chars = len(initial_sql)

    # bitmask removal

    if len(trace) > n_chars:
        return ""

    mutated_chars: list[str] = []

    for i, char in enumerate(initial_sql):
        if i < len(trace):
            if bool(trace[i]):
                continue

        mutated_chars.append(char)

    candidate_query = "".join(mutated_chars)

    return candidate_query


def split_n(s: str, n: int) -> list[str]:
    size, remainder = divmod(len(s), n)
    chunks: list[str] = []
    start = 0

    for i in range(n):
        end = start + size + (1 if i < remainder else 0)
        chunks.append(s[start:end])
        start = end

    return chunks


def yield_complements(seq: str, n: int):
    size, remainder = divmod(len(seq), n)
    start = 0

    ranges: list[tuple[int, int]] = []
    for i in range(n):
        end = start + size + (1 if i < remainder else 0)
        ranges.append((start, end))
        start = end

    for start, end in ranges:
        yield seq[:start] + seq[end:]


def algorithm2(trace: list[bool]) -> str:
    initial_sql = shared_context.get("initial_query", "")
    if not initial_sql:
        return ""

    # ddmin

    granularity = 2
    trace_idx = 0

    while True:
        current_len = len(initial_sql)

        if granularity > current_len:
            return ""

        splits = split_n(initial_sql, granularity)
        good_splt = None

        # Eval all splits
        for split in splits:
            trace_idx += 1
            if trace_idx > len(trace):
                return split

            if trace[trace_idx - 1]:
                good_splt = split
                break

        # Eval all complements
        if good_splt is None and granularity > 2:
            for complement in yield_complements(initial_sql, granularity):
                trace_idx += 1
                if trace_idx > len(trace):
                    return complement

                if trace[trace_idx - 1]:
                    good_splt = complement
                    break

        if good_splt is not None:
            initial_sql = good_splt
            granularity = 2
        else:
            granularity *= 2


async def oracle(query: str, test_script: str, reduction: int) -> ReductionStepResult:
    cached_value = cached.get(query)
    if cached_value is not None:
        return cached_value

    try:
        _ = sqlglot.parse_one(query, dialect="sqlite")
    except Exception as _e:
        # print(f"could not parse sql due to: {e}\n")
        return ReductionStepResult.new(0)

    with tempfile.NamedTemporaryFile(
        mode="w", encoding="utf-8", suffix=".sql", delete=False
    ) as tmp:
        _ = tmp.write(query)
        tmp_path = tmp.name

    process = None
    try:
        test_script_abs = os.path.abspath(test_script)

        env = os.environ.copy()
        env["TEST_CASE_LOCATION"] = tmp_path

        process = await asyncio.create_subprocess_exec(
            test_script_abs,
            env=env,
            stdout=asyncio.subprocess.DEVNULL,
            stderr=asyncio.subprocess.DEVNULL,
        )

        _ = await process.wait()

        if process.returncode != 0:
            r = ReductionStepResult.new(0)
            cached[query] = r
            return r
        else:
            r = ReductionStepResult.new(1 + reduction)
            cached[query] = r
            return r

    except asyncio.CancelledError:
        if process and process.returncode is None:
            try:
                process.terminate()
                _ = await process.wait()
            except ProcessLookupError:
                pass
        raise
    finally:
        try:
            os.remove(tmp_path)
        except OSError:
            print(f"could not remove file {tmp_path}")


async def worker(scheduler: BFScheduler, test_script: str):
    while True:
        cancel_event = asyncio.Event()
        path = scheduler.next(cancel_event)

        if path is None:
            break

        path_ = path.path()
        trace = path_.to_list()
        query = algorithm2(trace)

        if cancel_event.is_set():
            continue

        cancel_task = asyncio.create_task(cancel_event.wait())

        actual_reduction = len(shared_context["best_query"]) - len(query) if query else 0

        oracle_task = asyncio.create_task(oracle(query, test_script, actual_reduction))

        done, pending = await asyncio.wait(
            [cancel_task, oracle_task], return_when=asyncio.FIRST_COMPLETED
        )

        for task in pending:
            _ = task.cancel()

        if cancel_task in done:
            continue

        result = oracle_task.result()
        scheduler.put_result(path, result)

        if not result.is_dead() and query:
            shared_context["best_query"] = query


async def main(query_path: str, test_script: str):
    with open(query_path, "r", encoding="utf-8") as f:
        initial_query = f.read()

    shared_context["initial_query"] = initial_query
    shared_context["best_query"] = initial_query
    shared_context["test_script"] = test_script

    scheduler = BFScheduler()

    workers = [asyncio.create_task(worker(scheduler, test_script)) for _ in range(5)]

    _ = await asyncio.gather(*workers)

    print(f"Reduced query: {shared_context['best_query']}")


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description="SQL Test-Case Reducer")
    _ = parser.add_argument(
        "--query", required=True, help="Path to the target SQL query to minimize.", type=str
    )
    _ = parser.add_argument(
        "--test", required=True, help="Path to the external verification oracle script.", type=str
    )
    args = parser.parse_args()

    uvloop.run(main(args.query, args.test))
