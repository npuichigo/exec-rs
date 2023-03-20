use exec::{just, sync_wait, then};

fn main() {
    let sender = just(13);
    let sender = then(sender, |x|  { println!("Hello, world"); x + 1});
    let result = sync_wait(sender).unwrap();
    println!("result = {}", result.unwrap());
}
