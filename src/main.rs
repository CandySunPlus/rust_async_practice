use std::{
    future::Future,
    sync::{Arc, Mutex},
    task::{Context, Waker},
};

use sample::{server, AwaitFlag, DynFuture, NEW_TASKS};

async fn async_main() {
    server::async_main().await.unwrap();
}

fn main() {
    println!("Hello, world!");

    let awake_flag = Arc::new(AwaitFlag(Mutex::new(false)));
    let waker = Waker::from(awake_flag.clone());
    let mut cx = Context::from_waker(&waker);
    let mut main_task = Box::pin(async_main());
    let mut other_tasks = Vec::new();
    loop {
        if main_task.as_mut().poll(&mut cx).is_ready() {
            return;
        }

        let is_pendding = |task: &mut DynFuture| task.as_mut().poll(&mut cx).is_pending();

        other_tasks.retain_mut(is_pendding);

        loop {
            let Some(mut task) = NEW_TASKS.lock().unwrap().pop() else {
                break;
            };

            // Polling this task could spawn more tasks, so it's important that NEW_TASKS is not locked here.
            if task.as_mut().poll(&mut cx).is_pending() {
                other_tasks.push(task);
            }
        }

        if awake_flag.check_and_clear() {
            continue;
        }

        if other_tasks.is_empty() {
            break;
        }
    }
}
