use anyhow::Result;
use spam_block_reqs::begin_requesting;
use std::thread;

fn main() -> Result<()> {
    let _ = env_logger::builder()
        .target(env_logger::Target::Stdout)
        .try_init();
    let mut handlers = Vec::new();
    for _ in 0..8 {
        let handler = thread::spawn(|| {
            begin_requesting("127.0.0.1:8333".parse().unwrap(), None).unwrap();
        });
        handlers.push(handler)
    }
    handlers
        .into_iter()
        .for_each(|handler| handler.join().unwrap());

    Ok(())
}
