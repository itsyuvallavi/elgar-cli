use std::io::IsTerminal;

fn main() {
    let args = std::env::args().skip(1).collect::<Vec<_>>();
    if args.is_empty() {
        if elgar_cli::should_launch_terminal_tui_by_default(
            std::io::stdin().is_terminal(),
            std::io::stdout().is_terminal(),
        ) {
            if let Err(error) = elgar_cli::run_tui_terminal() {
                eprintln!("TUI terminal failed: {error}");
                std::process::exit(1);
            }
        } else {
            println!("{}", elgar_core::renderer::placeholder_message());
        }
        return;
    }

    if args
        .first()
        .is_some_and(|arg| arg == elgar_cli::PROVIDER_SMOKE_COMMAND)
    {
        let prompt = elgar_cli::provider_smoke_prompt(&args[1..]);
        match elgar_cli::run_provider_smoke_from_env(&prompt) {
            Ok(output) => println!("{output}"),
            Err(error) => {
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
        let paths = elgar_cli::RuntimePaths::from_current_dir();
        let policy_mode = match elgar_cli::runtime_permission_policy_mode(&paths.project_root) {
            Ok(mode) => mode,
            Err(error) => {
                eprintln!("TUI failed: {error}");
                std::process::exit(1);
            }
        };
        let stdin = std::io::stdin();
        let stdout = std::io::stdout();
        if let Err(error) = elgar_cli::run_tui_loop_with_policy(
            stdin.lock(),
            stdout.lock(),
            &paths.project_root,
            &paths.cwd,
            policy_mode,
        ) {
            eprintln!("TUI failed: {error}");
            std::process::exit(1);
        }
        return;
    }
    if args
        .first()
        .is_some_and(|arg| arg == elgar_cli::TUI_TERMINAL_COMMAND)
    {
        if let Err(error) = elgar_cli::run_tui_terminal() {
            eprintln!("TUI terminal failed: {error}");
            std::process::exit(1);
        }
        return;
    }
    if args
        .first()
        .is_some_and(|arg| arg == elgar_cli::PERF_BASELINE_COMMAND)
    {
        let report = elgar_cli::perf::run_perf_baseline();
        println!("{}", elgar_cli::perf::render_perf_baseline(&report));
        return;
    }

    let input = args.join(" ");
    let paths = elgar_cli::RuntimePaths::from_current_dir();
    match elgar_cli::render_cli_turn_from_runtime_config(&input, &paths.project_root, &paths.cwd) {
        Ok(rendered) => println!("{rendered}"),
        Err(error) => {
            eprintln!("{error}");
            std::process::exit(1);
        }
    }
}
