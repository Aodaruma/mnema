# Mnema

> Carry no tasks in your head.

Mnema is a local-first, AI-assisted **task memory** with an autonomous agent.
You throw in everything you need to do, and Mnema remembers, structures, and
proactively reviews it with you.

* Local-first vault with optional sync
* AI-powered inbox triage, scheduling, and weekly reviews
* Project-based views (list, board, calendar, Gantt)
* A customizable, proactive “secretary” agent as your interface

> Status: **Scheduling MVP alpha (S0-S3 implemented)**.
> CLI, desktop, REST/Web, Google Calendar, Habits, life/sleep hours, travel buffers,
> and reviewed preview/apply are available. See `docs/development-plan.md`.

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
* SQLite is the default source of truth for tasks and structure so the app can
  run locally without Docker or a database install. The vault directory still
  stores project notes, user preferences, assets, and exports so file-based data
  stays portable and readable.
* PostgreSQL remains an optional backend for self-hosted/server-style setups.

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
* **Desktop Shell**: egui / eframe for the current prototype; Slint or Tauri can
  be revisited once UI requirements stabilize
* **Storage**: SQLite by default, optional PostgreSQL via
  `MNEMA_STORAGE_BACKEND=postgres` and `MNEMA_DATABASE_URL`, plus a local vault
  folder for files, assets, exports, and app-local configuration
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

## Database

Mnema uses SQLite by default. No Docker or PostgreSQL install is required for the
local app flow.

The SQLite database is stored outside the vault by default to avoid sync-folder
file locking issues:

* Windows: `%LOCALAPPDATA%\Mnema\mnema.sqlite`
* Linux/macOS-style fallback: `$XDG_DATA_HOME/mnema/mnema.sqlite` or
  `$HOME/.local/share/mnema/mnema.sqlite`

You can override it with `MNEMA_SQLITE_PATH`.

PostgreSQL is optional:

```bash
export MNEMA_STORAGE_BACKEND=postgres
export MNEMA_DATABASE_URL=postgres://postgres:postgres@localhost/mnema
```

If `MNEMA_DATABASE_URL` is unset in PostgreSQL mode, development builds try
`postgres://postgres:postgres@localhost/mnema`.

You can try the scheduler without a database:

```bash
cargo run -p mnema-desktop -- --demo-plan
```

Try the headless Scheduling MVP:

```bash
cargo run -p mnema-cli -- --help
cargo run -p mnema-cli -- hours set --timezone Asia/Tokyo --work-start 09:00 --work-end 17:00 --sleep-start 23:00 --sleep-end 07:00 --travel-minutes 15
cargo run -p mnema-cli -- habit add "Morning walk" --minutes 30 --earliest 07:00 --latest 09:00
cargo run -p mnema-cli -- task add "Write proposal" --due 2026-08-20 --minutes 60
cargo run -p mnema-cli -- plan preview --days 7 --timezone Asia/Tokyo
```

`plan apply` requires the fingerprint printed by preview. See `calendar --help`
and `crates/server/README.md` for Google Calendar and credential setup.

Launch the current desktop GUI:

```bash
cargo run -p mnema-desktop
```

On Windows, the development launcher gives the same GUI flow without remembering
the cargo arguments:

```powershell
.\scripts\dev-desktop.ps1
```

Create a simple release folder:

```powershell
.\scripts\package-desktop-windows.ps1
```

Then run:

```powershell
.\dist\mnema-desktop-windows\run-mnema.ps1
```

Launch the REST API and responsive Web GUI:

```bash
cargo run -p mnema-server
```

Open `http://127.0.0.1:8080`. Docker users can run
`docker compose -f deploy/compose.yaml up --build`. Put the server behind an
authenticated TLS reverse proxy before exposing it beyond loopback.

With the default SQLite backend:

```bash
cargo run -p mnema-desktop -- add "Write first task" --due 2026-06-25 --minutes 45
cargo run -p mnema-desktop -- list
cargo run -p mnema-desktop -- plan --save
cargo run -p mnema-desktop -- schedule
```

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
    development-plan.md
    specification-v0.1.md
  crates/
    core/         # domain models, services
    scheduler/    # deterministic planning and schedule proposal logic
    app/           # use cases and application services
    infra/        # DB, LLM clients, sync
    desktop/      # desktop GUI (egui + eframe)
    cli/          # headless scheduling CLI
    server/       # REST API, worker, and embedded Web GUI
```

---

## Documentation

* Design / specification (early draft):

  * `docs/development-plan.md` (active short-term roadmap)
  * `docs/specification-v0.1.md`
  * `docs/desktop-runbook-2026-06-26.md`

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
