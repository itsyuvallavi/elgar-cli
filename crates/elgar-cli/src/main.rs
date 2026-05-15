fn main() {
    let args = std::env::args().skip(1).collect::<Vec<_>>();
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
        .is_some_and(|arg| arg == elgar_cli::CONTROLLER_SMOKE_COMMAND)
    {
        let prompt = elgar_cli::provider_smoke_prompt(&args[1..]);
        let cwd = std::env::current_dir().unwrap_or_else(|_| ".".into());
        match elgar_cli::render_controller_smoke_from_env(&prompt, &cwd, &cwd) {
            Ok(rendered) => println!("{rendered}"),
            Err(error) => {
                eprintln!("{error}");
                std::process::exit(1);
            }
        }
        return;
    }
    if args
        .first()
        .is_some_and(|arg| arg == elgar_cli::TUI_CONTROLLER_SMOKE_COMMAND)
    {
        let prompt = elgar_cli::provider_smoke_prompt(&args[1..]);
        let cwd = std::env::current_dir().unwrap_or_else(|_| ".".into());
        match elgar_cli::render_tui_controller_smoke_from_env(&prompt, &cwd, &cwd) {
            Ok(rendered) => println!("{rendered}"),
            Err(error) => {
                eprintln!("{error}");
                std::process::exit(1);
            }
        }
        return;
    }

    let input = args.join(" ");
    if input.is_empty() {
        println!("{}", elgar_core::renderer::placeholder_message());
        return;
    }

    let cwd = std::env::current_dir().unwrap_or_else(|_| ".".into());
    println!("{}", elgar_cli::render_cli_turn(&input, &cwd, &cwd));
}
