use std::collections::HashSet;

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeyAction {
    Back,
    ToggleHelp,
    ShowLastError,
    SwitchTeam,
    SwitchTheme,
    NavigateTo(crate::app::View),

    MoveUp,
    MoveDown,
    Top,
    Bottom,
    PageUp,
    PageDown,
    OpenDetail,

    StartSearch,
    FocusFilterBar,
    FilterMenu,
    ClearFilters,
    SortByField,

    Refresh,
    FullRefresh,
    OpenBrowser,
    SetStatus,
    ToggleState,
    EditLabels,
    EditAssignee,
    Comment,

    Approve,
    Merge,

    ReplyThread,
    NewThread,
    EditComment,
    ResolveThread,
    ToggleThread,
    NextUnresolved,
    PrevUnresolved,

    AddLink,
    RemoveLink,
    OpenLink,

    ColumnLeft,
    ColumnRight,
    ToggleDashboardFocus,

    ToggleColumnPrev,
    ToggleColumnNext,
    ToggleLayout,
    MoveIteration,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum KeyMatcher {
    Char(char),
    Ctrl(char),
    /// ALT, which emacs calls Meta.
    Alt(char),
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
            Self::Alt(c) => {
                key.code == KeyCode::Char(c) && key.modifiers.contains(KeyModifiers::ALT)
            }
            Self::Key(code) => key.code == code && key.modifiers == KeyModifiers::NONE,
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct Binding {
    pub matcher: KeyMatcher,
    pub action: KeyAction,
    /// Empty hides the binding from help and the status bar.
    pub label: &'static str,
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

/// A row is `(<key>) => <Action>`, optionally followed by `| "<label>"
/// "<description>"`.  A row without that tail is a hidden alias: it still
/// claims the key so nothing later can bind it.  Keys are written `'c'`,
/// `ctrl 'c'`, or `key Enter` for a named [`KeyCode`].
///
/// The first row whose key matches wins, within a group and between groups.
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

#[macro_export]
macro_rules! binding_key {
    ($c:literal) => {
        $crate::keybindings::KeyMatcher::Char($c)
    };
    (ctrl $c:literal) => {
        $crate::keybindings::KeyMatcher::Ctrl($c)
    };
    (alt $c:literal) => {
        $crate::keybindings::KeyMatcher::Alt($c)
    };
    (key $code:ident) => {
        $crate::keybindings::KeyMatcher::Key(::crossterm::event::KeyCode::$code)
    };
}

/// Scans `chain` innermost first, so a group nearer the focus shadows an outer
/// one binding the same key.
pub fn resolve(chain: &[&'static BindingGroup], key: &KeyEvent) -> Option<KeyAction> {
    chain
        .iter()
        .find_map(|group| match_group(group.bindings, key))
}

/// Drops any binding whose key an earlier group claimed, hidden aliases
/// included, so help and the status bar never advertise a key [`resolve`] sends
/// somewhere else.
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

pub fn match_group(bindings: &[Binding], key: &KeyEvent) -> Option<KeyAction> {
    bindings.iter().find(|b| b.matches(key)).map(|b| b.action)
}
