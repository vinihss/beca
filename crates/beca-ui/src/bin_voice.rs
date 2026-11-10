use clap::Parser;
use std::path::PathBuf;

#[derive(Parser, Debug)]
#[command(about = "Capture mic audio, show Slint UI, transcribe via voice daemon")]
struct CliArgs {
    /// Output WAV path (temp file used if omitted)
    #[arg(short = 'o', long = "output")]
    output: Option<PathBuf>,
    /// Transcribe an existing audio file (no capture/UI)
    #[arg(short = 'i', long = "input")]
    input: Option<PathBuf>,
    /// Run command with transcript as a single argument
    #[arg(long = "run")]
    run: Option<String>,
    /// Keep UI open and show transcript after success (breaks q-voice pipe/exit contract)
    #[arg(long)]
    hold: bool,
}

fn main() {
    let cli = CliArgs::parse();
    let args = beca_ui::voice::Args {
        output: cli.output,
        input: cli.input,
        run: cli.run,
        hold: cli.hold,
    };
    if let Err(e) = beca_ui::voice::run(args) {
        eprintln!("ERROR: {e}");
        std::process::exit(1);
    }
}
