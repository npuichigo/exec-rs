use exec::just;
use futures::executor::block_on;

fn main() {
    block_on(async {
        let sender = just(13);
        println!("result = {}", sender.await.unwrap().unwrap());
    })
}
