# SQL Query (Test Case) Reducer

Part 2 of the project for the course "Automated Software Testing" at ETHZ Spring 2026.

## Usage
The reducer can be run on a single query using the following command, where {n} specifies the test case.

```bash
reducer --query queries/query{n}/original_test.sql --test queries/query{n}/test.sql
```

For example, the reducer can be run on query (test case) 1 using

```bash
reducer --query queries/query1/original_test.sql --test queries/query1/test.sql
```

The docker container can be started on your local machine using:

```bash
  just docker-it
```
Then, the reducer can be ran on a specific query using simply

```bash
  just run-1 n
```

where ``n`` refers to the query number.