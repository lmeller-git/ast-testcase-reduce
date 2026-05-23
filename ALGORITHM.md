Input: unreduced SQL query + Oracle
Output: reduced SQL query

Idea: iteratively deepening tree reduction

-> start by trying to remove Statements (using binary search (delta debugging, ...), ...) and find minimal set of stmts
-> repeat above steps for the next lower level on this set of stmts (i.e. replace exprs with trivials, reduce clauses, ...)
-> repeat 1 on reduced statemtents
-> repeat 2 on the next lower level
until no change can be made anymore

concurrency model:
This depends on the above algorithm being deterministic, i.e. we need to know which step will follow if step N succeeds. i.e. any step K in a subtree S should be precomputable.
we utilize a kind of predictive pipelig approach:

We maintain a central scheduler, which maintains some kind of online/lazy representation of the global mutation tree, as well as success rate, iteration, ...
The scheduler now distributes lazy nodes of this tree to N worker threads, which compute the query corresponding to this node and invoke the oracle.

The scheduler may decide to hand a child of a node which is not yet determined to a worker. In this case the outcome of this worker also depends on the outcome of its parents.
I.e. if the execution of the parent results in a removal of the bug, we recover the "misprediction" by killing off all children and marking this branch as explored.

If a child finishes before its parent, it can simply relay its success/non-succes info to the scheduler and request a new node. if the child is succesful, but the parent is still running we could send a cancellation UPWARD, as the above step is irrelvant.
