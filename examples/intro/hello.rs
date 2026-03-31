// ~/~ begin <<book/src/using-this-book.md#examples/intro/hello.rs>>[init]
// ~/~ begin <<book/src/using-this-book.md#intro-imports>>[init]
use std::io::{self, Write};
// ~/~ end

fn main() {
    // ~/~ begin <<book/src/using-this-book.md#intro-prompt-greet>>[init]
    print!("what is your name? ");
    io::stdout().flush().unwrap();
    // ~/~ end
    // ~/~ begin <<book/src/using-this-book.md#intro-prompt-greet>>[1]
    let mut name = String::new();
    io::stdin().read_line(&mut name).unwrap();
    println!("hello, {}!", name.trim());
    // ~/~ end
}
// ~/~ end
