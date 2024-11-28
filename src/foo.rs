use std::{
    future::Future,
    pin::Pin,
    task::{Context, Poll},
    time::Duration,
};

use crate::{sleep, spawn, Sleep};

fn foo(n: u64) -> Foo {
    let started = false;
    let duration = Duration::from_secs(1);
    let sleep = Box::pin(sleep(duration));
    Foo { n, started, sleep }
}

struct Foo {
    n: u64,
    started: bool,
    sleep: Pin<Box<Sleep>>,
}

impl Future for Foo {
    type Output = ();

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        if !self.started {
            println!("start {}", self.n);
            self.started = true;
        }

        if self.sleep.as_mut().poll(cx).is_pending() {
            return Poll::Pending;
        }
        println!("end {}", self.n);
        Poll::Ready(())
    }
}

pub async fn async_main() {
    let mut task_handles = Vec::new();
    for n in 1..=10 {
        task_handles.push(spawn(foo(n)));
    }

    for handle in task_handles {
        handle.await;
    }
}
