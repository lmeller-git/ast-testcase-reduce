import asyncio
from typing import Any, override
from lib_ramis import CancelToken, GenericResult, PyState
from lib_ramis.binary import BinaryEvent
import argparse
import os
import sys
import tempfile
import sqlglot
import math

from reduce.ast_reducer_idea import reduce_sql_text


def add(n1: int, n2: int) -> int:
    return n1 + n2


class AsyncCancel(CancelToken):
    def __init__(self) -> None:
        super().__init__()
        self.event: asyncio.Event = asyncio.Event()

    @override
    def cancel(self) -> None:
        self.event.set()

    @override
    def is_cancelled(self) -> bool:
        return self.event.is_set()


class DDMinState(PyState):
    def __init__(self, sql: list[Any], n: int = 2, phase: str = "splits", idx: int = 0):
        super().__init__()
        self.sql: list[Any] = sql
        self.n: int = n
        self.phase: str = phase
        self.idx: int = idx

    @property
    def is_terminal(self) -> bool:
        if self.n > len(self.sql):
            return True

        chunk_size = len(self.sql) // self.n

        min_chunk_size = max(1, int(math.ceil(math.sqrt(len(self.sql)) / 5)))

        return chunk_size < min_chunk_size

    def get_candidate(self) -> list[Any]:
        if self.is_terminal:
            return []

        size, remainder = divmod(len(self.sql), self.n)
        start = self.idx * size + min(self.idx, remainder)
        end = start + size + (1 if self.idx < remainder else 0)

        if self.phase == "splits":
            return self.sql[start:end]
        else:
            return self.sql[:start] + self.sql[end:]

    @override
    def step(self, event: BinaryEvent) -> DDMinState:
        if self.is_terminal:
            return DDMinState(sql=[], n=self.n, phase=self.phase, idx=self.idx)

        if event == BinaryEvent.Yes:
            new_sql = self.get_candidate()
            return DDMinState(sql=new_sql, n=2, phase="splits", idx=0)

        elif event == BinaryEvent.No:
            next_idx = self.idx + 1
            next_phase = self.phase
            next_n = self.n

            if next_idx >= next_n:
                if next_phase == "splits" and next_n > 2:
                    next_phase = "complements"
                    next_idx = 0
                else:
                    next_n *= 2
                    next_phase = "splits"
                    next_idx = 0

            return DDMinState(sql=self.sql, n=next_n, phase=next_phase, idx=next_idx)


shared_context = {"initial_query": "", "best_query": "", "cached": {}}


async def oracle(query: str, test_script: str, reduction: int) -> GenericResult:
    cached = shared_context["cached"]
    cached_value = cached.get(query)
    if cached_value is not None:
        return cached_value

    try:
        expressions = list(sqlglot.parse(query, dialect="sqlite"))
        if not expressions:
            return GenericResult(0)
    except Exception:
        cached[query] = GenericResult(0)
        return GenericResult(0)

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
            r = GenericResult(0)
            cached[query] = r
            return r
        else:
            r = GenericResult(1 + reduction)
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


async def sequential_statements(test_script: str):
    try:
        statements = [
            expr for expr in sqlglot.parse(shared_context["best_query"], dialect="sqlite")
        ]
    except Exception:
        return

    state = DDMinState(statements)

    while True:
        next_candidate = state.get_candidate()
        if not next_candidate:
            break

        next_query_str = "".join([expr.sql(dialect="sqlite") + ";" for expr in next_candidate])

        oracle_task = asyncio.create_task(oracle(next_query_str, test_script, 0))
        _done, pending = await asyncio.wait([oracle_task], return_when=asyncio.FIRST_COMPLETED)

        for task in pending:
            _ = task.cancel()

        result = oracle_task.result()

        if result.is_dead():
            state = state.step(BinaryEvent.No)
        else:
            state = state.step(BinaryEvent.Yes)
            shared_context["best_query"] = next_query_str


async def sequential_tokens(test_script: str):
    state = DDMinState([token.text for token in sqlglot.tokenize(shared_context["best_query"])])

    while True:
        next = state.get_candidate()

        if not next:
            break

        next = "  ".join(next)

        oracle_task = asyncio.create_task(oracle(next, test_script, 0))

        _done, pending = await asyncio.wait([oracle_task], return_when=asyncio.FIRST_COMPLETED)

        for task in pending:
            _ = task.cancel()

        result = oracle_task.result()

        if result.is_dead():
            state = state.step(BinaryEvent.No)
        else:
            state = state.step(BinaryEvent.Yes)
            shared_context["best_query"] = next


async def sequential_chars(test_script: str):
    state = DDMinState(list(shared_context["best_query"]))

    while True:
        next = state.get_candidate()

        if not next:
            break

        next = "".join(next)

        oracle_task = asyncio.create_task(oracle(next, test_script, 0))

        _done, pending = await asyncio.wait([oracle_task], return_when=asyncio.FIRST_COMPLETED)

        for task in pending:
            _ = task.cancel()

        result = oracle_task.result()

        if result.is_dead():
            state = state.step(BinaryEvent.No)
        else:
            state = state.step(BinaryEvent.Yes)
            shared_context["best_query"] = next


def update_best(reduced: str):
    if len(reduced) < len(shared_context["best_query"]):
        shared_context["best_query"] = reduced


async def ddmin_runner(test_script: str, algo: Any) -> str:
    workers = [asyncio.create_task(algo(test_script)) for _ in range(1)]

    _ = await asyncio.gather(*workers)

    return shared_context["best_query"]


async def main(query_path: str, test_script: str, on_chars: bool):
    with open(query_path, "r", encoding="utf-8") as f:
        initial_query = f.read()

    shared_context["initial_query"] = initial_query
    shared_context["best_query"] = initial_query

    print(f"Initial length: {len(initial_query)}")

    while True:
        current_best = len(shared_context["best_query"])

        print("DDMin step Stmt...")
        reduced = await ddmin_runner(test_script, sequential_statements)

        print("DDMin step Tokens...")
        reduced = await ddmin_runner(test_script, sequential_tokens)

        if on_chars:
            print("DDMin step n chars...")
            reduced = await ddmin_runner(test_script, sequential_chars)

        print("Hierarchical step...")
        reduced = await reduce_sql_text(reduced, test_script)

        update_best(reduced)

        print(f"Current best: {len(shared_context['best_query'])}")

        if len(shared_context["best_query"]) == current_best:
            break

    print(f"Reduced Query: {shared_context['best_query']}")


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description="SQL Test-Case Reducer")
    _ = parser.add_argument(
        "--query", required=True, help="Path to the target SQL query to minimize.", type=str
    )
    _ = parser.add_argument(
        "--test", required=True, help="Path to the external verification oracle script.", type=str
    )
    _ = parser.add_argument(
        "--on_chars", default=False, type=bool, help="ddmin on chars instead of tokens"
    )
    args = parser.parse_args()

    if sys.platform == "win32":
        asyncio.run(main(args.query, args.test, args.on_chars))
    else:
        import uvloop

        uvloop.run(main(args.query, args.test, args.on_chars))
