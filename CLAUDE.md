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
- Scoped labels (`scope::value`) render with distinct colors via `label_spans()` — scope in blue bold, value in teal

## Tests

Tests are in `src/filter/tests.rs` and `src/config_tests.rs`. Run with `cargo test`.
