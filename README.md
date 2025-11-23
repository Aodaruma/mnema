# Mnema

> Carry no tasks in your head.

Mnema is a local-first, AI-assisted **task memory** with an autonomous agent.
You throw in everything you need to do, and Mnema remembers, structures, and
proactively reviews it with you.

* Local-first vault with optional sync
* AI-powered inbox triage, scheduling, and weekly reviews
* Project-based views (list, board, calendar, Gantt)
* A customizable, proactive “secretary” agent as your interface

> Status: **early design / prototyping phase**.
> APIs, structure, and technologies may change quickly.

---

## Why Mnema?

Your brain is good at thinking, not at storing todos.

Mnema is designed to:

* keep tasks **out of your head** and **inside a trusted system**
* turn short commands and loose notes into **projects, milestones, and schedules**
* help you regularly **review your week** with an AI “secretary” in your own tone
* take the initiative with reminders and light replanning when things get busy or stuck

Conceptually, Mnema sits somewhere between:

* a task manager
* a project planner (with milestones and Gantt)
* and an external memory that learns your habits and preferences

---

## Core ideas

### Local-first by default

* Vaults live on your machine (in a directory like `./mnema-vault/`).
* Sync is **optional**, not required (similar to Obsidian Sync vs. local vault).
* The local database (e.g. SQLite) is the source of truth for tasks and structure,
  and project descriptions / user preferences are also stored as Markdown in the
  same vault so your data stays portable and readable.

### AI as a secretary, not a boss

Mnema treats LLMs as a **self-directed “secretary”**, not just a passive chatbot.

* LLMs propose:

  * classification (inbox → project / personal / etc.)
  * dates and schedules
  * milestones and sub-tasks
  * weekly review summaries
  * proactive reminders and gentle replans when deadlines approach or tasks stall
* You keep the final say:

  * suggested changes can be reviewed
  * AI-driven changes are logged
  * basic rollbacks are possible per task/project
* You control automation levels per feature:

  * OFF / ASK / AUTO + review
* You can also decide **how proactive** Mnema may be (what kinds of notifications
  and suggestions are acceptable).

### Projects, milestones, and views

* **Projects**: group tasks and milestones with a time span and description.
* **Tasks**: carry due dates, cost points, dependencies, etc.
  `priority` is *not* stored permanently; it is derived from due dates, cost,
  dependencies, and other factors.
* **Milestones**: represent checkpoints and key events in a project.
  They use a fixed status set like `NOT_DONE / DONE`, and “overdue” can be
  inferred from the target date.

Planned views:

* List
* Board (status-based)
* Calendar
* Gantt (per project)

### Second memory, not a second job

Mnema tries to **reduce** friction, not add more:

* Frictionless capture (global shortcut → inbox)
* Slash-commands for dates, estimates, projects, etc. (e.g. `/due`, `/estimate`)
* Minimal required fields; most structure can be suggested by the AI
* When your workload is skewed or you seem stuck, Mnema can proactively propose:

  * “here are the 3 tasks to focus on now”
  * or a light, suggested plan for the next few days

---

## Architecture (high-level)

> Draft – see `docs/specification-v0.1.md` for details.

* **Language**: Rust (core logic and backend)
* **Desktop Shell**: TBD (Tauri is the current favorite; still evaluating)
* **Storage**: Local RDBMS (likely SQLite) inside a “vault” folder
* **LLM layer**:

  * pluggable providers (local via Ollama, cloud via OpenAI-compatible APIs)
  * separation between “planning” models and “routine” models
* **Automation**:

  * background jobs for:

    * inbox classification
    * due date / schedule suggestion
    * weekly review preparation
    * proactive notifications and suggestions (e.g. end-of-day or AFK windows)

---

## Repository layout (planned)

This is likely to change, but as a rough idea:

```text
mnema/
  README.md
  README.ja.md
  CONTRIBUTING.md
  LICENSE
  docs/
    specification-v0.1.md
  crates/
    core/         # domain models, services
    infra/        # DB, LLM clients, sync
    desktop/      # desktop app (Tauri / other)
```

---

## Documentation

* Design / specification (early draft):

  * `docs/specification-v0.1.md`

More detailed docs (API, UI flows, etc.) will be added as the project evolves.

---

## Contributing

Contributions, ideas, and feedback are very welcome.

* Please read **`CONTRIBUTING.md`** before opening large PRs.
* For now, issues can be used both for:

  * bug reports
  * design / architecture discussions

---

## License

Mnema is currently licensed under the **Apache License 2.0**.
See the `LICENSE` file for details.
