// ~/~ begin <<book/src/introduction.md#examples/intro/hello.rs>>[init]
// ~/~ begin <<book/src/introduction.md#intro-imports>>[init]
use std::io::{self, Write};
// ~/~ end

fn main() {
    // ~/~ begin <<book/src/introduction.md#intro-prompt-greet>>[init]
    print!("what is your name? ");
    io::stdout().flush().unwrap();
    // ~/~ end
    // ~/~ begin <<book/src/introduction.md#intro-prompt-greet>>[1]
    let mut name = String::new();
    io::stdin().read_line(&mut name).unwrap();
    println!("hello, {}!", name.trim());
    // ~/~ end
}
// ~/~ end
