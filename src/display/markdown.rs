/// Terminal markdown renderer — converts markdown syntax to ANSI escape codes.
/// Designed for line-by-line streaming: call `render_line` for each complete line.

const RESET: &str = "\x1b[0m";
const BOLD: &str = "\x1b[1m";
const DIM: &str = "\x1b[2m";
const ITALIC: &str = "\x1b[3m";
const UNDERLINE: &str = "\x1b[4m";
const CYAN: &str = "\x1b[36m";
const YELLOW: &str = "\x1b[33m";
const BLUE: &str = "\x1b[34m";
const MAGENTA: &str = "\x1b[35m";
const WHITE: &str = "\x1b[97m";
const GRAY: &str = "\x1b[90m";
const BG_CODE: &str = "\x1b[48;5;236m";

fn h_repeat(ch: char, n: usize) -> String {
    std::iter::repeat(ch).take(n).collect()
}

/// Render a single line of markdown to ANSI-colored terminal output.
/// `in_code_block` tracks multi-line fenced code block state across calls.
pub fn render_line(line: &str, in_code_block: &mut bool) -> String {
    let trimmed = line.trim_start();

    // Fenced code block toggle
    if trimmed.starts_with("```") {
        *in_code_block = !*in_code_block;
        if *in_code_block {
            let lang = trimmed.strip_prefix("```").unwrap_or("").trim();
            let rule = h_repeat('─', 40);
            if lang.is_empty() {
                return format!("  {DIM}┌{rule}{RESET}");
            } else {
                let pad = h_repeat('─', 35usize.saturating_sub(lang.len()));
                return format!("  {DIM}┌── {CYAN}{lang}{DIM} {pad}{RESET}");
            }
        } else {
            let rule = h_repeat('─', 40);
            return format!("  {DIM}└{rule}{RESET}");
        }
    }

    // Inside code block — render with background, no inline parsing
    if *in_code_block {
        return format!("  {DIM}│{RESET} {BG_CODE}{WHITE} {line} {RESET}");
    }

    // Horizontal rule
    if (trimmed == "---" || trimmed == "***" || trimmed == "___") && trimmed.len() >= 3 {
        let rule = h_repeat('─', 50);
        return format!("  {GRAY}{rule}{RESET}");
    }

    // Headers
    if trimmed.starts_with("#### ") {
        let content = render_inline(&trimmed[5..]);
        return format!("    {BOLD}{MAGENTA}{content}{RESET}");
    }
    if trimmed.starts_with("### ") {
        let content = render_inline(&trimmed[4..]);
        return format!("   {BOLD}{BLUE}{content}{RESET}");
    }
    if trimmed.starts_with("## ") {
        let content = render_inline(&trimmed[3..]);
        return format!("  {BOLD}{CYAN}{content}{RESET}");
    }
    if trimmed.starts_with("# ") {
        let content = render_inline(&trimmed[2..]);
        return format!("\n  {BOLD}{WHITE}{UNDERLINE}{content}{RESET}\n");
    }

    // Blockquote
    if trimmed.starts_with("> ") {
        let content = render_inline(&trimmed[2..]);
        return format!("  {GRAY}│{RESET} {ITALIC}{content}{RESET}");
    }

    // Unordered list (preserve indentation)
    if trimmed.starts_with("- ") || trimmed.starts_with("* ") {
        let indent = line.len() - trimmed.len();
        let pad = " ".repeat(indent + 2);
        let content = render_inline(&trimmed[2..]);
        return format!("{pad}{CYAN}•{RESET} {content}");
    }

    // Nested unordered list
    if trimmed.starts_with("  - ") || trimmed.starts_with("  * ") {
        let indent = line.len() - trimmed.len();
        let pad = " ".repeat(indent + 4);
        let content = render_inline(&trimmed[4..]);
        return format!("{pad}{DIM}◦{RESET} {content}");
    }

    // Numbered list
    if let Some((num_str, rest)) = try_numbered_list(trimmed) {
        let indent = line.len() - trimmed.len();
        let pad = " ".repeat(indent + 2);
        let content = render_inline(rest);
        return format!("{pad}{YELLOW}{num_str}{RESET} {content}");
    }

    // Empty line
    if trimmed.is_empty() {
        return String::new();
    }

    // Regular text with inline formatting
    format!("  {}", render_inline(trimmed))
}

fn try_numbered_list(s: &str) -> Option<(&str, &str)> {
    let dot_pos = s.find('.')?;
    if dot_pos > 4 {
        return None;
    }
    let num_part = &s[..dot_pos];
    if num_part.chars().all(|c| c.is_ascii_digit()) {
        let after_dot = &s[dot_pos + 1..];
        let rest = after_dot.strip_prefix(' ').unwrap_or(after_dot);
        Some((&s[..=dot_pos], rest))
    } else {
        None
    }
}

/// Render inline markdown: **bold**, *italic*, `code`
fn render_inline(text: &str) -> String {
    let mut result = String::with_capacity(text.len() * 2);
    let chars: Vec<char> = text.chars().collect();
    let len = chars.len();
    let mut i = 0;

    while i < len {
        // Bold: **text**
        if i + 1 < len && chars[i] == '*' && chars[i + 1] == '*' {
            if let Some(end) = find_double_delim(&chars, i + 2, '*') {
                result.push_str(BOLD);
                result.push_str(WHITE);
                for j in (i + 2)..end {
                    result.push(chars[j]);
                }
                result.push_str(RESET);
                i = end + 2;
                continue;
            }
        }

        // Bold: __text__
        if i + 1 < len && chars[i] == '_' && chars[i + 1] == '_' {
            if let Some(end) = find_double_delim(&chars, i + 2, '_') {
                result.push_str(BOLD);
                result.push_str(WHITE);
                for j in (i + 2)..end {
                    result.push(chars[j]);
                }
                result.push_str(RESET);
                i = end + 2;
                continue;
            }
        }

        // Italic: *text* (not preceded by another *)
        if chars[i] == '*' && (i + 1 < len && chars[i + 1] != '*') {
            if let Some(end) = find_single_delim(&chars, i + 1, '*') {
                if end > i + 1 {
                    result.push_str(ITALIC);
                    for j in (i + 1)..end {
                        result.push(chars[j]);
                    }
                    result.push_str(RESET);
                    i = end + 1;
                    continue;
                }
            }
        }

        // Inline code: `text`
        if chars[i] == '`' {
            if let Some(end) = find_single_delim(&chars, i + 1, '`') {
                if end > i + 1 {
                    result.push_str(BG_CODE);
                    result.push_str(CYAN);
                    result.push(' ');
                    for j in (i + 1)..end {
                        result.push(chars[j]);
                    }
                    result.push(' ');
                    result.push_str(RESET);
                    i = end + 1;
                    continue;
                }
            }
        }

        result.push(chars[i]);
        i += 1;
    }

    result
}

fn find_double_delim(chars: &[char], start: usize, delim: char) -> Option<usize> {
    let mut i = start;
    while i + 1 < chars.len() {
        if chars[i] == delim && chars[i + 1] == delim {
            return Some(i);
        }
        i += 1;
    }
    None
}

fn find_single_delim(chars: &[char], start: usize, delim: char) -> Option<usize> {
    for i in start..chars.len() {
        if chars[i] == delim {
            return Some(i);
        }
    }
    None
}

/// Render a full markdown block (multi-line) into ANSI-formatted terminal output.
pub fn render_markdown(text: &str) -> String {
    let mut in_code_block = false;
    let mut lines: Vec<String> = Vec::new();
    for line in text.lines() {
        lines.push(render_line(line, &mut in_code_block));
    }
    lines.join("\n")
}
