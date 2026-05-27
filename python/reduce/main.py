import asyncio
import random
from lib_tr import BFScheduler, DDMinPath


def add(n1: int, n2: int) -> int:
    return n1 + n2


def algorithm(path: DDMinPath) -> str:
    print(f"Evaluating path {path}")
    return f"{sum(path.to_list())}"


async def oracle(query: str) -> bool:
    await asyncio.sleep(random.random())
    n_true = int(query)
    return (n_true + 1) % 2 == 0


async def worker(scheduler: BFScheduler):
    for _ in range(10):
        cancel_event = asyncio.Event()

        path = scheduler.next(cancel_event)

        if path is None:
            break

        query = algorithm(path)

        if cancel_event.is_set():
            continue

        cancel_task = asyncio.create_task(cancel_event.wait())
        oracle_task = asyncio.create_task(oracle(query))

        done, pending = await asyncio.wait(
            [cancel_task, oracle_task], return_when=asyncio.FIRST_COMPLETED
        )

        for task in pending:
            _ = task.cancel()

        if cancel_task in done:
            continue

        result = oracle_task.result()
        scheduler.put_result(path, result)


async def main():
    scheduler = BFScheduler()

    workers = [asyncio.create_task(worker(scheduler)) for _ in range(5)]

    _ = await asyncio.gather(*workers)


if __name__ == "__main__":
    asyncio.run(main())
