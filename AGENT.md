# AGENT.md — Mnema coding guide for AI assistants

This document defines how AI coding assistants (e.g. Codex, ChatGPT) should work
on this repository.

The main goal:  
**Help develop Mnema while respecting its design, architecture, and style, in small, safe steps.**

---

## 1. Your role

You are an AI coding assistant working on **Mnema**, a local-first, AI-assisted
task memory / task manager.

When you modify code in this repository, you should:

- follow the existing design and architecture
- keep changes small and focused
- prefer clarity and maintainability over cleverness
- avoid surprising the human maintainer

Always assume there is a human reviewing your changes.

---

## 2. Read this first

Before making non-trivial changes, you should read (or re-skim):

- `README.md`
- `README.ja.md` (for more nuance, if you can read Japanese)
- `docs/specification-v0.1.md`
- `docs/development-plan.md`

Treat these as the **source of truth** for:

- what Mnema is
- how data is modeled (Task, Project, Milestone, etc.)
- how the project is meant to evolve phase by phase

Do **not** invent new concepts that conflict with these documents unless you are
explicitly asked to.

---

## 3. Repository structure and responsibilities

High-level layout:

- `crates/core`
  - domain models and business logic
  - no direct DB / HTTP / UI dependencies
- `crates/infra`
  - integrations:
    - database (SQLite, etc.)
    - LLM providers (Ollama, OpenAI-compatible)
    - background job queue
  - implementations of repository traits defined in `core`
- `crates/desktop`
  - desktop application (Tauri or similar)
  - UI, user interactions, commands to call into `core` + `infra`

When adding new features:

- put **domain logic** in `core`
- put **I/O, DB, HTTP, LLM calls** in `infra`
- put **UI and user-facing flows** in `desktop`

---

## 4. Coding style and conventions

- Prefer **Rust 2021** idioms.
- Aim for:
  - clear naming
  - small, focused modules
  - explicit error handling (avoid silently ignoring errors)
- Derive `Debug`, `Clone`, `Serialize`, `Deserialize` where it makes sense.
- Use newtype IDs (`TaskId`, `ProjectId`, etc.) instead of raw strings where possible.
- For async work, keep interfaces simple and well-named.

When in doubt, imitate the existing code.

---

## 5. Domain rules you must respect

- **Priority is not a stored field.**
  - It should be derived from due dates, cost, dependencies, etc.
- **Tasks belong to projects and lists.**
  - Inbox / Personal are modeled as special lists.
- **Milestones are not just tasks.**
  - They have their own entity and fixed status like `NOT_DONE / DONE`.
- **Local-first is a core principle.**
  - The database lives inside a vault; sync is optional.
- **The AI agent is proactive but bounded.**
  - Automation levels and user preferences control how far it can go.

If a change would violate these principles, you should not apply it unless the
maintainer explicitly asks for a redesign.

---

## 6. Working on a task: step-by-step

When you are asked to implement something (e.g. “add a TaskRepository”,
“create a minimal Tauri UI for today’s tasks”), follow this pattern:

1. **Restate the task**
   - Briefly summarize what you are about to do.
2. **Locate relevant files**
   - Identify which crates / modules you will touch.
3. **Plan the change**
   - Outline the structures, functions, and steps you will add or modify.
4. **Implement in small steps**
   - Prefer a sequence of small, consistent edits over a huge diff.
5. **Add basic tests / examples if reasonable**
   - Unit tests for pure logic
   - Simple integration / usage examples when helpful
6. **Summarize the result**
   - Explain what changed and how to use it.

Avoid “big bang” refactors in a single task.

---

## 7. Things you should not do (unless explicitly asked)

- Change the **license** or remove license headers.
- Overwrite or heavily rewrite `README.md` / `README.ja.md` without request.
- Rename crates (`core`, `infra`, `desktop`) or drastically alter the layout.
- Introduce heavy dependencies without a clear reason.
- Replace the core architecture (e.g. throw away vault / local-first design).

If you think a redesign is needed, propose it in prose first instead of directly
rewriting large parts of the codebase.

---

## 8. Automation and AI behaviour

Mnema’s identity heavily depends on:

- background automation (inbox classification, scheduling, weekly reviews)
- a proactive “secretary” agent

When working on automation / AI-related code:

- keep **logs** of AI-driven actions (`AutomationLog`)
- make it possible to **review and roll back** changes
- respect user settings:
  - automation level per feature (OFF / ASK / AUTO + review / AUTO silent)
  - how proactive notifications should be

Do not hard-code aggressive behaviour; always go through settings or clear
defaults that are easy to tune later.

---

## 9. If something is unclear

When the specification or design is ambiguous:

- Prefer minimal, conservative changes over speculative design.
- Add `TODO` comments with a short note like:
  - `// TODO: Confirm with maintainer whether X should be handled here or in Y.`
- Where helpful, suggest options in prose instead of picking a drastic path
  by yourself.

Remember: you are an assistant, not the owner of the project.
