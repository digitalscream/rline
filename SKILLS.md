# Agent Skills

rline's agent supports **skills** — self-contained instruction packs that the AI loads on demand. A skill is a directory containing a `SKILL.md` file with YAML frontmatter describing *when* the skill should be used. Only the name and description appear in the system prompt (cheap — a few tokens per skill); the full body is streamed in only when the model decides the skill is relevant and calls the built-in `use_skill` tool.

Skills are **fully Cline-compatible**. Any skill that works with the Cline VS Code extension works here unchanged, and vice versa — the directory layout, file format, discovery locations, and activation mechanism are all identical.

## Why skills?

Skills sit between two existing mechanisms you may already use:

| Mechanism       | Always in context? | Good for                                    |
| --------------- | ------------------ | ------------------------------------------- |
| `.clinerules/`  | Yes                | Short, ever-present conventions (≤ ~500 tokens) |
| `memory-bank/`  | Yes                | Durable project context / architecture notes |
| **Skills**      | **No — on demand** | **Long playbooks ("how to create a PR here", "how to debug a flaky migration") that only matter for specific tasks** |

Use a skill whenever the instructions are too big or too situational to live in `.clinerules` all the time.

## Where to put skills

rline searches six locations, in this order:

**Project-local** (relative to the workspace root):

- `.cline/skills/`
- `.clinerules/skills/`
- `.claude/skills/`
- `.agents/skills/`

**Global** (shared across all workspaces):

- `~/.cline/skills/`
- `~/.agents/skills/`

### Recommended defaults

- **Project skills that belong in version control** → `.cline/skills/` (commit them)
- **Personal skills you want everywhere** → `~/.cline/skills/`

### Name collisions

If the same skill name exists in both a project and a global location, **the global copy wins**. rline logs a `tracing` warning so you notice. This matches Cline's precedence rule.

## File format

Each skill lives in its own directory. The directory name *must* match the skill's `name` field exactly (kebab-case, lowercase with hyphens is the convention).

```
.cline/skills/
└── create-pull-request/
    └── SKILL.md
```

A `SKILL.md` begins with a YAML frontmatter block:

```markdown
---
name: create-pull-request
description: Create a GitHub pull request following project conventions. Use when the user asks to create a PR, submit changes for review, or open a pull request.
---

# Create Pull Request

Step-by-step instructions the agent will follow once this skill is loaded...
```

**Required fields:**

| Field         | Description                                                                               |
| ------------- | ----------------------------------------------------------------------------------------- |
| `name`        | Must exactly match the directory name. Skills where these differ are skipped with a warn. |
| `description` | One- or two-sentence summary. **This is what the model uses to decide when to load the skill.** |

Everything after the closing `---` is the skill body — plain markdown, shown to the agent verbatim when the skill is loaded.

## How invocation works

There is no `/skill-name` command. Skills are **model-driven** by design: when you send a message to the agent, it sees a block like this at the end of the system prompt:

```
## Skills

Available skills:
  - "create-pull-request": Create a GitHub pull request following project conventions. ...
  - "investigate-flaky-test": Diagnose and fix a flaky test.  ...
  - "conventional-commits": Enforce Conventional Commits for all commit messages. ...

To use a skill:
1. Match the user's request to a skill based on its description.
2. Call `use_skill` with `skill_name` set to the exact skill name.
3. Follow the instructions returned by the tool.
4. Do NOT call `use_skill` again for the same skill within a single task.
```

If your request matches a description, the model calls the `use_skill` tool with the skill's name. rline loads `SKILL.md`, returns the body as the tool result, and the model then follows those instructions for the rest of the task.

The `use_skill` tool is available in **both Plan and Act modes**.

## Writing a good description

The `description` is the only signal the model uses to decide whether to load your skill. Prioritise *when to use it*, not just *what it does*.

**Not great:**
```yaml
description: Helps with pull requests.
```

**Better:**
```yaml
description: Create a GitHub pull request following this repo's conventions. Use when the user asks to create a PR, open a PR, submit changes for review, or raise a pull request.
```

Tips:

- Lead with the triggering intent (*"Use when the user…"*).
- List a few concrete phrasings the user might say.
- Mention the **domain / repo style** if the skill is repo-specific ("follows this repo's conventions", "for Rust crates").
- Keep it under ~200 words — brevity helps the model scan the list.

## Supporting files

Skills can include any supporting files you like — scripts, templates, checklists, worked examples. The convention is subdirectories like `scripts/`, `templates/`, or `docs/`:

```
create-pull-request/
├── SKILL.md
├── templates/
│   └── pr-body.md
└── scripts/
    └── check-green-ci.sh
```

Reference them from inside `SKILL.md` with relative paths. rline does **not** auto-load these files — the agent will read them on demand using the normal `read_file` tool, which is exactly the Cline behaviour. Keep the SKILL.md body itself short (Cline's guidance is < 5k tokens); offload the heavy detail to referenced files.

## Example skills

### Example 1 — `conventional-commits` (small)

`.cline/skills/conventional-commits/SKILL.md`:

```markdown
---
name: conventional-commits
description: Enforce Conventional Commits formatting for every commit message in this repo. Use when the user asks to commit changes, write a commit message, or prepare a PR.
---

# Conventional Commits

All commits in this repo must follow the [Conventional Commits](https://www.conventionalcommits.org/) spec.

## Required structure

<type>(<scope>): <short summary>

<body explaining the *why*, wrapped at 72 cols>

## Allowed types

- `feat`: a new user-visible feature
- `fix`: a bug fix
- `refactor`: code change that neither fixes a bug nor adds a feature
- `test`: adding or fixing tests
- `docs`: documentation only
- `chore`: build/tooling, no src change
- `perf`: performance improvement

## Rules

1. Subject line ≤ 72 characters, imperative mood ("add X", not "added X" / "adds X").
2. No trailing period on the subject.
3. Always include a body for non-trivial changes — explain WHY, not what.
4. Breaking changes use `feat!:` or `fix!:` with a `BREAKING CHANGE:` footer.
5. Scope is the crate or module name when relevant (e.g. `feat(rline-ai): ...`).

## Before committing

- Run `cargo fmt && cargo clippy -- -D warnings && cargo test --workspace`.
- Review the diff and confirm the scope matches what you actually changed.
```

### Example 2 — `create-pull-request` (larger, with supporting files)

`.cline/skills/create-pull-request/SKILL.md`:

```markdown
---
name: create-pull-request
description: Create a GitHub pull request following this repo's conventions. Use when the user asks to create a PR, open a PR, submit changes for review, or raise a pull request. Handles branch checks, PR template usage, and PR creation via the `gh` CLI.
---

# Create Pull Request

Guide the user through creating a high-quality GitHub PR for this repo.

## 1. Pre-flight checks

Run these with `execute_command`:

bash
git status --porcelain
git rev-parse --abbrev-ref HEAD
gh --version

- Fail early if `gh` is missing — ask the user to install it.
- If the branch is `main` or `master`, ask the user to create a feature branch first.
- If there are uncommitted changes, offer to commit them (apply the `conventional-commits` skill if relevant).

## 2. Push the branch

bash
git push -u origin HEAD

## 3. Draft the PR body

Read `templates/pr-body.md` in this skill directory and fill it in with:

- A one-line summary (match the top commit's subject if it captures the change).
- A bullet list of notable changes (use `git log origin/main..HEAD --oneline`).
- A "Test plan" section — what you ran, what passed.

## 4. Create the PR

Write the body to a temporary file so special characters survive:

bash
gh pr create --title "<subject>" --body-file /tmp/pr-body.md --base main

Return the resulting PR URL to the user.

## Error handling

- If `gh pr create` fails with an auth error, instruct the user to run `gh auth login`.
- If the branch already has a PR, surface the existing URL instead of erroring.
```

`.cline/skills/create-pull-request/templates/pr-body.md`:

```markdown
## Summary

<one line>

## Changes

- <change 1>
- <change 2>

## Test plan

- [ ] `cargo test --workspace`
- [ ] <manual check, if relevant>
```

The agent will `read_file` the template when it reaches step 3 of the skill.

## Disabling a skill

There is no per-skill toggle UI in v1. To disable a skill, move or rename its directory so `SKILL.md` is no longer found:

```bash
mv .cline/skills/create-pull-request .cline/skills/.create-pull-request.disabled
```

(Directories whose name starts with `.` are still scanned — but the one above lacks a matching `SKILL.md/name` pair, so it gets skipped.)

## Troubleshooting

**My skill doesn't appear in the system prompt.**

- Confirm the directory name matches the `name:` field exactly.
- Confirm `SKILL.md` starts with `---` on the very first line (no blank lines, no extra whitespace).
- Confirm both `name` and `description` are non-empty.
- Check the rline log output — malformed skills produce a `tracing::warn!` line identifying the problem.

**The model ignores my skill even though it's listed.**

- Rewrite the `description` to lead with *when* the skill should trigger ("Use when the user asks to…").
- Mention concrete phrases the user might say.

**The skill loads but the model doesn't follow the instructions.**

- Keep `SKILL.md` focused and concise. Move detailed appendices into supporting files and reference them by path.
- Avoid contradicting `.clinerules` — always-on rules trump lazy-loaded skill bodies.

## Related documents

- [README.md](README.md) — project overview.
- [CLAUDE.md](CLAUDE.md) — contributor guide and project conventions.
- [Cline skills documentation](https://docs.cline.bot/) — reference for the upstream format; anything documented there works here.
