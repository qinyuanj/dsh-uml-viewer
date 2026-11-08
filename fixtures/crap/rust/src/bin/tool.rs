//! A binary below `src/bin` is its own crate: namespace `tool`.

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.len() > 1 {
        println!("{}", crap_demo::classify(args.len() as i32));
    }
}

fn helper(flag: bool) -> bool {
    !flag || flag
}
