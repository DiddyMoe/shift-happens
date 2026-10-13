# Instructions for ChatGPT models

These instructions apply to all work in this repository, including its
subdirectories. ChatGPT models and other coding agents must verify their work
directly without creating or running tests.

## Development database and versions

- This program is still under construction. Its current local application
  database is disposable; coding agents may delete and recreate it as needed to
  implement and directly verify the program, without asking for permission again.
- Recreate development data through the canonical database startup and existing
  embedded schema. Close application/database handles before removing the main
  database and its SQLite sidecars.
- Keep the application version and current schema/migration versions unchanged
  when resetting development data. A reset is not an application upgrade and
  does not require a new migration or a database-preservation workflow.
- This authorization does not change the no-tests policy below.

## No tests

- Do not create, generate, add, modify, or extend tests of any kind. This includes
  unit, integration, end-to-end, regression, smoke, snapshot, and property-based
  tests.
- Do not create test files, test suites, test fixtures, mocks, snapshots, test
  configurations, or testing framework dependencies.
- Do not run tests, test runners, coverage commands, or automated test suites,
  including existing tests.
- Do not disguise tests as temporary scripts, inline assertions, validation
  harnesses, examples with automated pass/fail checks, or evaluation scripts.
- Before running a package script, build, CI task, or other command, inspect its
  definition and relevant dependencies to ensure it does not invoke tests.
  If you cannot establish that it avoids tests, do not run it.
- Leave existing test assets in place. This policy does not authorize deleting
  tests or disabling existing CI workflows.

## Direct verification is required

For every change, perform the following verification yourself:

1. Read the affected files and relevant callers, configuration, and dependencies
   to understand the intended behavior and constraints.
2. Inspect the actual changes for correctness, completeness, consistency with
   repository conventions, and unintended effects.
3. Trace the affected behavior through the code. Examine relevant normal paths,
   edge cases, and error handling, and explain why the change meets the request.
4. Where the environment permits, exercise the affected behavior directly using
   the application, browser, CLI, or API and inspect the observed results. Keep
   these checks manual and specific to the change; do not turn them into an
   automated test or reusable testing artifact.
5. Builds, linters, format checks, and static type checks are allowed when their
   command definitions have been checked and they do not execute tests. These
   checks supplement direct inspection and do not replace it.
6. Fix any issues found, then directly recheck the affected behavior.

If runtime verification is unavailable, perform the inspection and code tracing
that are possible and state exactly what remains unverified. Never claim that
reasoning about behavior is evidence that it ran successfully.

## ChatGPT working habits

- Treat requests to "verify," "validate," or "check" as requests for direct
  verification under this policy, not permission to create or run tests.
- Do not follow a habitual code-then-tests workflow or use test-driven development.
- Do not delegate prohibited test work to another model, tool, or service.
- In the final response, briefly describe the change, the direct verification
  performed, and any remaining uncertainty.
