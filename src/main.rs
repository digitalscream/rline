fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt::init();

    let exit_code = rline_ui::run();
    std::process::exit(exit_code);
}
