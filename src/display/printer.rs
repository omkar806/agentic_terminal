use serde_json::Value;

// ANSI
const RESET: &str = "\x1b[0m";
const BOLD: &str = "\x1b[1m";
const DIM: &str = "\x1b[2m";
const ITALIC: &str = "\x1b[3m";
const GREEN: &str = "\x1b[32m";
const YELLOW: &str = "\x1b[33m";
const BLUE: &str = "\x1b[34m";
const MAGENTA: &str = "\x1b[35m";
const CYAN: &str = "\x1b[36m";
const RED: &str = "\x1b[31m";
const WHITE: &str = "\x1b[97m";
const GRAY: &str = "\x1b[90m";
const BG_DARK: &str = "\x1b[48;5;236m";
const BG_GREEN: &str = "\x1b[42m";
const BG_RED: &str = "\x1b[41m";
const BG_YELLOW: &str = "\x1b[43m";
const BG_BLUE: &str = "\x1b[44m";
const BG_MAGENTA: &str = "\x1b[45m";
const BG_CYAN: &str = "\x1b[46m";
const BLACK: &str = "\x1b[30m";

// Box drawing
const H_LINE: &str = "─";
const TOP_LEFT: &str = "┌";
const TOP_RIGHT: &str = "┐";
const BOT_LEFT: &str = "└";
const BOT_RIGHT: &str = "┘";
const V_LINE: &str = "│";
const T_RIGHT: &str = "├";
const T_LEFT: &str = "┤";

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Verbosity {
    Quiet,
    Normal,
    Verbose,
}

#[derive(Debug, Clone)]
pub struct Printer {
    pub verbosity: Verbosity,
}

fn hr(width: usize) -> String {
    H_LINE.repeat(width)
}

fn boxed_header(label: &str, color: &str, width: usize) -> String {
    let inner = width.saturating_sub(4);
    let padded = format!(" {} ", label);
    let pad_right = inner.saturating_sub(padded.len());
    format!(
        "{color}{TOP_LEFT}{hline}{TOP_RIGHT}{RESET}\n\
         {color}{V_LINE}{RESET}{BOLD}{WHITE}{padded}{RESET}{}{color}{V_LINE}{RESET}\n\
         {color}{BOT_LEFT}{hline}{BOT_RIGHT}{RESET}",
        " ".repeat(pad_right),
        hline = hr(inner + 2),
    )
}

impl Printer {
    pub fn new(verbosity: Verbosity) -> Self {
        Self { verbosity }
    }

    pub fn quiet() -> Self {
        Self { verbosity: Verbosity::Quiet }
    }

    fn is_quiet(&self) -> bool {
        self.verbosity == Verbosity::Quiet
    }

    fn is_verbose(&self) -> bool {
        self.verbosity == Verbosity::Verbose
    }

    pub fn turn_start(&self, agent_name: &str, turn: usize, max_turns: usize) {
        if self.is_quiet() { return; }
        eprintln!();
        eprintln!(
            "  {GRAY}{T_RIGHT}{hline}{T_LEFT}{RESET}",
            hline = hr(60)
        );
        eprintln!(
            "  {GRAY}{V_LINE}{RESET} {BG_CYAN}{BLACK}{BOLD} {agent_name} {RESET}  {DIM}turn {turn}/{max_turns}{RESET}  {ITALIC}{DIM}thinking...{RESET}"
        );
        eprintln!(
            "  {GRAY}{T_RIGHT}{hline}{T_LEFT}{RESET}",
            hline = hr(60)
        );
    }

    pub fn tool_call(&self, _agent_name: &str, tool_name: &str, args: &Value) {
        if self.is_quiet() { return; }

        if tool_name == "run_command" {
            if let Some(cmd) = args.get("command").and_then(|v| v.as_str()) {
                let sudo = args.get("use_sudo").and_then(|v| v.as_bool()).unwrap_or(false);
                eprintln!();
                if sudo {
                    eprintln!("  {BG_RED}{BLACK}{BOLD} SUDO {RESET} {RED}{BOLD}#{RESET} {BOLD}{WHITE}{cmd}{RESET}");
                } else {
                    eprintln!("  {BG_DARK}{GREEN}{BOLD} $ {RESET} {BOLD}{WHITE}{cmd}{RESET}");
                }
                eprintln!("  {GRAY}{}", hr(60));
                return;
            }
        }

        if tool_name == "read_file" {
            if let Some(path) = args.get("path").and_then(|v| v.as_str()) {
                eprintln!();
                eprintln!("  {BG_BLUE}{BLACK}{BOLD} READ {RESET} {BLUE}{path}{RESET}");
                eprintln!("  {GRAY}{}{RESET}", hr(60));
                return;
            }
        }

        if tool_name == "write_file" {
            if let Some(path) = args.get("path").and_then(|v| v.as_str()) {
                let size = args.get("content").and_then(|v| v.as_str()).map(|s| s.len()).unwrap_or(0);
                eprintln!();
                eprintln!("  {BG_YELLOW}{BLACK}{BOLD} WRITE {RESET} {YELLOW}{path}{RESET} {DIM}({size} bytes){RESET}");
                eprintln!("  {GRAY}{}{RESET}", hr(60));
                return;
            }
        }

        if tool_name == "list_directory" {
            if let Some(path) = args.get("path").and_then(|v| v.as_str()) {
                eprintln!();
                eprintln!("  {BG_BLUE}{BLACK}{BOLD} LS {RESET} {BLUE}{path}{RESET}");
                eprintln!("  {GRAY}{}{RESET}", hr(60));
                return;
            }
        }

        if tool_name == "run_script" {
            let lang = args.get("language").and_then(|v| v.as_str()).unwrap_or("script");
            eprintln!();
            eprintln!("  {BG_MAGENTA}{BLACK}{BOLD} SCRIPT {RESET} {MAGENTA}{lang}{RESET}");
            eprintln!("  {GRAY}{}{RESET}", hr(60));
            if self.is_verbose() {
                if let Some(content) = args.get("content").and_then(|v| v.as_str()) {
                    for line in content.lines().take(10) {
                        eprintln!("  {GRAY}{V_LINE}  {line}{RESET}");
                    }
                    let total = content.lines().count();
                    if total > 10 {
                        eprintln!("  {GRAY}{V_LINE}  ... ({} more lines){RESET}", total - 10);
                    }
                }
            }
            return;
        }

        if tool_name == "check_process" {
            if let Some(name) = args.get("name").and_then(|v| v.as_str()) {
                eprintln!();
                eprintln!("  {BG_CYAN}{BLACK}{BOLD} PROC {RESET} checking {BOLD}{name}{RESET}");
                eprintln!("  {GRAY}{}{RESET}", hr(60));
                return;
            }
        }

        if tool_name == "kill_process" {
            let target = args.get("target").and_then(|v| v.as_str()).unwrap_or("?");
            let signal = args.get("signal").and_then(|v| v.as_str()).unwrap_or("SIGTERM");
            eprintln!();
            eprintln!("  {BG_RED}{BLACK}{BOLD} KILL {RESET} {RED}{target}{RESET} {DIM}({signal}){RESET}");
            eprintln!("  {GRAY}{}{RESET}", hr(60));
            return;
        }

        // Generic
        let args_str = serde_json::to_string(args).unwrap_or_default();
        let truncated = if args_str.len() > 100 {
            format!("{}...", &args_str[..100])
        } else {
            args_str
        };
        eprintln!();
        eprintln!("  {BG_MAGENTA}{BLACK}{BOLD} {tool_name} {RESET} {GRAY}{truncated}{RESET}");
        eprintln!("  {GRAY}{}{RESET}", hr(60));
    }

    pub fn tool_result(&self, tool_name: &str, result: &Value) {
        if self.is_quiet() { return; }

        if tool_name == "run_command" || tool_name == "run_script" {
            self.print_shell_result(result);
            return;
        }

        if tool_name == "read_file" {
            if let Some(err) = result.get("error").and_then(|v| v.as_str()) {
                eprintln!("  {GRAY}{V_LINE}{RESET}  {RED}{err}{RESET}");
            } else if let Some(content) = result.get("content").and_then(|v| v.as_str()) {
                let size = result.get("size_bytes").and_then(|v| v.as_u64()).unwrap_or(0);
                let line_count = content.lines().count();
                eprintln!("  {GRAY}{V_LINE}{RESET}  {DIM}{line_count} lines, {size} bytes{RESET}");
                if self.is_verbose() {
                    for line in content.lines().take(8) {
                        eprintln!("  {GRAY}{V_LINE}{RESET}  {GRAY}{line}{RESET}");
                    }
                    if line_count > 8 {
                        eprintln!("  {GRAY}{V_LINE}{RESET}  {DIM}... ({} more lines){RESET}", line_count - 8);
                    }
                }
            }
            self.print_result_footer(true);
            return;
        }

        if tool_name == "write_file" {
            if let Some(err) = result.get("error").and_then(|v| v.as_str()) {
                eprintln!("  {GRAY}{V_LINE}{RESET}  {RED}{err}{RESET}");
                self.print_result_footer(false);
            } else {
                let bytes = result.get("bytes_written").and_then(|v| v.as_u64()).unwrap_or(0);
                eprintln!("  {GRAY}{V_LINE}{RESET}  {GREEN}Written{RESET} {DIM}{bytes} bytes{RESET}");
                self.print_result_footer(true);
            }
            return;
        }

        if tool_name == "list_directory" {
            if let Some(err) = result.get("error").and_then(|v| v.as_str()) {
                eprintln!("  {GRAY}{V_LINE}{RESET}  {RED}{err}{RESET}");
                self.print_result_footer(false);
            } else if let Some(entries) = result.get("entries").and_then(|v| v.as_array()) {
                let count = entries.len();
                for entry in entries.iter().take(20) {
                    let name = entry.get("name").and_then(|v| v.as_str()).unwrap_or("?");
                    let ftype = entry.get("type").and_then(|v| v.as_str()).unwrap_or("?");
                    let (icon, color) = match ftype {
                        "directory" => ("d", BLUE),
                        "symlink" => ("l", CYAN),
                        _ => ("-", WHITE),
                    };
                    eprintln!("  {GRAY}{V_LINE}{RESET}  {color}{icon}{RESET} {name}");
                }
                if count > 20 {
                    eprintln!("  {GRAY}{V_LINE}{RESET}  {DIM}... ({} more){RESET}", count - 20);
                }
                eprintln!("  {GRAY}{V_LINE}{RESET}  {DIM}{count} entries total{RESET}");
                self.print_result_footer(true);
            }
            return;
        }

        if tool_name == "check_process" {
            let running = result.get("running").and_then(|v| v.as_bool()).unwrap_or(false);
            let count = result.get("count").and_then(|v| v.as_u64()).unwrap_or(0);
            if running {
                eprintln!("  {GRAY}{V_LINE}{RESET}  {BG_GREEN}{BLACK}{BOLD} RUNNING {RESET} {DIM}{count} process(es) matched{RESET}");
                if self.is_verbose() {
                    if let Some(matches) = result.get("matches").and_then(|v| v.as_array()) {
                        for m in matches.iter().take(5) {
                            if let Some(line) = m.as_str() {
                                eprintln!("  {GRAY}{V_LINE}{RESET}  {GRAY}{line}{RESET}");
                            }
                        }
                    }
                }
            } else {
                eprintln!("  {GRAY}{V_LINE}{RESET}  {YELLOW}NOT RUNNING{RESET}");
            }
            self.print_result_footer(true);
            return;
        }

        // Generic error display
        if let Some(err) = result.get("error").and_then(|v| v.as_str()) {
            eprintln!("  {GRAY}{V_LINE}{RESET}  {RED}{err}{RESET}");
            self.print_result_footer(false);
        } else {
            self.print_result_footer(true);
        }
    }

    fn print_shell_result(&self, result: &Value) {
        if let Some(err) = result.get("error").and_then(|v| v.as_str()) {
            eprintln!("  {GRAY}{V_LINE}{RESET}  {RED}{err}{RESET}");
            self.print_result_footer(false);
            return;
        }

        if result.get("dry_run").and_then(|v| v.as_bool()).unwrap_or(false) {
            eprintln!("  {GRAY}{V_LINE}{RESET}  {BG_YELLOW}{BLACK} DRY RUN {RESET} {YELLOW}command not executed{RESET}");
            self.print_result_footer(true);
            return;
        }

        let exit_code = result.get("exit_code").and_then(|v| v.as_i64()).unwrap_or(-1);
        let stdout = result.get("stdout").and_then(|v| v.as_str()).unwrap_or("");
        let stderr = result.get("stderr").and_then(|v| v.as_str()).unwrap_or("");

        if !stdout.is_empty() {
            let lines: Vec<&str> = stdout.lines().collect();
            let show_limit = if self.is_verbose() { 50 } else { 25 };
            for line in lines.iter().take(show_limit) {
                eprintln!("  {GRAY}{V_LINE}{RESET}  {line}");
            }
            if lines.len() > show_limit {
                eprintln!(
                    "  {GRAY}{V_LINE}{RESET}  {DIM}... {more} more line(s){RESET}",
                    more = lines.len() - show_limit
                );
            }
        }

        if !stderr.is_empty() {
            if !stdout.is_empty() {
                eprintln!("  {GRAY}{V_LINE}{RESET}");
            }
            eprintln!("  {GRAY}{V_LINE}{RESET}  {RED}{BOLD}stderr:{RESET}");
            let lines: Vec<&str> = stderr.lines().collect();
            for line in lines.iter().take(15) {
                eprintln!("  {GRAY}{V_LINE}{RESET}  {RED}{line}{RESET}");
            }
            if lines.len() > 15 {
                eprintln!(
                    "  {GRAY}{V_LINE}{RESET}  {DIM}... {more} more stderr line(s){RESET}",
                    more = lines.len() - 15
                );
            }
        }

        if exit_code == 0 {
            eprintln!("  {GRAY}{BOT_LEFT}{}{RESET} {BG_GREEN}{BLACK}{BOLD} exit 0 {RESET}", hr(3));
        } else {
            eprintln!("  {GRAY}{BOT_LEFT}{}{RESET} {BG_RED}{WHITE}{BOLD} exit {exit_code} {RESET}", hr(3));
        }
    }

    fn print_result_footer(&self, success: bool) {
        if success {
            eprintln!("  {GRAY}{BOT_LEFT}{}{RESET} {GREEN}OK{RESET}", hr(3));
        } else {
            eprintln!("  {GRAY}{BOT_LEFT}{}{RESET} {RED}FAILED{RESET}", hr(3));
        }
    }

    pub fn handoff(&self, from_agent: &str, to_agent: &str) {
        if self.is_quiet() { return; }
        eprintln!();
        eprintln!(
            "  {BG_YELLOW}{BLACK}{BOLD} HANDOFF {RESET}  {DIM}{from_agent}{RESET} {YELLOW}=>{RESET} {BOLD}{CYAN}{to_agent}{RESET}"
        );
    }

    pub fn final_output(&self, agent_name: &str) {
        if self.is_quiet() { return; }
        eprintln!(
            "  {GRAY}{}{RESET}", hr(60)
        );
        eprintln!(
            "  {BG_GREEN}{BLACK}{BOLD} DONE {RESET}  {DIM}{agent_name}{RESET}"
        );
    }

    pub fn usage(&self, total_tokens: u32) {
        if !self.is_verbose() { return; }
        eprintln!("  {DIM}tokens used: {total_tokens}{RESET}");
    }

    pub fn banner(&self, version: &str, model: &str, session_id: &str, workdir: &str, sudo: bool) {
        let w = 60;
        eprintln!("{CYAN}{}{RESET}", hr(w + 4));
        eprintln!("{CYAN}{V_LINE}{RESET}  {BOLD}{WHITE}Agentic Terminal{RESET} {DIM}v{version}{RESET}{}{CYAN}{V_LINE}{RESET}",
            " ".repeat(w - 22 - version.len())
        );
        eprintln!("{CYAN}{}{RESET}", hr(w + 4));
        eprintln!("  {DIM}model:{RESET}    {BOLD}{model}{RESET}");
        eprintln!("  {DIM}session:{RESET}  {session_id}");
        eprintln!("  {DIM}workdir:{RESET}  {workdir}");
        let sudo_str = if sudo {
            format!("{GREEN}enabled{RESET}")
        } else {
            format!("{DIM}disabled{RESET}")
        };
        eprintln!("  {DIM}sudo:{RESET}     {sudo_str}");
        eprintln!();
        eprintln!("  {DIM}Type 'exit' or 'quit' to end the session.{RESET}");
        eprintln!("{GRAY}{}{RESET}", hr(w + 4));
        eprintln!();
    }

    pub fn memory_event(&self, action: &str, detail: &str) {
        if self.is_quiet() { return; }
        eprintln!();
        eprintln!(
            "  {BG_MAGENTA}{BLACK}{BOLD} MEMORY {RESET}  {MAGENTA}{action}{RESET}  {DIM}{detail}{RESET}"
        );
    }

    pub fn memory_stats(&self, total: i64, unsummarized: i64, summaries: i64, next_in: i64) {
        if self.is_quiet() { return; }
        eprintln!(
            "  {DIM}memory: {total} msgs total, {unsummarized} recent, {summaries} summaries, next compress in {next_in}{RESET}"
        );
    }
}
