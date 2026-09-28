use glab_core::domain::{Issue, Item, MergeRequest};
use glab_core::domain::{ItemKind, RelatedItem, Relation};
use ratatui::style::Style;
use ratatui::text::{Line, Span};

use crate::app::Overlay;
use crate::cmd::{Cmd, Effects, EventResult};
use crate::keybindings::KeyAction;
use crate::ui::components::detail_body::{self, DetailBody, Row};
use crate::ui::components::{chord_popup::ChordState, picker::PickerState};
use crate::ui::views::DetailCtx;
use crate::ui::{styles, wrap};

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
        KeyAction::AddLink => *overlay = pick_relation(cx.kind, cx.gid.clone()),
        KeyAction::RemoveLink => {
            let Some(target) = at_cursor(cx.related, body).filter(|r| r.is_unlinkable(cx.kind))
            else {
                return EventResult::Bubble;
            };
            fx.cmds.push(Cmd::RemoveLink {
                gid: cx.gid.clone(),
                target_gid: target.gid.clone(),
            });
        }
        _ => return EventResult::Bubble,
    }
    EventResult::Consumed
}

pub fn at_cursor<'a>(related: &'a [RelatedItem], body: &DetailBody) -> Option<&'a RelatedItem> {
    let Row::Related(index) = body.cursor_row() else {
        return None;
    };
    related.get(index)
}

/// Pushes nothing at all when there are none, not even a section rule.
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

    // Capped, so one long path cannot shove every title off the row.
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

fn reference(related: &RelatedItem) -> String {
    let kind = match related.item.kind {
        ItemKind::Issue => styles::ICON_ISSUES,
        ItemKind::MergeRequest => styles::ICON_MRS,
    };
    format!("{kind} {}", related.item.reference())
}

/// A settled relation keeps its shape and loses its color.
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
    // By display width: a reference is not all single-column glyphs.
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

#[derive(Clone, Copy)]
enum Choice {
    Link(Relation),
    /// The only way GitLab relates an issue to a merge request.  Named from
    /// the merge request's side, since that is where the line lives.
    Mention(Relation),
}

/// Labelled from `kind`'s own side.
fn choices(kind: ItemKind) -> Vec<(&'static str, Choice)> {
    match kind {
        ItemKind::Issue => vec![
            ("blocked by", Choice::Link(Relation::BlockedBy)),
            ("blocks", Choice::Link(Relation::Blocks)),
            ("relates to", Choice::Link(Relation::RelatesTo)),
            ("closed by MR", Choice::Mention(Relation::Closes)),
            ("related MR", Choice::Mention(Relation::RelatesTo)),
        ],
        ItemKind::MergeRequest => vec![
            ("closes", Choice::Mention(Relation::Closes)),
            ("relates to", Choice::Mention(Relation::RelatesTo)),
        ],
    }
}

/// The relation also decides what is picked from.
fn pick_relation(kind: ItemKind, gid: String) -> Overlay {
    let choices = choices(kind);
    let labels: Vec<String> = choices
        .iter()
        .map(|(label, _)| (*label).to_string())
        .collect();
    Overlay::Chord {
        state: ChordState::new_for_names("Link Type", labels),
        on_complete: Box::new(move |picked, app| {
            let Some(&(label, choice)) = choices.iter().find(|(l, _)| *l == picked) else {
                return;
            };
            let gid = gid.clone();
            app.ui.overlay = match choice {
                Choice::Link(relation) => {
                    let rows = issue_rows(&app.data.issues, &gid);
                    pick_target(label, rows, move |picked| Cmd::AddLink {
                        gid: gid.clone(),
                        target_gid: picked,
                        relation,
                    })
                }
                // The merge request holds the line, so each side picks the
                // other kind.
                Choice::Mention(relation) => {
                    let rows = match kind {
                        ItemKind::Issue => mr_rows(&app.data.mrs, &gid),
                        ItemKind::MergeRequest => issue_rows(&app.data.issues, &gid),
                    };
                    pick_target(label, rows, move |picked| {
                        mention(kind, gid.clone(), picked, relation)
                    })
                }
            };
        }),
    }
}

/// `kind` and `view_gid` are the view that asked, `picked` the other side;
/// whichever is the merge request carries the line.
fn mention(kind: ItemKind, view_gid: String, picked: String, relation: Relation) -> Cmd {
    let (gid, target_gid) = if kind == ItemKind::MergeRequest {
        (view_gid, picked)
    } else {
        (picked, view_gid)
    };
    Cmd::MentionInMr {
        gid,
        target_gid,
        relation,
    }
}

/// Label and gid travel together, so a pick is never read back out of a
/// label.
struct TargetRow {
    label: String,
    subtitle: String,
    gid: String,
}

fn pick_target(
    label: &str,
    rows: Vec<TargetRow>,
    cmd: impl Fn(String) -> Cmd + 'static,
) -> Overlay {
    let (labels, subtitles): (Vec<String>, Vec<String>) = rows
        .iter()
        .map(|r| (r.label.clone(), r.subtitle.clone()))
        .unzip();
    Overlay::Picker {
        state: PickerState::new(&format!("Link \u{2014} {label}"), labels, false)
            .with_subtitles(subtitles),
        on_complete: Box::new(move |values, app| {
            let Some(row) = values
                .first()
                .and_then(|l| rows.iter().find(|r| &r.label == l))
            else {
                return;
            };
            app.ui.pending_cmds.push(cmd(row.gid.clone()));
        }),
    }
}

/// An issue's custom status beats its raw state.
fn issue_rows(issues: &[Issue], self_gid: &str) -> Vec<TargetRow> {
    rows(issues, self_gid, |i| {
        i.status_name().unwrap_or(&i.state).to_string()
    })
}

fn mr_rows(mrs: &[MergeRequest], self_gid: &str) -> Vec<TargetRow> {
    rows(mrs, self_gid, |m| m.state.clone())
}

/// Leaves out the item itself: nothing relates to itself.
///
/// ponytail: offers only what has been fetched; naming something outside the
/// team's scope needs free text in the picker.
fn rows<T: Item>(items: &[T], self_gid: &str, state: impl Fn(&T) -> String) -> Vec<TargetRow> {
    items
        .iter()
        .filter(|i| i.gid() != self_gid)
        .map(|i| {
            let assignees: Vec<&str> = i.assignees().iter().map(|a| a.username.as_str()).collect();
            let who = if assignees.is_empty() {
                String::new()
            } else {
                format!("  @{}", assignees.join(" @"))
            };
            TargetRow {
                label: format!("{}  {}", i.reference(), i.title()),
                subtitle: state(i) + &who,
                gid: i.gid().to_string(),
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use glab_core::domain::{ItemRef, RelatedItem, Relation};

    use super::{Choice, at_cursor, choices, handle_key, mention, push};
    use crate::app::Overlay;
    use crate::cmd::{Cmd, Dirty, Effects};
    use crate::keybindings::KeyAction;
    use crate::ui::components::detail_body::{DetailBody, Row};
    use crate::ui::views::DetailCtx;
    use glab_core::domain::ItemKind;

    fn related(relation: Relation, gid: &str, iid: &str) -> RelatedItem {
        RelatedItem {
            relation,
            gid: gid.to_string(),
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
            related(Relation::BlockedBy, "gid://gitlab/WorkItem/1", "11"),
            related(Relation::Blocks, "gid://gitlab/WorkItem/2", "12"),
            related(Relation::RelatesTo, "gid://gitlab/WorkItem/3", "13"),
        ];
        let mut body = body(&items);

        let rows = body.rows().iter().filter(|r| matches!(r, Row::Related(_)));
        assert_eq!(rows.count(), 3, "one row per relation: {:?}", body.text());

        for row in 0..body.rows().len() {
            let expected = match body.rows()[row] {
                Row::Related(i) => Some(items[i].gid.clone()),
                _ => None,
            };
            body.set_cursor(row);
            assert_eq!(
                at_cursor(&items, &body).map(|r| r.gid.clone()),
                expected,
                "row {row}"
            );
        }
    }

    #[test]
    fn a_mention_is_written_to_whichever_side_is_the_merge_request() {
        const MR: &str = "gid://gitlab/MergeRequest/7";
        const ISSUE: &str = "gid://gitlab/Issue/1";

        for (kind, view, picked) in [
            (ItemKind::Issue, ISSUE, MR),
            (ItemKind::MergeRequest, MR, ISSUE),
        ] {
            let Cmd::MentionInMr {
                gid,
                target_gid,
                relation,
            } = mention(kind, view.to_string(), picked.to_string(), Relation::Closes)
            else {
                panic!("a mention is a mention");
            };
            assert_eq!(gid, MR, "the merge request is the side written");
            assert_eq!(target_gid, ISSUE);
            assert_eq!(relation, Relation::Closes);
        }
    }

    #[test]
    fn a_merge_request_offers_mentions_alone() {
        assert!(
            choices(ItemKind::MergeRequest)
                .iter()
                .all(|(_, c)| matches!(c, Choice::Mention(_))),
        );
        assert!(
            choices(ItemKind::Issue)
                .iter()
                .any(|(_, c)| matches!(c, Choice::Link(_))),
        );
        for kind in [ItemKind::Issue, ItemKind::MergeRequest] {
            let labels: Vec<&str> = choices(kind).iter().map(|(l, _)| *l).collect();
            let mut unique = labels.clone();
            unique.sort_unstable();
            unique.dedup();
            assert_eq!(unique.len(), labels.len(), "the chord keys off the label");
        }
    }

    #[test]
    fn only_a_stored_link_between_two_issues_can_be_unlinked() {
        let mut mr = related(Relation::RelatesTo, "gid://gitlab/MergeRequest/9", "9");
        mr.item = ItemRef::merge_request("team/infra", "9");
        let items = vec![
            related(Relation::ClosedBy, "gid://gitlab/MergeRequest/11", "11"),
            mr,
            related(Relation::RelatesTo, "gid://gitlab/WorkItem/7", "12"),
        ];
        let mut body = body(&items);
        let mut overlay = Overlay::None;
        let mut dirty = Dirty::default();
        let mut redraw = false;

        let mut unlink = |kind: ItemKind, body: &DetailBody, cmds: &mut Vec<Cmd>| {
            let cx = DetailCtx {
                kind,
                gid: "gid://gitlab/WorkItem/1".to_string(),
                related: &items,
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
        let mut cmds = Vec::new();

        body.set_cursor(rows[0]);
        assert!(
            !unlink(ItemKind::Issue, &body, &mut cmds),
            "a derived relation has no link"
        );

        body.set_cursor(rows[1]);
        assert!(
            !unlink(ItemKind::Issue, &body, &mut cmds),
            "a merge request is not a work item the mutation can name"
        );

        body.set_cursor(rows[2]);
        assert!(
            !unlink(ItemKind::MergeRequest, &body, &mut cmds),
            "a merge request holds no stored link to drop"
        );
        assert!(cmds.is_empty(), "{cmds:?}");

        assert!(
            unlink(ItemKind::Issue, &body, &mut cmds),
            "a stored link between two issues can be dropped"
        );
        assert!(
            matches!(
                cmds.as_slice(),
                [Cmd::RemoveLink { gid, target_gid }]
                    if gid == "gid://gitlab/WorkItem/1"
                        && target_gid == "gid://gitlab/WorkItem/7"
            ),
            "{cmds:?}"
        );
    }
}
