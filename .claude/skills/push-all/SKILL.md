---
name: push-all
description: "Resolve, verify, and push one exact git refspec for Shift Happens safely. Use only when the user explicitly requests this workflow."
argument-hint: "Optional remote, refspec, or constraints"
disable-model-invocation: true
---

# Push All

## Outcome

Push the intended existing commits to one explicitly resolved remote branch without altering history, widening the refspec, or including uncommitted work.

Inspect branch, upstream, remotes and push URLs, local commits, worktree, current tracking refs, and repository policy (`AGENTS.md`, `README.md`). Resolve a single source:destination refspec from explicit user intent and configured metadata, normally the current branch to its configured upstream; stop on ambiguity. Record the source commit's full SHA.

## Push gate

Before contacting the remote, run the "Checks" sequence from `.claude/skills/commit-all/SKILL.md` on the exact source commit, in a temporary detached worktree so uncommitted and untracked files cannot affect the result:

1. `git worktree add --detach <scratchpad>/push-gate <sha>`, using the session scratchpad directory.
2. In that worktree, run `npm ci --ignore-scripts --prefer-offline`, then each check in order with that worktree as the working directory. Set `CARGO_TARGET_DIR` to the main checkout's `src-tauri/target` on the `cargo xwin` commands so already-built dependencies are reused. Write the combined output to `<scratchpad>/push-gate.log`.
3. Every check must exit zero, and `git -C <worktree> status --porcelain` must be empty afterward. A rewrite by `npm run build` (version synchronization) or `npm run notices` means the commit needs a follow-up commit; stop.
4. Remove the worktree with `git worktree remove <worktree>`, and run `git worktree prune` if removal fails partway. Clean up whether the gate passed or failed.

The gate passes only for that SHA. If the source moves, run it again.

Then fetch the selected remote branch, recompute ahead/behind/divergence with `git rev-list --left-right --count`, and review the outgoing commit list and changed file names. Stop on:

- behind or diverged history;
- protected-target uncertainty;
- secrets, credentials, signing material (`*.pfx`, `*.p12`, `*.pem`, `*.key`), SQLite databases, exports, backups, or large accidental binaries in outgoing commits;
- unsigned commits when repository or user configuration requires signing;
- a gate failure or gate-generated changes requiring a commit.

## Push

Confirm the source branch still points at the gated SHA, then push the exact refspec, `git push <remote> <sha>:refs/heads/<destination>`, without force, tags, all refs, mirror, new upstream tracking, `--no-verify`, rebase, merge, or amend. Treat an interactive authentication prompt (SSH passphrase, unknown host key) as distinct from a rejection and do not retry blindly.

Verify the remote result with `git ls-remote <remote> refs/heads/<destination>` matching the pushed SHA. Report the remote, refspec, commits pushed, checks run, dirty-tree exclusions, and any unresolved authentication or policy state.

## Claude Code execution

- Run only on explicit invocation. Do not commit, repair, or rewrite history inside this workflow without explicit user authority; a failed gate ends the run with its log, and `/commit-all` is the repair path.
- Follow `AGENTS.md`: the gate is static checks and builds only; never add or run tests. Perform the Git operations directly without delegating them.
- A cold gate takes several minutes. Run it with `run_in_background` or the 10-minute Bash timeout, wait for its exit status, and read `<scratchpad>/push-gate.log` when it fails. Shell state does not persist between Bash calls, so repeat `cd <worktree>` and the `XWIN_*`/`CARGO_TARGET_DIR` assignments in each command.
