//! What a focused issue and a focused merge request answer to identically,
//! written once against [`Item`].  Each kind's own module keeps the rest.

use glab_core::domain::{Item, ItemRef};

use crate::cmd::EventResult;
use crate::keybindings::KeyAction;
use crate::ui::components::{
    chord_popup, conversation::draft_new_thread, input::CommentTarget, label_editor, picker,
};

use super::{AppCtx, AppData, AsyncMsg, Overlay, UiState, View};

/// Bubbles anything only one kind answers to.
pub fn handle_key(
    action: KeyAction,
    item: &impl Item,
    ctx: &AppCtx,
    data: &AppData,
    ui: &mut UiState,
) -> EventResult {
    match action {
        KeyAction::OpenBrowser => {
            if let Some(url) = item.web_url() {
                let _ = open::that_detached(url);
            }
        }
        KeyAction::EditLabels => {
            let label_names: Vec<String> = data.labels.iter().map(|l| l.name.clone()).collect();
            let issue_labels: Vec<Vec<String>> =
                data.issues.iter().map(|i| i.labels.clone()).collect();
            ui.overlay = Overlay::LabelEditor {
                state: label_editor::LabelEditorState::new(
                    label_names,
                    item.labels(),
                    &data.label_usage,
                    &issue_labels,
                    20,
                ),
            };
        }
        KeyAction::EditAssignee => {
            let members = ctx.config.all_members();
            // A detail has room for a picker; over a list, the chord.
            ui.overlay = if matches!(ui.view, View::IssueDetail | View::MrDetail) {
                Overlay::Picker {
                    state: picker::PickerState::new("Assignee", members, false),
                    on_complete: Box::new(|values, app| {
                        if let Some(username) = values.first() {
                            app.dispatch_update_assignee(username);
                        }
                    }),
                }
            } else {
                Overlay::Chord {
                    state: chord_popup::ChordState::new_for_names("Set Assignee", members),
                    on_complete: Box::new(|value, app| {
                        app.dispatch_update_assignee(&value);
                    }),
                }
            };
        }
        KeyAction::Comment => ui.overlay = draft_new_thread(),
        _ => return EventResult::Bubble,
    }
    EventResult::Consumed
}

/// Post `body` as a new thread, a reply or a rewrite, then re-list the threads
/// so the view shows what the server kept.
pub fn submit_comment(
    item: &ItemRef,
    body: &str,
    target: CommentTarget,
    ctx: &AppCtx,
    ui: &mut UiState,
) {
    let client = ctx.client.clone();
    let tx = ctx.async_tx.clone();
    let body = body.to_string();
    let (kind, project, iid) = (item.kind, item.project.clone(), item.iid.clone());

    ui.loading = true;
    tokio::spawn(async move {
        let written = match &target {
            CommentTarget::Reply(discussion) => client
                .reply_to_discussion(kind, &project, &iid, discussion, &body)
                .await
                .map(|_| ()),
            CommentTarget::NewThread => client
                .create_thread(kind, &project, &iid, &body)
                .await
                .map(|_| ()),
            CommentTarget::Edit(note) => client
                .update_note(kind, &project, &iid, *note, &body)
                .await
                .map(|_| ()),
        };
        if let Err(e) = written {
            let _ = tx.send(AsyncMsg::ActionDone(Err(e)));
            return;
        }
        let discussions = client.list_discussions(kind, &project, &iid).await;
        let _ = tx.send(AsyncMsg::DiscussionsLoaded(discussions));
    });
}
