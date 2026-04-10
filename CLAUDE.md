# glab-dash

Ultra-fast TUI for managing GitLab issues and merge requests across teams.

## Build & Run

```bash
cargo build              # dev build
cargo build --release    # optimized release build
cargo run                # run (requires config)
cargo test               # run all tests
cargo fmt                # format code
cargo clippy             # lint
```

## Config

Config file: `~/.config/glab-dash/config.yaml` (or set `GLAB_DASH_CONFIG` env var)

```yaml
gitlab_url: "https://gitlab.com"
token: "glpat-xxx"
me: "username"
tracking_project: "org/team-tracker"
teams:
  - name: "team1"
    members: ["alice", "bob"]
  - name: "team2"
    members: ["charlie", "dave"]
filters:
  - name: "My open MRs"
    kind: merge_request
    conditions:
      - { field: author, op: eq, value: "$me" }
      - { field: state, op: eq, value: opened }
```

Env var overrides: `GITLAB_URL`, `GITLAB_TOKEN`, `GITLAB_PROJECT`

## Architecture

- **Rust** + **ratatui** + **crossterm** + **tokio** + **reqwest**
- Edition 2024, rustfmt edition 2024
- Async event loop: crossterm events on a thread, API calls via tokio::spawn, messages over mpsc channels
- Views: Dashboard, IssueList, MrList, IssueDetail, MrDetail
- Overlays: Help, FilterEditor, Picker, ConfirmDialog, CommentInput
- Filter system: composable conditions applied client-side for instant filtering

### FocusedItem — centralized view context

`FocusedItem` (`src/app.rs`) captures the currently focused issue or MR. It is the **single source of truth** for context-dependent behavior:

- **Key handlers** read `self.focused` to dispatch actions (e.g., `do_set_status()`, `do_toggle_state()` work for any view without knowing which list/detail is active)
- **Status bar** shows per-view hints derived from `self.view`
- **Help overlay** shows per-view sections derived from `self.view`

`refresh_focused()` rebuilds `self.focused` from the current view + selection. It must be called after: view changes, list selection changes, data loads.

### Key binding scheme

**Global keys** (all views, skipped during search mode):
- `q` — back / quit, `Esc` — back, `?` — help, `1-9` — switch team
- `h` — go to Dashboard, `i` — go to IssueList, `m` — go to MrList

**Issue views** (IssueList, IssueDetail):
- `s` — set status (chord picker with all statuses)
- `x` — close/reopen (chord picker filtered to done/canceled category statuses, or simple confirm if none)

**MR views** (MrList, MrDetail):
- `A` — approve, `M` — merge, `x` — close MR

**Shared list keys**: `j/k` nav, `g/G` top/bottom, `/` search, `r` refresh, `o` browser, `l` labels, `a` assignee, `c` comment, `f/F/p` filters, `Tab` filter bar

When adding new context-dependent behavior, read from `self.focused` instead of looking up items ad-hoc.

## Key Directories

- `src/app.rs` — Main state machine, view routing, event handling
- `src/config.rs` — Config loading
- `src/event.rs` — Crossterm event handler
- `src/gitlab/` — API client and types
- `src/filter/` — Filter condition model, matching engine, tests
- `src/ui/views/` — View rendering (dashboard, issue_list, mr_list, etc.)
- `src/ui/components/` — Reusable UI components (status_bar, filter_bar, picker, etc.)
- `src/ui/styles.rs` — Color/style definitions
- `src/ui/keys.rs` — Key detection helpers

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

Tests are in `src/filter/tests.rs` and `src/config_tests.rs`. Run with `cargo test`.
