# glab-dash

Ultra-fast TUI for managing GitLab issues and merge requests across teams.

Built with Rust, ratatui, and the GitLab GraphQL API. Data is cached in SQLite for instant startup; incremental fetches keep things in sync.

## Install

```bash
# From source
cargo install --path .

# Or just build
cargo build --release
```

## Setup

Create `~/.config/glab-dash/config.toml`:

```toml
gitlab_url = "https://gitlab.com"
token = "glpat-xxx"               # needs api scope
me = "your-username"

# Projects whose issues/MRs you want to track
tracking_project = "org/team-tracker"
# Or multiple:
# tracking_projects = ["org/project-a", "org/project-b"]

# Team members (issues assigned to them are also fetched)
[[teams]]
name = "my-team"
members = ["alice", "bob", "charlie"]

# Optional: additional teams
[[teams]]
name = "platform"
members = ["dave", "eve"]
```

You can override with environment variables: `GITLAB_URL`, `GITLAB_TOKEN`, `GITLAB_PROJECT`.

Or set `GLAB_DASH_CONFIG` to use a config file at a custom path.

### Saved filters

Add reusable filters to your config:

```toml
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

## Views

Switch views with number keys:

| Key | View | Description |
|-----|------|-------------|
| `1` | Dashboard | Iteration board with health metrics |
| `2` | Issues | All tracked issues |
| `3` | Merge Requests | All tracked MRs |
| `4` | Planning | Issues grouped by iteration |

## Key Bindings

Press `?` in any view to see the full help overlay.

### Global

| Key | Action |
|-----|--------|
| `1-4` | Switch view |
| `q` / `Ctrl+c` | Quit |
| `Esc` | Go back / close overlay |
| `?` | Toggle help |
| `t` | Switch team |
| `E` | Show last error |

### Lists (Issues, MRs, Planning, Dashboard)

| Key | Action |
|-----|--------|
| `j/k` | Move down/up |
| `g/G` | Jump to top/bottom |
| `Ctrl+d/u` | Page down/up |
| `Enter` | Open detail view |
| `/` | Fuzzy search |
| `r` | Refresh |
| `R` | Full refresh (re-fetch all) |
| `o` | Open in browser |
| `f` | Filter menu |
| `F` | Clear all filters |
| `S` | Sort by field |
| `Tab` | Focus filter bar |

### Issue Actions

| Key | Action |
|-----|--------|
| `s` | Set status |
| `x` | Close / Reopen |
| `l` | Set labels |
| `a` | Set assignee |
| `c` | Add comment |
| `i` | Move to iteration |

### MR Actions

| Key | Action |
|-----|--------|
| `A` | Approve MR |
| `M` | Merge MR |
| `x` | Close MR |
| `l` | Set labels |
| `a` | Set assignee |
| `c` | Add comment |

### Detail View

| Key | Action |
|-----|--------|
| `j/k` | Scroll down/up |
| `o` | Open in browser |
| `r` | Reply to thread (or new comment if no threads) |

### Dashboard

| Key | Action |
|-----|--------|
| `Tab` | Toggle health panel / board focus |
| `[/]` | Switch column |

## Data & Cache

Data is stored in `~/.cache/glab-dash/data.db` (SQLite).

On startup, cached data is loaded instantly. A background fetch syncs with GitLab incrementally (only items updated since the last fetch). Auto-refresh runs every 60 seconds by default.

### Reset fetch timestamp

If you need to re-sync without pulling every issue:

```bash
# Re-fetch everything updated in the last 24 hours
sqlite3 ~/.cache/glab-dash/data.db \
  "INSERT OR REPLACE INTO kv (key, value) VALUES ('last_fetched_at', $(date -d '24 hours ago' +%s))"

# Or clear entirely (next fetch pulls everything)
sqlite3 ~/.cache/glab-dash/data.db "DELETE FROM kv WHERE key = 'last_fetched_at'"
```

### Clear all cached data

```bash
rm ~/.cache/glab-dash/data.db
```

## Development

```bash
cargo build              # dev build
cargo run                # run
cargo test               # run tests
make all                 # format + lint + test (pre-commit check)
make install             # build release + install
```
