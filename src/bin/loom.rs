//! Standalone loom front-end. The same commands are also reachable as
//! `emb-agent loom`.

fn main() {
    let mut args = vec!["loom".to_string()];
    args.extend(std::env::args().skip(1));
    let (body, exit) = loom::run(&args);
    println!("{body}");
    std::process::exit(exit.into());
}
