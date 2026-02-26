/// Detects whether user input is a shell command or natural language.
///
/// Conventions:
///   `!command`  — force shell execution (strip the `!`)
///   `?question` — force LLM (strip the `?`)
///   Otherwise   — heuristic detection

/// What the user intended.
pub enum InputKind {
    /// Run directly as a shell command.
    ShellCommand(String),
    /// Pass to the LLM agent as natural language.
    AgentQuery(String),
    /// Built-in shell command that needs special handling (cd, export).
    Builtin(BuiltinCmd),
}

pub enum BuiltinCmd {
    Cd(String),
    Export(String, String),
    Clear,
}

// Common Unix/macOS commands — first token checked against this set.
const KNOWN_COMMANDS: &[&str] = &[
    // filesystem
    "ls", "ll", "la", "dir", "pwd", "cat", "head", "tail", "less", "more",
    "touch", "mkdir", "mkdirp", "rmdir", "rm", "cp", "mv", "ln",
    "find", "locate", "mdfind", "file", "stat", "readlink", "realpath",
    "chmod", "chown", "chgrp", "umask",
    // text
    "grep", "rg", "ag", "awk", "sed", "sort", "uniq", "wc", "cut", "tr",
    "diff", "patch", "fmt", "column", "jq", "yq", "xargs", "tee",
    // system
    "ps", "kill", "killall", "pkill", "top", "htop", "btop",
    "df", "du", "free", "uname", "whoami", "hostname", "id",
    "uptime", "date", "cal", "w", "who", "last", "dmesg", "lsof",
    "strace", "dtrace", "sysctl", "launchctl",
    // network
    "ping", "curl", "wget", "ssh", "scp", "sftp", "rsync",
    "netstat", "ss", "ifconfig", "ip", "dig", "nslookup", "host",
    "traceroute", "tracepath", "nc", "ncat", "telnet", "ftp",
    // packages
    "brew", "apt", "apt-get", "dpkg", "yum", "dnf", "rpm",
    "pip", "pip3", "pipx", "npm", "npx", "yarn", "pnpm", "bun",
    "cargo", "rustup", "gem", "go", "composer",
    // dev tools
    "git", "gh", "svn", "hg",
    "docker", "docker-compose", "podman", "kubectl", "helm",
    "make", "cmake", "ninja", "gcc", "g++", "clang", "clang++",
    "rustc", "python", "python3", "node", "deno", "ruby", "perl",
    "java", "javac", "mvn", "gradle", "scala", "kotlin",
    "php", "lua", "swift", "swiftc", "go", "zig",
    "gdb", "lldb", "valgrind",
    // archive
    "tar", "zip", "unzip", "gzip", "gunzip", "bzip2", "xz", "7z",
    // misc
    "echo", "printf", "env", "which", "whereis", "type", "command",
    "man", "info", "help", "history", "alias", "unalias",
    "open", "xdg-open", "pbcopy", "pbpaste", "say",
    "watch", "time", "timeout", "sleep", "yes", "true", "false",
    "nohup", "screen", "tmux", "jobs", "fg", "bg", "wait",
    "test", "expr", "bc", "base64", "md5", "sha256sum", "shasum",
    "sudo", "su", "doas",
    "vim", "vi", "nano", "emacs", "code", "subl",
    "tree", "bat", "exa", "eza", "fd", "fzf", "ripgrep",
];

pub fn classify(input: &str) -> InputKind {
    let trimmed = input.trim();

    // Force shell: !command
    if let Some(cmd) = trimmed.strip_prefix('!') {
        let cmd = cmd.trim();
        if cmd.is_empty() {
            return InputKind::AgentQuery(trimmed.to_string());
        }
        return classify_shell_or_builtin(cmd);
    }

    // Force LLM: ?question
    if let Some(q) = trimmed.strip_prefix('?') {
        return InputKind::AgentQuery(q.trim().to_string());
    }

    // Handle builtins before general detection
    if let Some(builtin) = try_builtin(trimmed) {
        return InputKind::Builtin(builtin);
    }

    // Path-based execution: ./script, /usr/bin/thing, ~/bin/thing
    if trimmed.starts_with("./")
        || trimmed.starts_with('/')
        || trimmed.starts_with("~/")
    {
        return InputKind::ShellCommand(trimmed.to_string());
    }

    // Pipe or redirect — definitely a shell command
    if trimmed.contains(" | ")
        || trimmed.contains(" > ")
        || trimmed.contains(" >> ")
        || trimmed.contains(" < ")
        || trimmed.contains(" 2>&1")
        || trimmed.contains(" && ")
        || trimmed.contains(" || ")
        || trimmed.ends_with(" &")
    {
        return InputKind::ShellCommand(trimmed.to_string());
    }

    // ENV_VAR=value command pattern
    if trimmed.contains('=') {
        let first = trimmed.split_whitespace().next().unwrap_or("");
        if first.contains('=') && first.chars().next().map(|c| c.is_ascii_uppercase()).unwrap_or(false) {
            return InputKind::ShellCommand(trimmed.to_string());
        }
    }

    // Check first token against known commands
    let tokens: Vec<&str> = trimmed.split_whitespace().collect();
    let first_token = tokens.first().map(|t| t.to_lowercase()).unwrap_or_default();

    if KNOWN_COMMANDS.contains(&first_token.as_str()) {
        // Before treating as shell, check if the rest looks like natural language.
        // "who are you?" → agent.  "who -a" → shell.  "who" → shell.
        if looks_like_natural_language(trimmed, &tokens) {
            return InputKind::AgentQuery(trimmed.to_string());
        }
        return InputKind::ShellCommand(trimmed.to_string());
    }

    // Default: treat as natural language for the agent
    InputKind::AgentQuery(trimmed.to_string())
}

/// Words that strongly signal natural language when they appear as the
/// second token after a known command.  These are pronouns, articles,
/// linking/auxiliary verbs, and question words that almost never appear
/// as shell arguments.
const NL_SIGNAL_WORDS: &[&str] = &[
    // pronouns
    "me", "my", "i", "you", "your", "he", "she", "it", "its",
    "we", "our", "they", "their", "this", "that", "these", "those",
    "myself", "yourself",
    // articles
    "a", "an", "the",
    // auxiliary / linking verbs
    "is", "are", "was", "were", "am", "do", "does", "did",
    "can", "could", "would", "should", "will", "shall", "may", "might",
    "has", "have", "had",
    // question words
    "what", "how", "why", "where", "when", "which", "who",
    // common NL glue
    "please", "about", "really", "actually", "just", "not",
    "don't", "doesn't", "isn't", "aren't", "shouldn't", "can't",
    "want", "need", "like", "some", "every", "all",
    "into", "from", "with", "without", "using", "through",
];

fn looks_like_natural_language(input: &str, tokens: &[&str]) -> bool {
    // Trailing `?` is a very strong NL signal: "who are you?", "find the file?"
    if input.ends_with('?') {
        return true;
    }

    // Single-word input that matched a command → run as shell ("ls", "who", "date")
    if tokens.len() <= 1 {
        return false;
    }

    // If the second token starts with `-` it's a flag → shell
    let second = tokens[1].to_lowercase();
    if second.starts_with('-') {
        return false;
    }

    // If the second token is a known NL signal word → natural language
    if NL_SIGNAL_WORDS.contains(&second.as_str()) {
        return true;
    }

    // If there are 4+ words and most are common English → likely NL
    if tokens.len() >= 4 {
        let nl_count = tokens[1..]
            .iter()
            .filter(|t| NL_SIGNAL_WORDS.contains(&t.to_lowercase().as_str()))
            .count();
        let ratio = nl_count as f64 / (tokens.len() - 1) as f64;
        if ratio >= 0.4 {
            return true;
        }
    }

    false
}

fn classify_shell_or_builtin(cmd: &str) -> InputKind {
    if let Some(builtin) = try_builtin(cmd) {
        InputKind::Builtin(builtin)
    } else {
        InputKind::ShellCommand(cmd.to_string())
    }
}

fn try_builtin(input: &str) -> Option<BuiltinCmd> {
    let trimmed = input.trim();

    // cd
    if trimmed == "cd" {
        return Some(BuiltinCmd::Cd("~".to_string()));
    }
    if let Some(path) = trimmed.strip_prefix("cd ") {
        return Some(BuiltinCmd::Cd(path.trim().to_string()));
    }

    // export VAR=value
    if let Some(rest) = trimmed.strip_prefix("export ") {
        let rest = rest.trim();
        if let Some(eq_pos) = rest.find('=') {
            let key = rest[..eq_pos].trim().to_string();
            let val = rest[eq_pos + 1..].trim().to_string();
            return Some(BuiltinCmd::Export(key, val));
        }
    }

    // clear
    if trimmed == "clear" || trimmed == "cls" {
        return Some(BuiltinCmd::Clear);
    }

    None
}
