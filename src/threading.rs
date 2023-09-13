use std::{thread::{JoinHandle, sleep}, time::Duration};

pub(crate) fn wait_if_too_busy(threads: &mut Vec<JoinHandle<()>>, max_threads: usize) {
    let mut thread_id_finished = None;
    if threads.len() > max_threads {
        while let None = thread_id_finished {
            for (thread_id, thread) in threads.iter().enumerate() {
                if thread.is_finished() {
                    thread_id_finished = Some(thread_id);
                }
            }
            sleep(Duration::from_millis(10));
        }
        if let Some(thread_id_finished) = thread_id_finished {
            threads.swap_remove(thread_id_finished).join().expect("Failed to join finished thread");
        } else {
            panic!("Failed to get finished thread ID")
        }
    }
}