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
) -> Result<(), ()> 
{
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
            let r = match threads
                        .swap_remove(thread_id_finished)
                        .join() {
                Ok(r) => r,
                Err(e) => {
                    eprintln!("Failed to join finished thread: {:?}", e);
                    return Err(())
                },
            };
            callback(r);
        } else {
            eprintln!("Failed to get finished thread ID");
            return Err(())
        }
    }
    Ok(())
}

pub(crate) fn wait_if_too_busy<T>(
    threads: &mut Vec<JoinHandle<T>>, max_threads: usize, job: &str
) -> Result<(), ()>
{
    wait_if_too_busy_with_callback(threads, max_threads, job, |_|())
}

pub(crate) fn wait_remaining_with_callback<T, F: FnMut(T)>(
    mut threads: Vec<JoinHandle<T>>, job: &str, mut callback: F
) -> Result<(), ()>
{
    if threads.len() == 0 {
        return Ok(())
    }
    let mut changed = true;
    let mut bad_threads = 0;
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
                match threads
                    .swap_remove(thread_id)
                    .join() 
                {
                    Ok(r) => callback(r),
                    Err(e) => {
                        eprintln!(
                            "Failed to join finished thread: {:?}", e);
                        bad_threads += 1;
                    },
                };
                changed = true;
            },
            None => sleep(Duration::from_millis(10)),
        }
    }
    println!("Finished waiting for all threads {}", job);
    if bad_threads > 0 {
        eprintln!("{} threads has bad return", bad_threads);
        Err(())
    } else {
        Ok(())
    }
}

pub(crate) fn wait_remaining<T>(threads: Vec<JoinHandle<T>>, job: &str) 
    -> Result<(), ()>
{
    wait_remaining_with_callback(threads, job, |_|())
}