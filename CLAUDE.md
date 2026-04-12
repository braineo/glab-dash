# glab-dash

Ultra-fast TUI for managing GitLab issues and merge requests across teams.

## Build & Run

```bash
cargo build              # dev build
cargo build --release    # optimized release build
cargo run                # run (requires config)
cargo test               # run all tests
cargo fmt                # format code
cargo clippy             # lint (pedantic, must pass with zero warnings)
typos                    # spell check
make lint                # auto-fix clippy warnings (must run before committing)
make all                 # format + lint + test (full pre-commit check)
```

## Code Quality

All code must pass these checks before committing (enforced by CI):

1. **`cargo fmt --check`** — all code formatted
2. **`cargo clippy`** — zero warnings; `clippy::pedantic` is enabled in `Cargo.toml` with `warnings = "deny"` in `[lints.rust]`
3. **`cargo build`** — zero warnings (dead code, unused imports, etc.)
4. **`cargo test`** — all tests pass
5. **`typos`** — no spelling errors (`_typos.toml` has exceptions)

**Before committing, always run `make lint`** (`cargo clippy --all-targets --all-features --fix --allow-dirty -- -D warnings`) to auto-fix clippy warnings. This matches the CI clippy check and prevents pipeline failures.

Pedantic lint exceptions are configured in `[lints.clippy]` in `Cargo.toml`. Do not add new `#[allow(...)]` attributes without good reason — prefer fixing the lint.

**No defensive serde parsing**: Do not use `#[serde(default)]` on GraphQL response structs. The GraphQL schema defines a fixed shape — trust it. `Option<T>` already handles nullable fields correctly without `default`.

## Config

Config file: `~/.config/glab-dash/config.toml` (or set `GLAB_DASH_CONFIG` env var)

```toml
gitlab_url = "https://gitlab.com"
token = "glpat-xxx"
me = "username"
tracking_project = "org/team-tracker"

[[teams]]
name = "team1"
members = ["alice", "bob"]

[[teams]]
name = "team2"
members = ["charlie", "dave"]

[[filters]]
name = "My open MRs"
kind = "merge_request"

[[filters.conditions]]
field = "author"
op = "eq"
value = "$me"

[[filters.conditions]]
field = "state"
op = "eq"
value = "opened"
```

Env var overrides: `GITLAB_URL`, `GITLAB_TOKEN`, `GITLAB_PROJECT`

## Architecture

- **Rust** + **ratatui** + **crossterm** + **tokio** + **reqwest** + **rusqlite**
- Edition 2024, rustfmt edition 2024
- Async event loop: crossterm events on a thread, API calls via tokio::spawn, messages over mpsc channels
- **TEA-inspired update cycle**: `handle → reconcile(dirty flags) → execute(cmds)` — see below
- **SQLite persistence**: targeted per-table writes instead of full serialization
- Views: Dashboard, IssueList, MrList, IssueDetail, MrDetail, Planning
- Overlays: Help, FilterEditor, Picker, ConfirmDialog, CommentInput
- Filter system: composable conditions applied client-side for instant filtering

### Update cycle (TEA-inspired)

State updates follow a **handle → reconcile → execute** pattern inspired by The Elm Architecture:

```
Event (key press / async result / timer)
  → handle: mutate state, set dirty flags, push Cmd values
    → reconcile(&dirty): refilter/refresh only what changed
      → execute(cmds): DB writes, API spawns, side effects
```

**`Dirty`** (`src/cmd.rs`): flags tracking which data domains changed (`issues`, `mrs`, `labels`, `iterations`, `statuses`, `view_state`, `selection`). Set by handlers, consumed by `reconcile()`.

**`Cmd`** (`src/cmd.rs`): side-effect descriptors. Handlers push cmds to `self.pending_cmds`; the event loop drains them via `execute_cmd()`. Variants cover targeted SQLite writes (`PersistIssues`, `PersistMrs`, etc.), API fetches, and async task spawns.

**`reconcile()`** (`src/app.rs`): single function that runs all downstream updates based on dirty flags. Dependency map:

| Dirty flag | Triggers |
|------------|----------|
| `issues` | `refilter_issues`, `refilter_planning`, `refilter_iteration_board`, `refresh_focused`, `compute_iteration_health` |
| `mrs` | `refilter_mrs`, `refresh_focused` |
| `iterations` | `refilter_planning`, `refilter_iteration_board`, `refresh_focused`, `compute_iteration_health` |
| `statuses` | `rebuild_iteration_board_columns`, `refilter_iteration_board`, `refresh_focused`, `compute_iteration_health` |
| `view_state` | all refilters + `refresh_focused` |
| `selection` | `refresh_focused` |

**Never call `refilter_*()`, `refresh_focused()`, or `compute_iteration_health()` directly from handlers.** Set the appropriate dirty flags and let `reconcile()` handle it.

**`process_async_msg()` / `process_key()`**: wrappers called from the event loop that run the full handle → reconcile → execute cycle.

### Persistence (SQLite)

**`Db`** (`src/db.rs`): SQLite-backed storage at `~/.cache/glab-dash/data.db`. Hybrid schema: scalar columns for queries + JSON `data` column for full serde round-trip.

Tables: `issues`, `merge_requests`, `labels`, `iterations`, `work_item_statuses`, `kv` (key-value for view state, label usage, scope creep dates, `last_fetched_at`).

**Key behaviors:**
- `IssuesLoaded`/`MrsLoaded` persist ALL items (open + closed) to DB before filtering to open-only in memory — closed issues accumulate in DB for shadow work queries
- `load_from_db()` restores `last_fetched_at` from DB so restarts use fast incremental fetches
- `query_shadow_work()` queries closed issues by date range directly from DB
- `ViewState` (`src/db.rs`): serializable filter/sort/fuzzy state, persisted to `kv` table

### List data pipeline

All list views follow a 4-stage pipeline:

```
Source (Vec<TrackedIssue> or Vec<TrackedMergeRequest>, shared on App)
  → Prefilter (view-specific: none / by iteration / by status)
    → User filter + sort + fuzzy search (per-view, via UserFilter)
      → filtered indices (rendered by the view)
```

**`ItemList<T>`** (`src/ui/views/list_model.rs`): generic list holding `TableState` + `Vec<usize>` indices into source data. Provides selection navigation (`select_next`, `select_prev`, `select_first`, `select_last`, `page_down`, `page_up`), selection accessors (`selected_item`, `selected_index`), and `clamp_selection`. Used by all list views.

**`UserFilter`** (`src/ui/views/list_model.rs`): bundle of filter conditions, sort specs, fuzzy query, and filter bar state. Each view owns its own instance(s). Methods: `handle_fuzzy_input`, `fuzzy_matches`, `start_search`.

**Per-view scope:**

| View | Prefilter | Filter/sort/fuzzy |
|------|-----------|-------------------|
| IssueList | none | 1 `UserFilter` |
| MrList | none | 1 `UserFilter` |
| Planning | iteration per column | 1 `UserFilter` per column (`PlanningColumn`) |

Filter/sort state lives **in the view**, not on `App`. Views own their `ItemList` and `UserFilter` directly.

### FocusedItem — centralized view context

`FocusedItem` (`src/app.rs`) captures the currently focused issue or MR. It is the **single source of truth** for context-dependent behavior:

- **Key handlers** read `self.focused` to dispatch actions (e.g., `do_set_status()`, `do_toggle_state()` work for any view without knowing which list/detail is active)
- **Status bar** shows context-specific hints derived from the binding registry
- **Help overlay** shows per-view key sections derived from the binding registry
- **Tab bar** shows top-level view navigation (Dashboard, Issues, MRs, Planning)

`refresh_focused()` rebuilds `self.focused` from the current view + selection. It is called automatically by `reconcile()` when `dirty.selection` (or any data flag) is set — do not call it directly from handlers.

### Key dispatch and binding registry

Key dispatch uses a **4-mode system** derived from current app state (`src/app.rs: input_mode()`):

| Mode | When | Behavior |
|------|------|----------|
| `TextInput` | Fuzzy search active, CommentInput/Picker/FilterEditor(value step) overlay | All chars go to text widget |
| `Chord` | Chord overlay active | Home-row keys select; anything else cancels |
| `Modal` | Help/Confirm/Error overlay | Overlay-specific keys only |
| `Normal` | Everything else | Binding registry dispatch |

In Normal mode, keys are matched against the **binding registry** (`src/keybindings.rs`). Each `Binding` pairs a `KeyMatcher` with a `KeyAction`, a display label, and a description. Bindings are grouped into `BindingGroup`s (e.g., `GLOBAL_BINDINGS`, `LIST_NAV_BINDINGS`, `ISSUE_ACTION_BINDINGS`). The function `binding_groups_for_view(view)` composes groups per view — first match wins.

This is the **single source of truth**: the same binding definitions drive dispatch, help overlay rendering, and status bar hints. When a binding is added or removed, all three update automatically.

**To add a new key binding:**
1. Add a `KeyAction` variant in `src/keybindings.rs`
2. Add a `Binding` entry to the appropriate group constant (e.g., `ISSUE_ACTION_BINDINGS`)
3. Handle the action in `execute_action()` in `src/app.rs`

**To add a new binding group:**
1. Define the `static` binding array and `BindingGroup`
2. Add it to the relevant views in `binding_groups_for_view()`

When adding new context-dependent behavior, read from `self.focused` instead of looking up items ad-hoc.

## Key Directories

- `src/app.rs` — Main state machine, view routing, key dispatch, update cycle (`process_key`, `process_async_msg`, `reconcile`, `execute_cmd`)
- `src/cmd.rs` — `Cmd` enum (side-effect descriptors), `Dirty` flags
- `src/db.rs` — SQLite persistence layer (`Db`, `ViewState`, schema, CRUD, shadow work queries)
- `src/keybindings.rs` — Binding registry: `KeyAction`, `Binding`, `BindingGroup`, `InputMode`, `KeyMatcher`, binding constants, `binding_groups_for_view()`
- `src/config.rs` — Config loading
- `src/main.rs` — Entry point, terminal setup, event loop (`tokio::select!` over crossterm events, async messages, refresh timer)
- `src/gitlab/` — API client and types
- `src/filter/` — Filter condition model, matching engine, tests
- `src/ui/views/list_model.rs` — `ItemList<T>`, `UserFilter`, shared helpers
- `src/ui/views/` — View rendering (dashboard, issue_list, mr_list, planning, etc.)
- `src/ui/components/` — Reusable UI components (tab_bar, status_bar, filter_bar, help, picker, etc.)
- `src/ui/styles.rs` — Color/style definitions
- `src/ui/keys.rs` — Key detection helpers (used by overlay handlers and filter bar)

## Style & Accessibility

- **WCAG AA contrast required**: All text must meet 4.5:1 contrast ratio against its background (3:1 for bold/large text). This applies to all UI layers:
  - Base background `(26,27,38)`: use `TEXT`, `TEXT_BRIGHT`, or accent colors
  - Surface background `(36,40,59)`: use `TEXT`, `TEXT_BRIGHT`, or accent colors
  - Overlay background `(52,59,88)`: use `OVERLAY_TEXT`, `OVERLAY_TEXT_DIM`, or `overlay_*_style()` functions — **never** use `TEXT_DIM` or `help_desc_style()` on overlay backgrounds
- Use `overlay_key_style()`, `overlay_desc_style()`, `overlay_text_style()` for any text rendered inside overlays/popups (help, picker, confirm, error, filter editor)
- Use `help_key_style()`, `help_desc_style()` only on base/surface backgrounds
- Labels render as powerline-style chip spans via `label_spans()` — no `::` shown, segments joined by `\u{E0B0}` arrows
- Scoped labels (`a::b::c`) split into colored segments; first scope uses server label color, rest from curated palette (16 colors in `CHIP_PALETTE`)
- Non-scoped labels use server color when available, else curated palette
- **`RenderCtx`** (`src/ui/mod.rs`): shared context passed to all render functions. Add new server-derived or global state here instead of adding parameters to every render function

## Tests

Tests are in `src/filter/tests.rs`, `src/config_tests.rs`, `src/onboarding_tests.rs`, `src/ui/views/list_model_tests.rs`, `src/db.rs` (inline `#[cfg(test)]`), and `src/ui/views/dashboard.rs` (inline `#[cfg(test)]`). Run with `cargo test`.
