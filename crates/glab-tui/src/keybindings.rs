use std::collections::HashSet;

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

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
    NavigateTo(crate::app::View),

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
    FilterMenu,
    ClearFilters,
    SortByField,

    // --- Shared item actions (resolved via FocusedItem) ---
    Refresh,
    FullRefresh,
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
    /// Reply into the thread the cursor is on.
    ReplyThread,
    /// Open a new top-level thread.
    NewThread,
    /// Resolve or reopen the thread the cursor is on.
    ResolveThread,
    /// Fold the thread the cursor is on away, or open it back up.
    ToggleThread,
    /// Jump to the next thread still needing an answer.
    NextUnresolved,
    /// Jump back to the previous one.
    PrevUnresolved,

    // --- Board / column navigation (Dashboard & Planning) ---
    ColumnLeft,
    ColumnRight,
    /// Toggle focus between health panel and iteration board on dashboard.
    ToggleDashboardFocus,

    // --- Planning-specific ---
    ToggleColumnPrev,
    ToggleColumnNext,
    ToggleLayout,
    MoveIteration,
}

// ---------------------------------------------------------------------------
// KeyMatcher — how a binding matches key events
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
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
            Self::Char(c) => {
                key.code == KeyCode::Char(c)
                    && !key.modifiers.contains(KeyModifiers::CONTROL)
                    && !key.modifiers.contains(KeyModifiers::ALT)
            }
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
    pub bindings: &'static [Binding],
}

// ---------------------------------------------------------------------------
// Declaring groups
// ---------------------------------------------------------------------------

/// Declare a [`BindingGroup`], one line per binding.
///
/// A row is `(<key>) => <Action>`, optionally followed by `| "<label>"
/// "<description>"`.  A row without that tail is a hidden alias: it still
/// claims the key (so nothing later can bind it) but stays out of the help
/// overlay and the status bar.  Keys are written `'c'`, `ctrl 'c'`, or
/// `key Enter` for a named [`KeyCode`]; an action carrying a payload is
/// written with it, `NavigateTo(View::Planning)`.
///
/// Order matters inside a group and between groups: the first row whose key
/// matches wins, so put the more specific binding first.
#[macro_export]
macro_rules! binding_group {
    (
        $(#[$attr:meta])*
        $vis:vis $name:ident: $title:literal {
            $( ( $($key:tt)+ ) => $action:ident $(($($arg:expr),*))?
                 $(| $label:literal $desc:literal)? ),* $(,)?
        }
    ) => {
        $(#[$attr])*
        $vis static $name: $crate::keybindings::BindingGroup =
            $crate::keybindings::BindingGroup {
                title: $title,
                bindings: &[
                    $($crate::keybindings::Binding {
                        matcher: $crate::binding_key!($($key)+),
                        action: $crate::keybindings::KeyAction::$action $(($($arg),*))?,
                        label: $crate::binding_group!(@or_blank $($label)?),
                        description: $crate::binding_group!(@or_blank $($desc)?),
                    }),*
                ],
            };
    };
    (@or_blank) => { "" };
    (@or_blank $text:literal) => { $text };
}

/// The [`KeyMatcher`] for one `binding_group!` key spec.
#[macro_export]
macro_rules! binding_key {
    ($c:literal) => {
        $crate::keybindings::KeyMatcher::Char($c)
    };
    (ctrl $c:literal) => {
        $crate::keybindings::KeyMatcher::Ctrl($c)
    };
    (key $code:ident) => {
        $crate::keybindings::KeyMatcher::Key(::crossterm::event::KeyCode::$code)
    };
}

// ---------------------------------------------------------------------------
// Resolving
// ---------------------------------------------------------------------------

/// Resolve a key to the one action it fires, scanning `chain` in order.
///
/// The chain runs innermost first, so a group nearer the focus shadows an
/// outer one binding the same key — a detail view's `r` (reply) wins over the
/// global `r` (refresh) with no special case anywhere.
pub fn resolve(chain: &[&'static BindingGroup], key: &KeyEvent) -> Option<KeyAction> {
    chain
        .iter()
        .find_map(|group| match_group(group.bindings, key))
}

/// The bindings in `chain` that can actually fire, grouped, with any binding
/// whose key an earlier group already claimed dropped.  Hidden aliases claim
/// their key too, so a labelled binding shadowed by an unlabelled one goes as
/// well.
///
/// Help and the status bar render from this rather than from the raw groups,
/// so neither can advertise a key [`resolve`] sends somewhere else.
pub fn active_bindings(
    chain: &[&'static BindingGroup],
) -> Vec<(&'static BindingGroup, Vec<&'static Binding>)> {
    let mut claimed = HashSet::new();
    chain
        .iter()
        .map(|group| {
            let live = group
                .bindings
                .iter()
                .filter(|b| claimed.insert(b.matcher))
                .collect();
            (*group, live)
        })
        .collect()
}

/// Find the first matching action in a single binding group.
pub fn match_group(bindings: &[Binding], key: &KeyEvent) -> Option<KeyAction> {
    bindings.iter().find(|b| b.matches(key)).map(|b| b.action)
}
