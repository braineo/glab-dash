use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Borders, Paragraph, Row, Table};

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
    let header_text = Line::from(vec![
        Span::styled(" ◈ glab-dash", styles::title_style()),
        Span::styled(styles::ICON_SEPARATOR, styles::help_desc_style()),
        Span::styled(format!("Team: {team_name}"), Style::default().fg(styles::TEAL)),
        Span::styled(styles::ICON_SEPARATOR, styles::help_desc_style()),
        Span::styled(
            format!("Tracking: {}", config.tracking_project),
            styles::help_desc_style(),
        ),
    ]);
    let header = Paragraph::new(header_text).block(
        Block::default()
            .borders(Borders::BOTTOM)
            .border_type(BorderType::Rounded)
            .border_style(Style::default().fg(styles::BORDER)),
    );
    frame.render_widget(header, chunks[0]);

    let content_chunks =
        Layout::horizontal([Constraint::Percentage(50), Constraint::Percentage(50)])
            .split(chunks[1]);

    render_member_summary(frame, content_chunks[0], config, active_team, issues, mrs);
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
        .enumerate()
        .map(|(i, member)| {
            let issue_count = issues
                .iter()
                .filter(|issue| {
                    issue
                        .issue
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
            let row = Row::new(vec![
                member.clone(),
                issue_count.to_string(),
                mr_count.to_string(),
            ]);
            if i % 2 == 1 {
                row.style(styles::row_alt_style())
            } else {
                row
            }
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
        .block(styles::block("Team Members"));

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
        .filter(|i| i.project_path == config.tracking_project)
        .count();
    let external_issues = issues
        .iter()
        .filter(|i| i.project_path != config.tracking_project)
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

    let loading_indicator = if loading { " ⟳" } else { "" };

    let lines = vec![
        Line::from(""),
        Line::from(vec![
            Span::styled(
                format!("  ● Issues{loading_indicator}"),
                styles::section_header_style(),
            ),
        ]),
        Line::from(vec![
            Span::styled("    Tracking repo:   ", styles::help_desc_style()),
            Span::styled(
                tracking_issues.to_string(),
                Style::default()
                    .fg(styles::TEXT_BRIGHT)
                    .add_modifier(Modifier::BOLD),
            ),
        ]),
        Line::from(vec![
            Span::styled("    External:        ", styles::help_desc_style()),
            Span::styled(
                external_issues.to_string(),
                Style::default()
                    .fg(styles::TEXT_BRIGHT)
                    .add_modifier(Modifier::BOLD),
            ),
        ]),
        Line::from(if unassigned_issues > 0 {
            vec![
                Span::styled("    Unassigned:      ", styles::help_desc_style()),
                Span::styled(
                    unassigned_issues.to_string(),
                    styles::error_style(),
                ),
            ]
        } else {
            vec![
                Span::styled("    Unassigned:      ", styles::help_desc_style()),
                Span::styled("0", Style::default().fg(styles::TEXT_BRIGHT)),
            ]
        }),
        Line::from(""),
        Line::from(Span::styled(
            "  ⑂ Merge Requests",
            styles::section_header_style(),
        )),
        Line::from(vec![
            Span::styled("    Open:            ", styles::help_desc_style()),
            Span::styled(
                open_mrs.to_string(),
                Style::default()
                    .fg(styles::TEXT_BRIGHT)
                    .add_modifier(Modifier::BOLD),
            ),
        ]),
        Line::from(vec![
            Span::styled("    Draft:           ", styles::help_desc_style()),
            Span::styled(
                draft_mrs.to_string(),
                Style::default()
                    .fg(styles::TEXT_BRIGHT)
                    .add_modifier(Modifier::BOLD),
            ),
        ]),
        Line::from(vec![
            Span::styled("    Needs my review: ", styles::help_desc_style()),
            Span::styled(
                my_review_mrs.to_string(),
                Style::default()
                    .fg(styles::TEXT_BRIGHT)
                    .add_modifier(Modifier::BOLD),
            ),
        ]),
        Line::from(""),
        Line::from(""),
        Line::from(vec![
            Span::raw("  Press "),
            Span::styled("i", styles::help_key_style()),
            Span::styled(" issues ", styles::help_desc_style()),
            Span::styled("m", styles::help_key_style()),
            Span::styled(" mrs ", styles::help_desc_style()),
            Span::styled("?", styles::help_key_style()),
            Span::styled(" help", styles::help_desc_style()),
        ]),
    ];

    let paragraph = Paragraph::new(lines).block(styles::block("Overview"));
    frame.render_widget(paragraph, area);
}
