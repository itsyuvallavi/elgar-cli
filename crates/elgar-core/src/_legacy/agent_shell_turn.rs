use crate::action::ShellActionVerification;

#[derive(Debug, Clone)]
pub(crate) struct ShellTransaction {
    active: bool,
    primary_command_seen: bool,
    primary_command_class: ShellCommandClass,
    result_conclusive: bool,
}

impl ShellTransaction {
    pub(crate) fn new(shell_execution: bool, explicit_tool_command: bool) -> Self {
        let active = shell_execution && !explicit_tool_command;
        Self {
            active,
            primary_command_seen: false,
            primary_command_class: ShellCommandClass::Generic,
            result_conclusive: false,
        }
    }

    pub(crate) fn observe_verified_shell(&mut self, shell: &ShellActionVerification) {
        if !self.active || self.primary_command_seen {
            return;
        }
        self.primary_command_seen = true;
        self.primary_command_class = classify_shell_command(&shell.command);
        self.result_conclusive = shell.timed_out || shell.exit_code.is_some();
    }

    pub(crate) fn should_synthesize_now(&self) -> bool {
        self.active
            && self.primary_command_seen
            && self.result_conclusive
            && self.primary_command_class != ShellCommandClass::DevServer
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
enum ShellCommandClass {
    Build,
    Test,
    Lint,
    Install,
    DevServer,
    #[default]
    Generic,
}

fn classify_shell_command(command: &str) -> ShellCommandClass {
    let normalized = command.split_whitespace().collect::<Vec<_>>().join(" ");
    let lower = normalized.to_ascii_lowercase();
    if matches_shell_command_prefix(
        &lower,
        &[
            "npm run dev",
            "npm dev",
            "pnpm dev",
            "yarn dev",
            "bun dev",
            "next dev",
            "vite",
        ],
    ) {
        return ShellCommandClass::DevServer;
    }
    if matches_shell_command_prefix(
        &lower,
        &[
            "npm run build",
            "npm build",
            "pnpm build",
            "yarn build",
            "bun run build",
            "cargo build",
            "go build",
        ],
    ) {
        return ShellCommandClass::Build;
    }
    if matches_shell_command_prefix(
        &lower,
        &[
            "npm test",
            "npm run test",
            "pnpm test",
            "yarn test",
            "bun test",
            "cargo test",
            "go test",
        ],
    ) {
        return ShellCommandClass::Test;
    }
    if matches_shell_command_prefix(
        &lower,
        &[
            "npm run lint",
            "npm lint",
            "pnpm lint",
            "yarn lint",
            "cargo clippy",
        ],
    ) {
        return ShellCommandClass::Lint;
    }
    if matches_shell_command_prefix(
        &lower,
        &[
            "npm install",
            "pnpm install",
            "yarn install",
            "cargo install",
        ],
    ) {
        return ShellCommandClass::Install;
    }
    ShellCommandClass::Generic
}

fn matches_shell_command_prefix(command: &str, prefixes: &[&str]) -> bool {
    prefixes.iter().any(|prefix| {
        command == *prefix
            || command
                .strip_prefix(prefix)
                .is_some_and(|suffix| suffix.starts_with(' ') || suffix.starts_with(" --"))
    })
}

#[cfg(test)]
mod tests {
    use super::{classify_shell_command, ShellCommandClass, ShellTransaction};

    #[test]
    fn shell_transaction_helpers_synthesize_shell_execution_except_dev_server() {
        let mut transaction = ShellTransaction::new(true, false);
        transaction.observe_verified_shell(&crate::action::ShellActionVerification {
            command: "npm run build".to_string(),
            cwd: ".".to_string(),
            exit_code: Some(0),
            stdout: String::new(),
            stderr: String::new(),
            elapsed_millis: 1,
            timed_out: false,
            stdout_truncated: false,
            stderr_truncated: false,
            verified_effect: None,
        });
        assert!(transaction.should_synthesize_now());

        let mut dev_transaction = ShellTransaction::new(true, false);
        dev_transaction.observe_verified_shell(&crate::action::ShellActionVerification {
            command: "npm run dev".to_string(),
            cwd: ".".to_string(),
            exit_code: None,
            stdout: String::new(),
            stderr: String::new(),
            elapsed_millis: 1,
            timed_out: false,
            stdout_truncated: false,
            stderr_truncated: false,
            verified_effect: None,
        });
        assert!(!dev_transaction.should_synthesize_now());
        assert_eq!(
            classify_shell_command("npm run build"),
            ShellCommandClass::Build
        );
        assert_eq!(
            classify_shell_command("npm run dev"),
            ShellCommandClass::DevServer
        );
    }
}
