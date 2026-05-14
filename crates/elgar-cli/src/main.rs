fn main() {
    let input = std::env::args().skip(1).collect::<Vec<_>>().join(" ");
    if input.is_empty() {
        println!("{}", elgar_core::renderer::placeholder_message());
        return;
    }

    let cwd = std::env::current_dir().unwrap_or_else(|_| ".".into());
    println!("{}", elgar_cli::render_cli_turn(&input, &cwd, &cwd));
}
