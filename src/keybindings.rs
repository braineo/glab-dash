use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use crate::app::View;

// ---------------------------------------------------------------------------
// InputMode — derived from FocusTarget, never stored
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InputMode {
    /// Keys dispatch to navigation, commands, global bindings.
    Normal,
    /// All chars go to the active text widget (search, picker, comment editor).
    TextInput,
    /// Home-row keys select a chord option; anything else cancels.
    Chord,
    /// Overlay-specific modal (Help: any key dismisses; Confirm: y/n/Esc; Error: any key).
    Modal,
}

// ---------------------------------------------------------------------------
// KeyAction — unified action enum replacing per-view actions
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeyAction {
    // --- Global ---
    Back,
    ToggleHelp,
    ShowLastError,
    SwitchTeam,
    NavigateTo(View),

    // --- List / column navigation ---
    MoveUp,
    MoveDown,
    Top,
    Bottom,
    PageUp,
    PageDown,
    OpenDetail,

    // --- Search & Filter ---
    StartSearch,
    FocusFilterBar,
    AddFilter,
    ClearFilters,
    ApplyPreset(u8),
    PickPreset,
    PickSortPreset,

    // --- Shared item actions (resolved via FocusedItem) ---
    Refresh,
    OpenBrowser,
    SetStatus,
    ToggleState,
    EditLabels,
    EditAssignee,
    Comment,

    // --- MR-specific ---
    Approve,
    Merge,

    // --- Detail-specific ---
    ReplyThread,

    // --- Board / column navigation (Dashboard & Planning) ---
    ColumnLeft,
    ColumnRight,

    // --- Planning-specific ---
    ToggleColumnPrev,
    ToggleColumnNext,
    ToggleLayout,
    MoveIteration,
}

// ---------------------------------------------------------------------------
// KeyMatcher — how a binding matches key events
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy)]
pub enum KeyMatcher {
    /// Character key with no modifiers: KeyCode::Char(c), mods == NONE.
    Char(char),
    /// Character key with Control: KeyCode::Char(c), mods contains CONTROL.
    Ctrl(char),
    /// Non-character key with no modifiers.
    Key(KeyCode),
}

impl KeyMatcher {
    pub fn matches(self, key: &KeyEvent) -> bool {
        match self {
            Self::Char(c) => key.code == KeyCode::Char(c) && key.modifiers == KeyModifiers::NONE,
            Self::Ctrl(c) => {
                key.code == KeyCode::Char(c) && key.modifiers.contains(KeyModifiers::CONTROL)
            }
            Self::Key(code) => key.code == code && key.modifiers == KeyModifiers::NONE,
        }
    }
}

// ---------------------------------------------------------------------------
// Binding + BindingGroup
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy)]
pub struct Binding {
    pub matcher: KeyMatcher,
    pub action: KeyAction,
    /// Display label for help/status bar (empty = hidden from help).
    pub label: &'static str,
    /// Description for help overlay (empty = hidden from help).
    pub description: &'static str,
}

impl Binding {
    pub fn matches(&self, key: &KeyEvent) -> bool {
        self.matcher.matches(key)
    }

    pub fn visible_in_help(&self) -> bool {
        !self.label.is_empty()
    }
}

#[derive(Debug, Clone, Copy)]
pub struct BindingGroup {
    pub title: &'static str,
    pub icon: &'static str,
    pub bindings: &'static [Binding],
}

// ---------------------------------------------------------------------------
// Binding constants
// ---------------------------------------------------------------------------

use KeyAction as A;
use KeyMatcher::{Char, Ctrl, Key};

pub static GLOBAL_BINDINGS: &[Binding] = &[
    Binding {
        matcher: Char('q'),
        action: A::Back,
        label: "q",
        description: "Back / Quit",
    },
    Binding {
        matcher: Ctrl('c'),
        action: A::Back,
        label: "",
        description: "",
    },
    Binding {
        matcher: Char('?'),
        action: A::ToggleHelp,
        label: "?",
        description: "Toggle help",
    },
    Binding {
        matcher: Key(KeyCode::Esc),
        action: A::Back,
        label: "Esc",
        description: "Go back / close",
    },
    Binding {
        matcher: Char('E'),
        action: A::ShowLastError,
        label: "E",
        description: "Show last error",
    },
    Binding {
        matcher: Char('t'),
        action: A::SwitchTeam,
        label: "t",
        description: "Switch team",
    },
];

pub static GLOBAL_NAV_BINDINGS: &[Binding] = &[
    Binding {
        matcher: Char('h'),
        action: A::NavigateTo(View::Dashboard),
        label: "h",
        description: "Dashboard (home)",
    },
    Binding {
        matcher: Char('i'),
        action: A::NavigateTo(View::IssueList),
        label: "i",
        description: "Go to issues",
    },
    Binding {
        matcher: Char('m'),
        action: A::NavigateTo(View::MrList),
        label: "m",
        description: "Go to merge requests",
    },
    Binding {
        matcher: Char('p'),
        action: A::NavigateTo(View::Planning),
        label: "p",
        description: "Go to planning",
    },
];

pub static LIST_NAV_BINDINGS: &[Binding] = &[
    Binding {
        matcher: Char('j'),
        action: A::MoveDown,
        label: "j/k",
        description: "Move down/up",
    },
    Binding {
        matcher: Key(KeyCode::Down),
        action: A::MoveDown,
        label: "",
        description: "",
    },
    Binding {
        matcher: Ctrl('n'),
        action: A::MoveDown,
        label: "",
        description: "",
    },
    Binding {
        matcher: Char('k'),
        action: A::MoveUp,
        label: "",
        description: "",
    },
    Binding {
        matcher: Key(KeyCode::Up),
        action: A::MoveUp,
        label: "",
        description: "",
    },
    Binding {
        matcher: Ctrl('p'),
        action: A::MoveUp,
        label: "",
        description: "",
    },
    Binding {
        matcher: Char('g'),
        action: A::Top,
        label: "g/G",
        description: "Jump to top/bottom",
    },
    Binding {
        matcher: Char('G'),
        action: A::Bottom,
        label: "",
        description: "",
    },
    Binding {
        matcher: Ctrl('d'),
        action: A::PageDown,
        label: "Ctrl+d/u",
        description: "Page down/up",
    },
    Binding {
        matcher: Ctrl('u'),
        action: A::PageUp,
        label: "",
        description: "",
    },
    Binding {
        matcher: Key(KeyCode::Enter),
        action: A::OpenDetail,
        label: "Enter",
        description: "Open detail",
    },
    Binding {
        matcher: Char('/'),
        action: A::StartSearch,
        label: "/",
        description: "Fuzzy search",
    },
    Binding {
        matcher: Char('r'),
        action: A::Refresh,
        label: "r",
        description: "Refresh data",
    },
    Binding {
        matcher: Char('o'),
        action: A::OpenBrowser,
        label: "o",
        description: "Open in browser",
    },
];

pub static DETAIL_NAV_BINDINGS: &[Binding] = &[
    Binding {
        matcher: Char('j'),
        action: A::MoveDown,
        label: "j/k",
        description: "Scroll down/up",
    },
    Binding {
        matcher: Key(KeyCode::Down),
        action: A::MoveDown,
        label: "",
        description: "",
    },
    Binding {
        matcher: Ctrl('n'),
        action: A::MoveDown,
        label: "",
        description: "",
    },
    Binding {
        matcher: Char('k'),
        action: A::MoveUp,
        label: "",
        description: "",
    },
    Binding {
        matcher: Key(KeyCode::Up),
        action: A::MoveUp,
        label: "",
        description: "",
    },
    Binding {
        matcher: Ctrl('p'),
        action: A::MoveUp,
        label: "",
        description: "",
    },
    Binding {
        matcher: Char('o'),
        action: A::OpenBrowser,
        label: "o",
        description: "Open in browser",
    },
    Binding {
        matcher: Char('r'),
        action: A::ReplyThread,
        label: "r",
        description: "Reply to thread",
    },
];

pub static FILTER_BINDINGS: &[Binding] = &[
    Binding {
        matcher: Char('f'),
        action: A::AddFilter,
        label: "f",
        description: "Add filter condition",
    },
    Binding {
        matcher: Char('F'),
        action: A::ClearFilters,
        label: "0/F",
        description: "Clear all filters",
    },
    Binding {
        matcher: Char('0'),
        action: A::ClearFilters,
        label: "",
        description: "",
    },
    Binding {
        matcher: Char('1'),
        action: A::ApplyPreset(1),
        label: "1-9",
        description: "Apply filter preset",
    },
    Binding {
        matcher: Char('2'),
        action: A::ApplyPreset(2),
        label: "",
        description: "",
    },
    Binding {
        matcher: Char('3'),
        action: A::ApplyPreset(3),
        label: "",
        description: "",
    },
    Binding {
        matcher: Char('4'),
        action: A::ApplyPreset(4),
        label: "",
        description: "",
    },
    Binding {
        matcher: Char('5'),
        action: A::ApplyPreset(5),
        label: "",
        description: "",
    },
    Binding {
        matcher: Char('6'),
        action: A::ApplyPreset(6),
        label: "",
        description: "",
    },
    Binding {
        matcher: Char('7'),
        action: A::ApplyPreset(7),
        label: "",
        description: "",
    },
    Binding {
        matcher: Char('8'),
        action: A::ApplyPreset(8),
        label: "",
        description: "",
    },
    Binding {
        matcher: Char('9'),
        action: A::ApplyPreset(9),
        label: "",
        description: "",
    },
    Binding {
        matcher: Char('e'),
        action: A::PickPreset,
        label: "e",
        description: "Pick saved preset",
    },
    Binding {
        matcher: Char('S'),
        action: A::PickSortPreset,
        label: "S",
        description: "Pick sort preset",
    },
    Binding {
        matcher: Key(KeyCode::Tab),
        action: A::FocusFilterBar,
        label: "Tab",
        description: "Focus filter bar",
    },
];

pub static ISSUE_ACTION_BINDINGS: &[Binding] = &[
    Binding {
        matcher: Char('s'),
        action: A::SetStatus,
        label: "s",
        description: "Set status",
    },
    Binding {
        matcher: Char('x'),
        action: A::ToggleState,
        label: "x",
        description: "Close / Reopen",
    },
    Binding {
        matcher: Char('l'),
        action: A::EditLabels,
        label: "l",
        description: "Set labels",
    },
    Binding {
        matcher: Char('a'),
        action: A::EditAssignee,
        label: "a",
        description: "Set assignee",
    },
    Binding {
        matcher: Char('c'),
        action: A::Comment,
        label: "c",
        description: "Add comment",
    },
];

pub static MR_ACTION_BINDINGS: &[Binding] = &[
    Binding {
        matcher: Char('A'),
        action: A::Approve,
        label: "A",
        description: "Approve MR",
    },
    Binding {
        matcher: Char('M'),
        action: A::Merge,
        label: "M",
        description: "Merge MR",
    },
    Binding {
        matcher: Char('x'),
        action: A::ToggleState,
        label: "x",
        description: "Close MR",
    },
    Binding {
        matcher: Char('l'),
        action: A::EditLabels,
        label: "l",
        description: "Set labels",
    },
    Binding {
        matcher: Char('a'),
        action: A::EditAssignee,
        label: "a",
        description: "Set assignee",
    },
    Binding {
        matcher: Char('c'),
        action: A::Comment,
        label: "c",
        description: "Add comment",
    },
];

pub static BOARD_NAV_BINDINGS: &[Binding] = &[
    Binding {
        matcher: Char('['),
        action: A::ColumnLeft,
        label: "[/]",
        description: "Switch column",
    },
    Binding {
        matcher: Key(KeyCode::Left),
        action: A::ColumnLeft,
        label: "",
        description: "",
    },
    Binding {
        matcher: Char(']'),
        action: A::ColumnRight,
        label: "",
        description: "",
    },
    Binding {
        matcher: Key(KeyCode::Right),
        action: A::ColumnRight,
        label: "",
        description: "",
    },
];

pub static PLANNING_NAV_BINDINGS: &[Binding] = &[
    Binding {
        matcher: Char('['),
        action: A::ColumnLeft,
        label: "[/]",
        description: "Switch column",
    },
    Binding {
        matcher: Char(']'),
        action: A::ColumnRight,
        label: "",
        description: "",
    },
    Binding {
        matcher: Char('<'),
        action: A::ToggleColumnPrev,
        label: "</>",
        description: "Toggle prev/next column",
    },
    Binding {
        matcher: Char('>'),
        action: A::ToggleColumnNext,
        label: "",
        description: "",
    },
    Binding {
        matcher: Char('v'),
        action: A::ToggleLayout,
        label: "v",
        description: "Toggle 3-col / 2-col",
    },
    Binding {
        matcher: Char('I'),
        action: A::MoveIteration,
        label: "I",
        description: "Move to iteration",
    },
    Binding {
        matcher: Char('H'),
        action: A::NavigateTo(View::Dashboard),
        label: "H",
        description: "Dashboard (home)",
    },
];

// ---------------------------------------------------------------------------
// Binding groups — each view composes from these
// ---------------------------------------------------------------------------

use crate::ui::styles;

static GLOBAL_GROUP: BindingGroup = BindingGroup {
    title: "Global",
    icon: styles::ICON_SECTION,
    bindings: GLOBAL_BINDINGS,
};

static GLOBAL_NAV_GROUP: BindingGroup = BindingGroup {
    title: "Navigation",
    icon: styles::ICON_SECTION,
    bindings: GLOBAL_NAV_BINDINGS,
};

static LIST_NAV_GROUP: BindingGroup = BindingGroup {
    title: "List Navigation",
    icon: styles::ICON_SECTION,
    bindings: LIST_NAV_BINDINGS,
};

static DETAIL_NAV_GROUP: BindingGroup = BindingGroup {
    title: "Detail",
    icon: styles::ICON_SECTION,
    bindings: DETAIL_NAV_BINDINGS,
};

static FILTER_GROUP: BindingGroup = BindingGroup {
    title: "Filtering",
    icon: styles::ICON_SECTION,
    bindings: FILTER_BINDINGS,
};

static ISSUE_ACTION_GROUP: BindingGroup = BindingGroup {
    title: "Issue Actions",
    icon: styles::ICON_SECTION,
    bindings: ISSUE_ACTION_BINDINGS,
};

static MR_ACTION_GROUP: BindingGroup = BindingGroup {
    title: "MR Actions",
    icon: styles::ICON_SECTION,
    bindings: MR_ACTION_BINDINGS,
};

static BOARD_NAV_GROUP: BindingGroup = BindingGroup {
    title: "Board Navigation",
    icon: styles::ICON_SECTION,
    bindings: BOARD_NAV_BINDINGS,
};

static PLANNING_NAV_GROUP: BindingGroup = BindingGroup {
    title: "Planning Navigation",
    icon: styles::ICON_SECTION,
    bindings: PLANNING_NAV_BINDINGS,
};

// ---------------------------------------------------------------------------
// Composer: binding groups per view
// ---------------------------------------------------------------------------

/// Returns the binding groups applicable to the given view.
/// Order matters: first match wins for dispatch; groups render top-to-bottom in help.
pub fn binding_groups_for_view(view: View) -> Vec<&'static BindingGroup> {
    match view {
        View::Dashboard => vec![
            &GLOBAL_GROUP,
            &GLOBAL_NAV_GROUP,
            &BOARD_NAV_GROUP,
            &LIST_NAV_GROUP,
            &ISSUE_ACTION_GROUP,
        ],
        View::IssueList => vec![
            &GLOBAL_GROUP,
            &GLOBAL_NAV_GROUP,
            &LIST_NAV_GROUP,
            &ISSUE_ACTION_GROUP,
            &FILTER_GROUP,
        ],
        View::IssueDetail => vec![
            &GLOBAL_GROUP,
            &GLOBAL_NAV_GROUP,
            &DETAIL_NAV_GROUP,
            &ISSUE_ACTION_GROUP,
        ],
        View::MrList => vec![
            &GLOBAL_GROUP,
            &GLOBAL_NAV_GROUP,
            &LIST_NAV_GROUP,
            &MR_ACTION_GROUP,
            &FILTER_GROUP,
        ],
        View::MrDetail => vec![
            &GLOBAL_GROUP,
            &GLOBAL_NAV_GROUP,
            &DETAIL_NAV_GROUP,
            &MR_ACTION_GROUP,
        ],
        View::Planning => vec![
            &GLOBAL_GROUP,
            // Note: no GLOBAL_NAV_GROUP here — Planning has its own nav
            // that overrides h and adds H
            &PLANNING_NAV_GROUP,
            &LIST_NAV_GROUP,
            &ISSUE_ACTION_GROUP,
        ],
    }
}

/// Find the first matching binding across all groups for a view.
pub fn match_binding(view: View, key: &KeyEvent) -> Option<KeyAction> {
    let groups = binding_groups_for_view(view);
    for group in groups {
        for binding in group.bindings {
            if binding.matches(key) {
                return Some(binding.action);
            }
        }
    }
    None
}
