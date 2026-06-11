from __future__ import annotations

import argparse
import asyncio
import os
import tempfile
import time
from collections.abc import Iterable
from dataclasses import dataclass
from typing import Any

import sqlglot
from sqlglot import exp


DEFAULT_DIALECT = "sqlite"
DEFAULT_MAX_CANDIDATES = 300
DEFAULT_MAX_CANDIDATES_PER_STATEMENT = 30
SqlExpression = Any
_EXPRESSION_TYPE = exp.Expression  # type: ignore[attr-defined]


@dataclass(frozen=True)
class PathPart:
    key: str
    index: int | None = None


ExpressionPath = tuple[PathPart, ...]


@dataclass(frozen=True)
class Candidate:
    sql: str
    description: str


def parse_sql(sql: str, dialect: str = DEFAULT_DIALECT) -> SqlExpression | None:
    try:
        return sqlglot.parse_one(sql, dialect=dialect)
    except Exception:
        return None


def render_sql(tree: SqlExpression, dialect: str = DEFAULT_DIALECT) -> str | None:
    try:
        sql = tree.sql(dialect=dialect)
        if parse_sql(sql, dialect=dialect) is None:
            return None
        return sql
    except Exception:
        return None


def iter_expression_paths(root: SqlExpression) -> Iterable[tuple[ExpressionPath, SqlExpression]]:
    queue: list[tuple[ExpressionPath, SqlExpression]] = [((), root)]

    while queue:
        path, node = queue.pop(0)
        yield path, node

        for key, value in node.args.items():
            if isinstance(value, _EXPRESSION_TYPE):
                queue.append(((*path, PathPart(key)), value))
            elif isinstance(value, list):
                for index, item in enumerate(value):
                    if isinstance(item, _EXPRESSION_TYPE):
                        queue.append(((*path, PathPart(key, index)), item))


def get_at_path(root: SqlExpression, path: ExpressionPath) -> SqlExpression | None:
    current: SqlExpression = root

    for part in path:
        value = current.args.get(part.key)
        if part.index is None:
            if not isinstance(value, _EXPRESSION_TYPE):
                return None
            current = value
        else:
            if not isinstance(value, list) or part.index >= len(value):
                return None
            item = value[part.index]
            if not isinstance(item, _EXPRESSION_TYPE):
                return None
            current = item

    return current


def set_at_path(
    root: SqlExpression, path: ExpressionPath, replacement: SqlExpression | None
) -> SqlExpression | None:
    clone = root.copy()

    if not path:
        return replacement.copy() if replacement is not None else None

    parent = get_at_path(clone, path[:-1])
    if parent is None:
        return None

    part = path[-1]

    if part.index is None:
        parent.set(part.key, replacement.copy() if replacement is not None else None)
        return clone

    value = parent.args.get(part.key)
    if not isinstance(value, list) or part.index >= len(value):
        return None

    new_items = list(value)
    if replacement is None:
        if len(new_items) <= 1:
            return None
        del new_items[part.index]
    else:
        new_items[part.index] = replacement.copy()

    parent.set(part.key, new_items)
    return clone


def depth_of(path: ExpressionPath) -> int:
    return len(path)


def split_sql_statements(sql: str) -> list[str]:
    try:
        tokens = sqlglot.Tokenizer().tokenize(sql)
    except Exception:
        return [part + ";" for part in sql.split(";") if part.strip()]

    statements: list[str] = []
    start = 0
    for token in tokens:
        if token.token_type.name == "SEMICOLON":
            end = token.end + 1
            statement = sql[start:end]
            if statement.strip():
                statements.append(statement)
            start = end

    tail = sql[start:]
    if tail.strip():
        statements.append(tail)

    return statements


def replace_statement(statements: list[str], index: int, replacement: str) -> str:
    rewritten = list(statements)
    suffix = ";" if statements[index].rstrip().endswith(";") else ""
    rewritten[index] = f"{replacement.rstrip(';')}{suffix}\n"
    return "".join(rewritten)


def candidate_priority(description: str) -> int:
    if description.startswith("remove WHERE"):
        return 0
    if description.startswith("remove HAVING"):
        return 1
    if description.startswith("remove ORDER BY"):
        return 2
    if description.startswith("remove GROUP BY"):
        return 3
    if description.startswith("remove LIMIT"):
        return 4
    if description.startswith("remove JOIN"):
        return 5
    if description.startswith("replace boolean"):
        return 6
    if description.startswith("replace predicate"):
        return 7
    if description.startswith("remove") and "list item" in description:
        return 8
    if description.startswith("remove parentheses"):
        return 9
    if description.startswith("replace"):
        return 10
    return 11


def simple_literal_replacements(node: SqlExpression) -> list[SqlExpression]:
    if isinstance(node, exp.Literal):
        if node.is_string:
            return [exp.Literal.string("")]
        return [exp.Literal.number(0), exp.Literal.number(1)]

    return []


def local_replacements(node: SqlExpression) -> Iterable[tuple[str, SqlExpression | None]]:
    if isinstance(node, exp.Where):
        yield "remove WHERE", None
        return

    if isinstance(node, exp.Order):
        yield "remove ORDER BY", None
        return

    if isinstance(node, exp.Group):
        yield "remove GROUP BY", None
        return

    if isinstance(node, exp.Having):
        yield "remove HAVING", None
        return

    if isinstance(node, exp.Limit):
        yield "remove LIMIT", None
        return

    if isinstance(node, exp.Join):
        yield "remove JOIN", None
        return

    if isinstance(node, exp.And | exp.Or):
        left = node.args.get("this")
        right = node.args.get("expression")
        if isinstance(left, _EXPRESSION_TYPE):
            yield "replace boolean expression with left side", left
        if isinstance(right, _EXPRESSION_TYPE):
            yield "replace boolean expression with right side", right

    if isinstance(node, exp.Predicate):
        yield (
            "replace predicate with TRUE",
            exp.EQ(this=exp.Literal.number(1), expression=exp.Literal.number(1)),
        )

    if isinstance(node, exp.Paren):
        inner = node.args.get("this")
        if isinstance(inner, _EXPRESSION_TYPE):
            yield "remove parentheses", inner

    for replacement in simple_literal_replacements(node):
        yield f"replace {node.key} with {replacement.sql(dialect=DEFAULT_DIALECT)}", replacement


def generate_statement_candidates(
    statement: str, dialect: str = DEFAULT_DIALECT, max_candidates: int = DEFAULT_MAX_CANDIDATES
) -> list[Candidate]:
    root = parse_sql(statement, dialect=dialect)
    if root is None:
        return []

    seen: set[str] = {statement.strip()}
    candidates: list[tuple[int, Candidate]] = []

    for path, node in iter_expression_paths(root):
        if path and path[-1].index is not None:
            mutated = set_at_path(root, path, None)
            if mutated is not None:
                candidate_sql = render_sql(mutated, dialect=dialect)
                if candidate_sql and candidate_sql.strip() not in seen:
                    seen.add(candidate_sql.strip())
                    candidates.append(
                        (depth_of(path), Candidate(candidate_sql, f"remove {node.key} list item"))
                    )
                    if len(candidates) >= max_candidates:
                        break

        for description, replacement in local_replacements(node):
            mutated = set_at_path(root, path, replacement)
            if mutated is None:
                continue

            candidate_sql = render_sql(mutated, dialect=dialect)
            if not candidate_sql or candidate_sql.strip() in seen:
                continue

            seen.add(candidate_sql.strip())
            candidates.append((depth_of(path), Candidate(candidate_sql, description)))
            if len(candidates) >= max_candidates:
                break

        if len(candidates) >= max_candidates:
            break

    candidates.sort(
        key=lambda item: (
            candidate_priority(item[1].description),
            item[0],
            len(item[1].sql),
        )
    )
    return [candidate for _, candidate in candidates]


def generate_candidates(
    sql: str, dialect: str = DEFAULT_DIALECT, max_candidates: int = DEFAULT_MAX_CANDIDATES
) -> list[Candidate]:
    statements = split_sql_statements(sql)
    if len(statements) <= 1:
        return generate_statement_candidates(sql, dialect=dialect, max_candidates=max_candidates)

    seen: set[str] = {sql.strip()}
    candidates: list[Candidate] = []

    for index, statement in enumerate(statements):
        remaining = max_candidates - len(candidates)
        if remaining <= 0:
            break

        statement_budget = min(remaining, DEFAULT_MAX_CANDIDATES_PER_STATEMENT)
        for candidate in generate_statement_candidates(
            statement, dialect=dialect, max_candidates=statement_budget
        ):
            candidate_sql = replace_statement(statements, index, candidate.sql)
            if candidate_sql.strip() in seen:
                continue

            seen.add(candidate_sql.strip())
            candidates.append(
                Candidate(candidate_sql, f"statement {index + 1}: {candidate.description}")
            )

            if len(candidates) >= max_candidates:
                break

    candidates.sort(key=lambda candidate: (candidate_priority(candidate.description.split(": ", 1)[-1]), len(candidate.sql)))
    return candidates[:max_candidates]


async def oracle(query: str, test_script: str, dialect: str = DEFAULT_DIALECT) -> bool:
    if not any(parse_sql(statement, dialect=dialect) is not None for statement in split_sql_statements(query)):
        return False

    with tempfile.NamedTemporaryFile(
        mode="w", encoding="utf-8", suffix=".sql", delete=False
    ) as tmp:
        _ = tmp.write(query)
        tmp_path = tmp.name

    process = None
    try:
        env = os.environ.copy()
        env["TEST_CASE_LOCATION"] = tmp_path

        process = await asyncio.create_subprocess_exec(
            os.path.abspath(test_script),
            env=env,
            stdout=asyncio.subprocess.DEVNULL,
            stderr=asyncio.subprocess.DEVNULL,
        )
        _ = await process.wait()
        return process.returncode == 0
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
            pass


async def reduce_sql_text(
    sql: str,
    test_script: str,
    dialect: str = DEFAULT_DIALECT,
    max_passes: int = 100,
    max_time: float = -1.0,
    max_candidates: int = DEFAULT_MAX_CANDIDATES,
) -> str:
    """Greedily reduce SQL using hierarchical AST-local transformations."""
    current = sql
    cache: dict[str, bool] = {}
    now = time.time()

    for _ in range(max_passes):
        accepted = False

        for candidate in generate_candidates(
            current, dialect=dialect, max_candidates=max_candidates
        ):
            cached = cache.get(candidate.sql)
            interesting = cached
            if interesting is None:
                interesting = await oracle(candidate.sql, test_script, dialect=dialect)
                cache[candidate.sql] = interesting

            if interesting and len(candidate.sql) <= len(current):
                current = candidate.sql
                accepted = True
                break

            if max_time >= 0 and time.time() - now > max_time:
                break


        if not accepted:
            break

    return current


async def reduce_sql_file(
    query_path: str, test_script: str, dialect: str = DEFAULT_DIALECT, max_passes: int = 100
) -> str:
    with open(query_path, encoding="utf-8") as f:
        sql = f.read()

    return await reduce_sql_text(
        sql=sql, test_script=test_script, dialect=dialect, max_passes=max_passes
    )


async def main() -> None:
    parser = argparse.ArgumentParser(description="AST-based SQL test-case reducer")
    _ = parser.add_argument("--query", required=True, help="Path to the SQL query to reduce")
    _ = parser.add_argument("--test", required=True, help="Path to the oracle test script")
    _ = parser.add_argument("--dialect", default=DEFAULT_DIALECT, help="sqlglot dialect")
    _ = parser.add_argument("--max-passes", default=100, type=int)
    args = parser.parse_args()

    reduced = await reduce_sql_file(
        query_path=args.query,
        test_script=args.test,
        dialect=args.dialect,
        max_passes=args.max_passes,
    )
    print(f"Reduced query: {reduced}")


if __name__ == "__main__":
    asyncio.run(main())
