/// Rich terminal diff renderer — shows file changes with colored +/- lines,
/// collapsible hidden sections, file stats, and interactive review prompts.

use similar::{ChangeTag, TextDiff};
use std::io::{self, BufRead, Write};

const RESET: &str = "\x1b[0m";
const BOLD: &str = "\x1b[1m";
const DIM: &str = "\x1b[2m";
const GREEN: &str = "\x1b[32m";
const RED: &str = "\x1b[31m";
const YELLOW: &str = "\x1b[33m";
const CYAN: &str = "\x1b[36m";
const WHITE: &str = "\x1b[97m";
const GRAY: &str = "\x1b[90m";
const BG_GREEN: &str = "\x1b[48;5;22m";
const BG_RED: &str = "\x1b[48;5;52m";
const BG_DARK: &str = "\x1b[48;5;236m";
const BG_BLUE: &str = "\x1b[48;5;17m";
const V_LINE: &str = "│";

const CONTEXT_LINES: usize = 3;
const MAX_PREVIEW_LINES: usize = 40;

#[derive(Debug, Clone)]
pub struct DiffLine {
    pub tag: LineTag,
    pub content: String,
    pub old_lineno: Option<usize>,
    pub new_lineno: Option<usize>,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum LineTag {
    Equal,
    Insert,
    Delete,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ReviewDecision {
    AcceptAll,
    RejectAll,
    PerLine,
}

pub struct DiffResult {
    pub lines: Vec<DiffLine>,
    pub additions: usize,
    pub deletions: usize,
    pub is_new_file: bool,
}

/// Compute a structured diff between old and new content.
pub fn compute_diff(old: &str, new: &str) -> DiffResult {
    let is_new = old.is_empty();
    let diff = TextDiff::from_lines(old, new);

    let mut lines = Vec::new();
    let mut additions = 0usize;
    let mut deletions = 0usize;
    let mut old_lineno = 1usize;
    let mut new_lineno = 1usize;

    for change in diff.iter_all_changes() {
        let content = change.to_string();
        match change.tag() {
            ChangeTag::Equal => {
                lines.push(DiffLine {
                    tag: LineTag::Equal,
                    content,
                    old_lineno: Some(old_lineno),
                    new_lineno: Some(new_lineno),
                });
                old_lineno += 1;
                new_lineno += 1;
            }
            ChangeTag::Insert => {
                additions += 1;
                lines.push(DiffLine {
                    tag: LineTag::Insert,
                    content,
                    old_lineno: None,
                    new_lineno: Some(new_lineno),
                });
                new_lineno += 1;
            }
            ChangeTag::Delete => {
                deletions += 1;
                lines.push(DiffLine {
                    tag: LineTag::Delete,
                    content,
                    old_lineno: Some(old_lineno),
                    new_lineno: None,
                });
                old_lineno += 1;
            }
        }
    }

    DiffResult {
        lines,
        additions,
        deletions,
        is_new_file: is_new,
    }
}

/// Render a file header with change stats (like Cursor's display).
fn render_file_header(filename: &str, diff: &DiffResult) {
    let name = filename
        .rsplit('/')
        .next()
        .unwrap_or(filename);

    if diff.is_new_file {
        eprintln!();
        eprintln!(
            "  {BG_BLUE}{WHITE}{BOLD} ◆ {name} {RESET} {GREEN}(new){RESET} {GREEN}+{}{RESET}",
            diff.additions
        );
    } else {
        let stats = match (diff.additions, diff.deletions) {
            (0, 0) => format!("{DIM}(no changes){RESET}"),
            (a, 0) => format!("{GREEN}+{a}{RESET}"),
            (0, d) => format!("{RED}-{d}{RESET}"),
            (a, d) => format!("{GREEN}+{a}{RESET} {RED}-{d}{RESET}"),
        };
        eprintln!();
        eprintln!("  {BG_BLUE}{WHITE}{BOLD} ◆ {name} {RESET} {stats}");
    }

    eprintln!(
        "  {GRAY}{}{RESET}",
        "─".repeat(58)
    );
}

/// Render a new file creation preview — shows all lines in green.
pub fn render_new_file(filename: &str, content: &str) {
    let lines: Vec<&str> = content.lines().collect();
    let total = lines.len();

    render_file_header(
        filename,
        &DiffResult {
            lines: Vec::new(),
            additions: total,
            deletions: 0,
            is_new_file: true,
        },
    );

    let show = total.min(MAX_PREVIEW_LINES);
    for (i, line) in lines.iter().take(show).enumerate() {
        let lineno = i + 1;
        let content = line.trim_end();
        eprintln!(
            "  {GRAY}{V_LINE}{RESET} {DIM}{lineno:>4}{RESET} {BG_GREEN}{GREEN}+{RESET} {GREEN}{content}{RESET}"
        );
    }

    if total > show {
        let hidden = total - show;
        eprintln!(
            "  {GRAY}{V_LINE}{RESET}        {DIM}  ... {hidden} more lines ...{RESET}"
        );
    }

    eprintln!(
        "  {GRAY}{}{RESET}",
        "─".repeat(58)
    );
}

/// Render a diff for an existing file — shows context, insertions, deletions
/// with collapsible hidden sections.
pub fn render_diff(filename: &str, diff: &DiffResult) {
    if diff.additions == 0 && diff.deletions == 0 {
        return;
    }

    render_file_header(filename, diff);

    // Build groups: collapse runs of Equal lines that are far from changes
    let mut i = 0;
    let total = diff.lines.len();

    while i < total {
        let line = &diff.lines[i];

        match line.tag {
            LineTag::Insert => {
                render_insert_line(line);
                i += 1;
            }
            LineTag::Delete => {
                render_delete_line(line);
                i += 1;
            }
            LineTag::Equal => {
                // Determine how many consecutive equal lines
                let start = i;
                while i < total && diff.lines[i].tag == LineTag::Equal {
                    i += 1;
                }
                let end = i;
                let span = end - start;

                // Check proximity to changes
                let near_before = start > 0 && diff.lines[start - 1].tag != LineTag::Equal;
                let near_after = end < total && diff.lines[end].tag != LineTag::Equal;

                if span <= CONTEXT_LINES * 2 + 1 {
                    // Small gap — show all lines
                    for line in &diff.lines[start..end] {
                        render_context_line(line);
                    }
                } else {
                    // Show context before, collapse middle, show context after
                    let show_before = if near_before { CONTEXT_LINES } else { 0 };
                    let show_after = if near_after { CONTEXT_LINES } else { 0 };

                    for line in diff.lines[start..start + show_before].iter() {
                        render_context_line(line);
                    }

                    let hidden = span - show_before - show_after;
                    if hidden > 0 {
                        eprintln!(
                            "  {GRAY}{V_LINE}{RESET}        {DIM}  ... {hidden} unchanged lines ...{RESET}"
                        );
                    }

                    for line in diff.lines[end - show_after..end].iter() {
                        render_context_line(line);
                    }
                }
            }
        }
    }

    eprintln!(
        "  {GRAY}{}{RESET}",
        "─".repeat(58)
    );
}

fn render_insert_line(line: &DiffLine) {
    let lineno = line.new_lineno.unwrap_or(0);
    let content = line.content.trim_end();
    eprintln!(
        "  {GRAY}{V_LINE}{RESET} {DIM}{lineno:>4}{RESET} {BG_GREEN}{GREEN}+{RESET} {GREEN}{content}{RESET}"
    );
}

fn render_delete_line(line: &DiffLine) {
    let lineno = line.old_lineno.unwrap_or(0);
    let content = line.content.trim_end();
    eprintln!(
        "  {GRAY}{V_LINE}{RESET} {DIM}{lineno:>4}{RESET} {BG_RED}{RED}-{RESET} {RED}{content}{RESET}"
    );
}

fn render_context_line(line: &DiffLine) {
    let lineno = line.new_lineno.unwrap_or(line.old_lineno.unwrap_or(0));
    let content = line.content.trim_end();
    eprintln!(
        "  {GRAY}{V_LINE}{RESET} {DIM}{lineno:>4}{RESET} {BG_DARK} {RESET} {DIM}{content}{RESET}"
    );
}

/// Prompt the user to accept, reject, or review line-by-line.
pub fn prompt_review(filename: &str) -> ReviewDecision {
    let short = filename.rsplit('/').next().unwrap_or(filename);
    eprint!(
        "\n  {YELLOW}{BOLD}?{RESET} {WHITE}{short}{RESET}: \
         {GREEN}[a]{RESET}ccept  {RED}[r]{RESET}eject  {CYAN}[l]{RESET}ine-by-line  > "
    );
    let _ = io::stderr().flush();

    let mut input = String::new();
    let _ = io::stdin().lock().read_line(&mut input);
    let choice = input.trim().to_lowercase();

    match choice.as_str() {
        "a" | "accept" | "y" | "yes" | "" => ReviewDecision::AcceptAll,
        "r" | "reject" | "n" | "no" => ReviewDecision::RejectAll,
        "l" | "line" | "line-by-line" => ReviewDecision::PerLine,
        _ => ReviewDecision::AcceptAll,
    }
}

/// Interactive per-line review. Returns a Vec<bool> — true for each accepted change.
pub fn review_per_line(diff: &DiffResult) -> Vec<bool> {
    let changes: Vec<&DiffLine> = diff
        .lines
        .iter()
        .filter(|l| l.tag != LineTag::Equal)
        .collect();

    let total = changes.len();
    let mut decisions = Vec::new();

    for (idx, line) in changes.iter().enumerate() {
        let content = line.content.trim_end();
        let prefix = match line.tag {
            LineTag::Insert => format!("{GREEN}+{RESET}"),
            LineTag::Delete => format!("{RED}-{RESET}"),
            LineTag::Equal => unreachable!(),
        };
        let lineno = line
            .new_lineno
            .or(line.old_lineno)
            .unwrap_or(0);

        eprintln!(
            "\n  {DIM}[{}/{}]{RESET}  {DIM}L{lineno}{RESET}  {prefix} {content}",
            idx + 1,
            total
        );
        eprint!("  {YELLOW}Accept?{RESET} {GREEN}[y]{RESET}/{RED}[n]{RESET}/{CYAN}[s]{RESET}kip rest > ");
        let _ = io::stderr().flush();

        let mut input = String::new();
        let _ = io::stdin().lock().read_line(&mut input);
        let choice = input.trim().to_lowercase();

        match choice.as_str() {
            "n" | "no" => decisions.push(false),
            "s" | "skip" => {
                decisions.push(true);
                // Accept remaining
                for _ in (idx + 1)..total {
                    decisions.push(true);
                }
                break;
            }
            _ => decisions.push(true),
        }
    }

    // Pad with true if we broke early
    while decisions.len() < total {
        decisions.push(true);
    }

    decisions
}

/// Apply per-line decisions to produce the final file content.
/// `decisions` maps 1:1 to the non-Equal lines in the diff.
pub fn apply_decisions(old: &str, diff: &DiffResult, decisions: &[bool]) -> String {
    let mut result = String::new();
    let mut decision_idx = 0;

    for line in &diff.lines {
        match line.tag {
            LineTag::Equal => {
                result.push_str(&line.content);
            }
            LineTag::Insert => {
                let accept = decisions.get(decision_idx).copied().unwrap_or(true);
                decision_idx += 1;
                if accept {
                    result.push_str(&line.content);
                }
            }
            LineTag::Delete => {
                let accept = decisions.get(decision_idx).copied().unwrap_or(true);
                decision_idx += 1;
                if !accept {
                    // Rejection of a delete = keep the old line
                    result.push_str(&line.content);
                }
            }
        }
    }

    // If old content was empty and all inserts accepted, just return new content
    if old.is_empty() && result.is_empty() {
        return result;
    }

    result
}

/// Render a summary line after review decisions.
pub fn render_review_summary(accepted: usize, rejected: usize) {
    eprintln!();
    if rejected == 0 {
        eprintln!(
            "  {GREEN}{BOLD}✓{RESET} {GREEN}All {accepted} changes accepted{RESET}"
        );
    } else if accepted == 0 {
        eprintln!(
            "  {RED}{BOLD}✗{RESET} {RED}All {rejected} changes rejected — file not modified{RESET}"
        );
    } else {
        eprintln!(
            "  {YELLOW}{BOLD}◆{RESET} {GREEN}{accepted} accepted{RESET}, {RED}{rejected} rejected{RESET}"
        );
    }
}
