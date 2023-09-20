use std::{
        thread::{
            JoinHandle,
            sleep,
        },
        time::Duration,
    };

pub(crate) fn wait_if_too_busy<T>(
    threads: &mut Vec<JoinHandle<T>>, max_threads: usize
) {
    if threads.len() >= max_threads {
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
            threads
                .swap_remove(thread_id_finished)
                .join()
                .expect("Failed to join finished thread");
        } else {
            panic!("Failed to get finished thread ID")
        }
    }
}

pub(crate) fn wait_if_too_busy_with_callback<T, F: FnMut(T)>(
    threads: &mut Vec<JoinHandle<T>>, max_threads: usize, mut callback: F
) {
    if threads.len() >= max_threads {
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

pub(crate) fn wait_also_print<T>(mut threads: Vec<JoinHandle<T>>, job: &str) {
    let mut ender = '🕐';
    while threads.len() > 0 {
        ender = match ender {
            '🕐' => '🕒',
            '🕒' => '🕕',
            '🕕' => '🕘',
            '🕘' => '🕐',
            _ => panic!("Unexpected character")
        };
        print!("Waiting for {} threads {} ... {}\r", threads.len(), job, ender);
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
                threads
                    .swap_remove(thread_id)
                    .join()
                    .expect("Failed to join finished thread");
            },
            None => sleep(Duration::from_millis(100)),
        }
    }
}