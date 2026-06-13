//! Binary entrypoint for the `elgar` command.
//!
//! This file only decides which CLI mode to run, prints user-facing output, and
//! exits with the right process status. The real behavior lives in `elgar_cli`.

use std::io::IsTerminal;

fn main() {
    elgar_cli::init_terminal_logging();
    let args = std::env::args().skip(1).collect::<Vec<_>>();
    log::debug!("elgar_cli_start args={}", args.len());
    if args.is_empty() {
        if elgar_cli::should_launch_terminal_tui_by_default(
            std::io::stdin().is_terminal(),
            std::io::stdout().is_terminal(),
        ) {
            log::info!("launching terminal tui by default");
            if let Err(error) = elgar_cli::run_tui_terminal() {
                log::error!("terminal tui failed: {error}");
                eprintln!("TUI terminal failed: {error}");
                std::process::exit(1);
            }
        } else {
            log::debug!("non-interactive empty invocation rendered placeholder");
            println!("{}", elgar_core::renderer::placeholder_message());
        }
        return;
    }

    if args
        .first()
        .is_some_and(|arg| arg == elgar_cli::PROVIDER_SMOKE_COMMAND)
    {
        log::info!("running provider smoke");
        let prompt = elgar_cli::provider_smoke_prompt(&args[1..]);
        match elgar_cli::run_provider_smoke_from_env(&prompt) {
            Ok(output) => println!("{output}"),
            Err(error) => {
                log::error!("provider smoke failed: {error}");
                eprintln!("{error}");
                std::process::exit(1);
            }
        }
        return;
    }
    if elgar_cli::is_logs_latest_command(&args) {
        log::info!("running logs latest diagnostic");
        let paths = elgar_cli::RuntimePaths::from_current_dir();
        match elgar_cli::render_logs_latest_from_args(&args, &paths.project_root) {
            Ok(output) => println!("{output}"),
            Err(error) => {
                log::error!("logs latest failed: {error}");
                eprintln!("{error}");
                std::process::exit(1);
            }
        }
        return;
    }
    if elgar_cli::is_mcp_command(&args) {
        log::info!("running mcp diagnostic");
        let paths = elgar_cli::RuntimePaths::from_current_dir();
        match elgar_cli::render_mcp_from_args(&args, &paths.project_root) {
            Ok(output) => println!("{output}"),
            Err(error) => {
                log::error!("mcp diagnostic failed: {error}");
                eprintln!("{error}");
                std::process::exit(1);
            }
        }
        return;
    }
    if args
        .first()
        .is_some_and(|arg| arg == elgar_cli::TUI_COMMAND)
    {
        log::info!("running script tui");
        let paths = elgar_cli::RuntimePaths::from_current_dir();
        let stdin = std::io::stdin();
        let stdout = std::io::stdout();
        if let Err(error) = elgar_cli::run_tui_loop_from_runtime_config(
            stdin.lock(),
            stdout.lock(),
            &paths.project_root,
            &paths.cwd,
        ) {
            log::error!("script tui failed: {error}");
            eprintln!("TUI failed: {error}");
            std::process::exit(1);
        }
        return;
    }
    if args
        .first()
        .is_some_and(|arg| arg == elgar_cli::TUI_TERMINAL_COMMAND)
    {
        log::info!("running terminal tui");
        if let Err(error) = elgar_cli::run_tui_terminal() {
            log::error!("terminal tui failed: {error}");
            eprintln!("TUI terminal failed: {error}");
            std::process::exit(1);
        }
        return;
    }
    let input = args.join(" ");
    log::info!(
        "running single cli turn input_chars={}",
        input.chars().count()
    );
    let paths = elgar_cli::RuntimePaths::from_current_dir();
    match elgar_cli::render_cli_turn_from_runtime_config(&input, &paths.project_root, &paths.cwd) {
        Ok(rendered) => println!("{rendered}"),
        Err(error) => {
            log::error!("single cli turn failed: {error}");
            eprintln!("{error}");
            std::process::exit(1);
        }
    }
}
