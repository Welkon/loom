//! Standalone loom front-end. The same commands are also reachable as
//! `emb-agent loom`.

fn main() {
    let mut args: Vec<String> = std::env::args().skip(1).collect();
    if matches!(args.first().map(String::as_str), Some("--version" | "-V")) {
        println!("{}", loom::VERSION);
        return;
    }
    args.insert(0, "loom".to_string());
    let (body, exit) = loom::run(&args);
    println!("{body}");
    std::process::exit(exit.into());
}
