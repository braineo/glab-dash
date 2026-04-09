use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::Modifier;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph, Row, Table};

use crate::config::Config;
use crate::gitlab::types::{TrackedIssue, TrackedMergeRequest};
use crate::ui::styles;

pub fn render(
    frame: &mut Frame,
    area: Rect,
    config: &Config,
    active_team: usize,
    issues: &[TrackedIssue],
    mrs: &[TrackedMergeRequest],
    loading: bool,
) {
    let chunks = Layout::vertical([
        Constraint::Length(3), // Header
        Constraint::Min(1),    // Content
    ])
    .split(area);

    // Header
    let team_name = config
        .teams
        .get(active_team)
        .map(|t| t.name.as_str())
        .unwrap_or("all");
    let header_text = format!(
        " glab-dash  |  Team: {}  |  Tracking: {} ",
        team_name, config.tracking_project
    );
    let header = Paragraph::new(Line::from(Span::styled(header_text, styles::title_style())))
        .block(
            Block::default()
                .borders(Borders::BOTTOM)
                .border_style(styles::title_style()),
        );
    frame.render_widget(header, chunks[0]);

    let content_chunks =
        Layout::horizontal([Constraint::Percentage(50), Constraint::Percentage(50)])
            .split(chunks[1]);

    // Left: Member summary
    render_member_summary(frame, content_chunks[0], config, active_team, issues, mrs);

    // Right: Quick stats
    render_quick_stats(frame, content_chunks[1], config, issues, mrs, loading);
}

fn render_member_summary(
    frame: &mut Frame,
    area: Rect,
    config: &Config,
    active_team: usize,
    issues: &[TrackedIssue],
    mrs: &[TrackedMergeRequest],
) {
    let members = config.team_members(active_team);
    let rows: Vec<Row> = members
        .iter()
        .map(|member| {
            let issue_count = issues
                .iter()
                .filter(|i| {
                    i.issue
                        .assignees
                        .iter()
                        .any(|a| a.username.eq_ignore_ascii_case(member))
                })
                .count();
            let mr_count = mrs
                .iter()
                .filter(|m| {
                    m.mr.assignees
                        .iter()
                        .any(|a| a.username.eq_ignore_ascii_case(member))
                        || m.mr
                            .reviewers
                            .iter()
                            .any(|r| r.username.eq_ignore_ascii_case(member))
                })
                .count();
            Row::new(vec![
                member.clone(),
                issue_count.to_string(),
                mr_count.to_string(),
            ])
        })
        .collect();

    let widths = [
        Constraint::Min(20),
        Constraint::Length(10),
        Constraint::Length(10),
    ];
    let table = Table::new(rows, widths)
        .header(
            Row::new(vec!["Member", "Issues", "MRs"])
                .style(styles::header_style())
                .bottom_margin(1),
        )
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title(" Team Members ")
                .title_style(styles::title_style()),
        );

    frame.render_widget(table, area);
}

fn render_quick_stats(
    frame: &mut Frame,
    area: Rect,
    config: &Config,
    issues: &[TrackedIssue],
    mrs: &[TrackedMergeRequest],
    loading: bool,
) {
    let tracking_issues = issues
        .iter()
        .filter(|i| matches!(i.source, crate::gitlab::types::ItemSource::Tracking))
        .count();
    let external_issues = issues
        .iter()
        .filter(|i| matches!(i.source, crate::gitlab::types::ItemSource::External(_)))
        .count();
    let unassigned_issues = issues
        .iter()
        .filter(|i| i.issue.assignees.is_empty())
        .count();
    let open_mrs = mrs.iter().filter(|m| m.mr.state == "opened").count();
    let draft_mrs = mrs
        .iter()
        .filter(|m| m.mr.draft || m.mr.work_in_progress)
        .count();
    let my_review_mrs = mrs
        .iter()
        .filter(|m| {
            m.mr.reviewers
                .iter()
                .any(|r| r.username.eq_ignore_ascii_case(&config.me))
        })
        .count();

    let loading_indicator = if loading { " (loading...)" } else { "" };

    let lines = vec![
        Line::from(""),
        Line::from(vec![
            Span::styled(
                "  Issues",
                styles::title_style().add_modifier(Modifier::UNDERLINED),
            ),
            Span::raw(loading_indicator),
        ]),
        Line::from(format!("    Tracking repo:   {tracking_issues}")),
        Line::from(format!("    External:        {external_issues}")),
        Line::from(Span::styled(
            format!("    Unassigned:      {unassigned_issues}"),
            if unassigned_issues > 0 {
                styles::error_style()
            } else {
                ratatui::style::Style::default()
            },
        )),
        Line::from(""),
        Line::from(Span::styled(
            "  Merge Requests",
            styles::title_style().add_modifier(Modifier::UNDERLINED),
        )),
        Line::from(format!("    Open:            {open_mrs}")),
        Line::from(format!("    Draft:           {draft_mrs}")),
        Line::from(format!("    Needs my review: {my_review_mrs}")),
        Line::from(""),
        Line::from(""),
        Line::from(vec![
            Span::raw("  Press "),
            Span::styled("i", styles::help_key_style()),
            Span::raw(" for issues, "),
            Span::styled("m", styles::help_key_style()),
            Span::raw(" for MRs, "),
            Span::styled("?", styles::help_key_style()),
            Span::raw(" for help"),
        ]),
    ];

    let block = Block::default()
        .borders(Borders::ALL)
        .title(" Overview ")
        .title_style(styles::title_style());

    let paragraph = Paragraph::new(lines).block(block);
    frame.render_widget(paragraph, area);
}
