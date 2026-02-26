use std::sync::atomic::{AtomicBool, Ordering};

static DRY_RUN: AtomicBool = AtomicBool::new(false);

pub struct DryRunMode;

impl DryRunMode {
    pub fn enable() {
        DRY_RUN.store(true, Ordering::SeqCst);
    }

    pub fn disable() {
        DRY_RUN.store(false, Ordering::SeqCst);
    }

    pub fn is_active() -> bool {
        DRY_RUN.load(Ordering::SeqCst)
    }
}

/// Patterns that are unambiguously destructive / catastrophic.
/// wget/curl are NOT blocked — they are legitimate agent tools.
const DANGEROUS_PATTERNS: &[&str] = &[
    "rm -rf /",
    "rm -rf /*",
    "mkfs.",
    "dd if=/dev/zero",
    "> /dev/sda",
    "chmod -R 777 /",
    ":(){:|:&};:",
    "mv / ",
];

const DESTRUCTIVE_PREFIXES: &[&str] = &[
    "rm -rf",
    "mkfs",
    "fdisk",
    "parted",
    "shutdown",
    "reboot",
    "init 0",
    "init 6",
];

pub fn is_dangerous(command: &str) -> bool {
    let lower = command.to_lowercase();
    DANGEROUS_PATTERNS.iter().any(|p| lower.contains(p))
}

pub fn safety_check(command: &str) -> (bool, Option<&'static str>) {
    if is_dangerous(command) {
        return (false, Some("Command matches a known dangerous pattern"));
    }
    (true, None)
}

/// Returns true if the command is potentially destructive and should be logged.
pub fn requires_confirmation(command: &str) -> bool {
    let lower = command.to_lowercase();
    DESTRUCTIVE_PREFIXES.iter().any(|p| lower.starts_with(p))
}
