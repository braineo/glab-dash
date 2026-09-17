//! What the open item is related to, as rows in a detail body, and the keys
//! that add and drop one.
//!
//! What a relation means, how it sorts and whether it can be dropped are the
//! domain's answers; this spends them on icons, colors and keys.

use glab_core::domain::Issue;
use glab_core::domain::{ItemKind, ItemRef, RelatedItem, Relation};
use ratatui::style::Style;
use ratatui::text::{Line, Span};

use crate::app::Overlay;
use crate::cmd::{Cmd, Effects, EventResult};
use crate::keybindings::KeyAction;
use crate::ui::components::detail_body::{self, DetailBody, Row};
use crate::ui::components::{chord_popup::ChordState, picker::PickerState};
use crate::ui::views::DetailCtx;
use crate::ui::{styles, wrap};

/// The icon for a relation that neither blocks nor is blocked.
const ICON_RELATED: &str = "\u{21C4}";
/// Columns the relation is padded to, so the references line up.
const RELATION_WIDTH: usize = 11;

/// Bubbles anything else, and anything aimed at a row holding no relation.
pub fn handle_key(
    action: KeyAction,
    cx: &DetailCtx<'_>,
    body: &DetailBody,
    overlay: &mut Overlay,
    fx: &mut Effects<'_>,
) -> EventResult {
    match action {
        KeyAction::AddLink => *overlay = pick_relation(cx.item.clone()),
        KeyAction::RemoveLink => {
            // Off the section, or a derived relation with no link to delete.
            let Some(link_id) = at_cursor(cx.related, body).and_then(|r| r.link_id) else {
                return EventResult::Bubble;
            };
            fx.cmds.push(Cmd::RemoveLink {
                item: cx.item.clone(),
                link_id,
            });
        }
        _ => return EventResult::Bubble,
    }
    EventResult::Consumed
}

/// `None` on every row that holds no relation.
pub fn at_cursor<'a>(related: &'a [RelatedItem], body: &DetailBody) -> Option<&'a RelatedItem> {
    let Row::Related(index) = body.cursor_row() else {
        return None;
    };
    related.get(index)
}

/// Nothing at all when there are none: no section rule either.
pub fn push(body: &mut DetailBody, related: &[RelatedItem], width: usize) {
    if related.is_empty() {
        return;
    }
    let count = related.len();
    let blocking = related.iter().filter(|r| r.is_blocker()).count();
    let tally = match blocking {
        0 => format!("{count} item{}", detail_body::plural(count)),
        n => format!(
            "{count} item{} \u{00B7} {n} blocking",
            detail_body::plural(count)
        ),
    };
    body.section("LINKED", Some(tally), width);

    // References run from `app!33` to a four-segment path, so the title column
    // is set by the widest one here rather than by a constant.  Capped, so one
    // long path cannot shove every title off the row.
    let refs: Vec<String> = related.iter().map(reference).collect();
    let ref_width = refs
        .iter()
        .map(|r| wrap::width(r))
        .max()
        .unwrap_or(0)
        .min(width / 3);

    let rows: Vec<(Row, Line<'static>)> = related
        .iter()
        .zip(refs)
        .enumerate()
        .map(|(i, (item, reference))| (Row::Related(i), row(item, &reference, ref_width, width)))
        .collect();
    body.extend(rows);
}

/// The kind icon the tab bar uses, then the full reference: the sigil alone,
/// buried at the end of a long project path, is not what the eye lands on.
fn reference(related: &RelatedItem) -> String {
    let kind = match related.item.kind {
        ItemKind::Issue => styles::ICON_ISSUES,
        ItemKind::MergeRequest => styles::ICON_MRS,
    };
    format!("{kind} {}", related.item.reference())
}

/// The relation carries the color — an open blocker is what the reader scans
/// for.  A settled one keeps its shape and loses its color.
fn row(related: &RelatedItem, reference: &str, ref_width: usize, width: usize) -> Line<'static> {
    let (icon, tint) = match related.relation {
        Relation::BlockedBy => (styles::ICON_BLOCKED, styles::red()),
        Relation::Blocks => (styles::ICON_BLOCKED, styles::yellow()),
        Relation::Closes | Relation::ClosedBy => (styles::ICON_CHECK, styles::green()),
        Relation::RelatesTo => (ICON_RELATED, styles::text_dim()),
    };
    let (tint, text) = if related.is_open() {
        (tint, styles::text())
    } else {
        (styles::text_dim(), styles::text_dim())
    };
    let relation = format!("{icon} {:<RELATION_WIDTH$}", related.relation.label());
    // Padded by display width: a reference is not all single-column glyphs.
    let pad = " ".repeat(ref_width.saturating_sub(wrap::width(reference)));
    let reference = format!("{reference}{pad}  ");
    let used = wrap::width(&relation) + wrap::width(&reference) + detail_body::LEAD;
    let title = wrap::truncate(&related.title, width.saturating_sub(used));

    let rail = Span::styled(detail_body::RAIL, Style::default().fg(tint));
    detail_body::indented(
        std::slice::from_ref(&rail),
        Line::from(vec![
            Span::styled(relation, Style::default().fg(tint)),
            Span::styled(reference, Style::default().fg(styles::cyan())),
            Span::styled(title, Style::default().fg(text)),
        ]),
    )
}

/// `L`, first half: which relation.
fn pick_relation(item: ItemRef) -> Overlay {
    let labels = Relation::LINKABLE.iter().map(|r| r.label().to_string());
    Overlay::Chord {
        state: ChordState::new_for_names("Link Type", labels.collect()),
        on_complete: Box::new(move |label, app| {
            let Some(relation) = Relation::LINKABLE.into_iter().find(|r| r.label() == label) else {
                return;
            };
            app.ui.overlay = pick_target(&item, relation, &app.data.issues);
        }),
    }
}

/// `L`, second half: which item.  The reference leads each row, so the pick
/// alone says which item it was.
///
/// ponytail: offers only what has been fetched; linking outside the team's
/// scope needs free text in the picker.
fn pick_target(item: &ItemRef, relation: Relation, issues: &[Issue]) -> Overlay {
    let self_reference = item.reference();
    let (items, subtitles): (Vec<String>, Vec<String>) = issues
        .iter()
        .filter(|i| i.reference != self_reference)
        .map(|i| {
            let state = i.status_name().unwrap_or(&i.state).to_string();
            let assignees: Vec<&str> = i.assignees.iter().map(|a| a.username.as_str()).collect();
            let who = if assignees.is_empty() {
                String::new()
            } else {
                format!("  @{}", assignees.join(" @"))
            };
            (format!("{}  {}", i.reference, i.title), state + &who)
        })
        .unzip();

    let item = item.clone();
    Overlay::Picker {
        state: PickerState::new(&format!("Link \u{2014} {}", relation.label()), items, false)
            .with_subtitles(subtitles),
        on_complete: Box::new(move |values, app| {
            let target = values
                .first()
                .and_then(|picked| picked.split_whitespace().next())
                .and_then(ItemRef::parse);
            let Some(target) = target else {
                return;
            };
            app.ui.pending_cmds.push(Cmd::AddLink {
                item,
                target,
                relation,
            });
        }),
    }
}

#[cfg(test)]
mod tests {
    use glab_core::domain::{ItemRef, RelatedItem, Relation};

    use super::{at_cursor, handle_key, push};
    use crate::app::Overlay;
    use crate::cmd::{Cmd, Dirty, Effects};
    use crate::keybindings::KeyAction;
    use crate::ui::components::detail_body::{DetailBody, Row};
    use crate::ui::views::DetailCtx;

    fn related(relation: Relation, link_id: Option<u64>, iid: &str) -> RelatedItem {
        RelatedItem {
            relation,
            link_id,
            item: ItemRef::issue("team/infra", iid),
            title: "a related issue".to_string(),
            state: "opened".to_string(),
            web_url: "https://gitlab.example.com/team/infra/-/issues/1".to_string(),
        }
    }

    fn body(related: &[RelatedItem]) -> DetailBody {
        let mut body = DetailBody::default();
        body.begin();
        body.description(Some("the description"), 60);
        push(&mut body, related, 60);
        body
    }

    #[test]
    fn every_related_row_answers_with_its_own_relation() {
        let items = vec![
            related(Relation::BlockedBy, Some(1), "11"),
            related(Relation::Blocks, Some(2), "12"),
            related(Relation::RelatesTo, Some(3), "13"),
        ];
        let mut body = body(&items);

        let rows = body.rows().iter().filter(|r| matches!(r, Row::Related(_)));
        assert_eq!(rows.count(), 3, "one row per relation: {:?}", body.text());

        for row in 0..body.rows().len() {
            let expected = match body.rows()[row] {
                Row::Related(i) => items[i].link_id,
                _ => None,
            };
            body.set_cursor(row);
            assert_eq!(
                at_cursor(&items, &body).and_then(|r| r.link_id),
                expected,
                "row {row}"
            );
        }
    }

    #[test]
    fn a_derived_relation_refuses_to_be_unlinked() {
        let items = vec![
            related(Relation::ClosedBy, None, "11"),
            related(Relation::RelatesTo, Some(7), "12"),
        ];
        let mut body = body(&items);
        let mut overlay = Overlay::None;
        let mut dirty = Dirty::default();
        let mut cmds = Vec::new();
        let mut redraw = false;

        let mut unlink = |body: &DetailBody, cmds: &mut Vec<Cmd>| {
            let cx = DetailCtx {
                item: ItemRef::issue("team/app", "1"),
                related: &items,
                issues: &[],
            };
            let mut fx = Effects {
                dirty: &mut dirty,
                cmds,
                needs_redraw: &mut redraw,
            };
            handle_key(KeyAction::RemoveLink, &cx, body, &mut overlay, &mut fx).handled()
        };

        let rows: Vec<usize> = (0..body.rows().len())
            .filter(|&r| matches!(body.rows()[r], Row::Related(_)))
            .collect();

        body.set_cursor(rows[0]);
        assert!(!unlink(&body, &mut cmds), "a derived relation has no link");
        assert!(cmds.is_empty(), "{cmds:?}");

        body.set_cursor(rows[1]);
        assert!(unlink(&body, &mut cmds), "a stored link can be dropped");
        assert!(
            matches!(cmds.as_slice(), [Cmd::RemoveLink { link_id: 7, .. }]),
            "{cmds:?}"
        );
    }
}
