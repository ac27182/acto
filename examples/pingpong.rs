use acto::{Actor, Mailbox, Queue, Sender, SenderGone, MPSC, SPSC};
use derive_more::From;
use std::{sync::mpsc::RecvError, time::Instant};
use tokio::{
    runtime::{Builder, Handle},
    sync::oneshot,
};

#[derive(Debug, From)]
enum Error {
    E(std::io::Error),
    S(SenderGone),
    R(RecvError),
    T(tokio::sync::oneshot::error::RecvError),
}

struct Ping<R: Queue<Msg = u32>> {
    count: u32,
    // senders statically distinguish SPSC/MPSC, but we don’t care and check at runtime
    reply: Sender<R>,
}

// simple pong actor: respond to ping with the contained pong value
async fn pong<Q: Queue<Msg = Ping<R>>, R: Queue<Msg = u32>>(
    mut mailbox: Mailbox<Q>,
) -> Result<(), SenderGone> {
    loop {
        let Ping { count, mut reply } = mailbox.next().await?;
        reply.send_mut(count);
    }
}

async fn ping(mut mailbox: Mailbox<MPSC<u32>>) -> Result<(), Error> {
    // create pong actor on its own thread
    let rt = Builder::new_multi_thread().worker_threads(1).build()?;
    let mut _pong = Actor::spawn::<SPSC<Ping<MPSC<u32>>>>(&rt, pong, ());
    let mut pong = Actor::spawn(&rt, pong::<SPSC<Ping<MPSC<u32>>>, _>, ());

    // loop to handle all pong messages (u32)
    loop {
        let count = mailbox.next().await?;
        if count == 0 {
            break;
        }
        pong.send_mut(Ping {
            count: count - 1,
            reply: mailbox.me(),
        });
    }

    // This actor stops when receiving a pong of zero, so clean up correctly.
    // (dropping a Runtime in async context will panic)
    // This also “kills” the pong actor.
    Handle::current().spawn_blocking(|| drop(rt));
    Ok(())
}

#[tokio::main]
async fn main() -> Result<(), Error> {
    // receive supervisor results here
    let (tx, rx) = oneshot::channel();

    // run the ping actor on its own thread
    let rt = Builder::new_multi_thread().worker_threads(1).build()?;
    let ping = Actor::spawn(&rt, ping, tx);

    // inject some pinging rounds
    let start = Instant::now();
    ping.send(1000000);
    ping.send(1000000);
    ping.send(1000000);
    ping.send(1000000);

    // wait on supervisor channel — the result of the `ping` actor’s function body
    rx.await??;

    // dropping a runtime is “not allowed” in an async function (wat)
    Handle::current().spawn_blocking(|| drop(rt));

    println!("elapsed: {:?}", start.elapsed());
    Ok(())
}
