use std::{
        thread::{
            JoinHandle,
            sleep,
        },
        time::Duration,
    };

pub(crate) fn wait_if_too_busy_with_callback<T, F: FnMut(T)>(
    threads: &mut Vec<JoinHandle<T>>, 
    max_threads: usize, 
    job: &str, 
    mut callback: F
) {
    if threads.len() >= max_threads {
        if max_threads > 1 {
            println!("Waiting for any one of {} threads {} ...", 
                    threads.len(), job);
        }
        let mut thread_id_finished = None;
        loop {
            for (thread_id, thread) in
                threads.iter().enumerate()
            {
                if thread.is_finished() {
                    thread_id_finished = Some(thread_id);
                    break
                }
            }
            if let None = thread_id_finished {
                sleep(Duration::from_millis(10));
            } else {
                break
            }
        }
        if let Some(thread_id_finished) = thread_id_finished {
            if max_threads > 1 {
                println!("One of {} threads {} ended", threads.len(), job);
            }
            let r = threads
                        .swap_remove(thread_id_finished)
                        .join()
                        .expect("Failed to join finished thread");
            callback(r);
        } else {
            panic!("Failed to get finished thread ID")
        }
    }
}

pub(crate) fn wait_if_too_busy<T>(
    threads: &mut Vec<JoinHandle<T>>, max_threads: usize, job: &str
) {
    wait_if_too_busy_with_callback(threads, max_threads, job, |_|());
}

pub(crate) fn wait_remaining_with_callback<T, F: FnMut(T)>(
    mut threads: Vec<JoinHandle<T>>, job: &str, mut callback: F
) {
    let mut changed = true;
    while threads.len() > 0 {
        if changed {
            println!("Waiting for all {} threads {} ...", threads.len(), job);
        }
        changed = false;
        let mut thread_id_finished = None;
        for (thread_id, thread) in
            threads.iter().enumerate()
        {
            if thread.is_finished() {
                thread_id_finished = Some(thread_id);
                break
            }
        }
        match thread_id_finished {
            Some(thread_id) => {
                println!("One of {} threads {} ended", threads.len(), job);
                let r = threads
                    .swap_remove(thread_id)
                    .join()
                    .expect("Failed to join finished thread");
                callback(r);
                changed = true;
            },
            None => sleep(Duration::from_millis(10)),
        }
    }
    println!("Finished waiting for all threads {}", job);
}

pub(crate) fn wait_remaining<T>(threads: Vec<JoinHandle<T>>, job: &str) {
    wait_remaining_with_callback(threads, job, |_|());
}