use std::{
        collections::HashMap,
        thread::{
            JoinHandle,
            sleep,
        },
        time::Duration,
    };

use crate::error::{
        Error,
        Result
    };

// pub(crate) struct ThreadPool<T: Send + 'static> 
// {
//     threads: Vec<JoinHandle<Result<T>>>,
//     max: usize,
//     job: String,
// }

// impl<T: Send + 'static> ThreadPool<T> {
//     pub(crate) fn new<S: AsRef<str>>(max: usize, job: S) -> Self {
//         Self {
//             threads: Vec::new(),
//             max,
//             job: job.as_ref().to_string()
//         }
//     }

//     fn is_busy(&self) -> bool {
//         self.threads.len() >= self.max
//     }

//     /// Wait for any one thread
//     fn wait_one(&mut self) -> Result<Result<T>> {
//         if self.threads.is_empty() {
//             log::error!("Trying to wait for thread when no thread available");
//             return Err(Error::ThreadFailure(None))
//         }
//         let mut thread_id_finished = None;
//         loop {
//             for (thread_id, thread) in
//                 self.threads.iter().enumerate()
//             {
//                 if thread.is_finished() {
//                     thread_id_finished = Some(thread_id);
//                     break
//                 }
//             }
//             if let None = thread_id_finished {
//                 sleep(Duration::from_millis(10));
//             } else {
//                 break
//             }
//         }
//         if let Some(thread_id_finished) = thread_id_finished {
//             match self.threads.swap_remove(
//                 thread_id_finished).join()
//             {
//                 Ok(r) => Ok(r),
//                 Err(e) => {
//                     log::error!("Failed to join finished thread: {:?}", e);
//                     Err(Error::ThreadFailure(None))
//                 },
//             }
//         } else {
//             log::error!("Failed to get finished thread ID");
//             Err(Error::ThreadFailure(None))
//         }
//     }

//     /// Try to spawn a thread and push it to the pool, wait first if too busy
//     /// 
//     /// return `Err(e)` if something bad happended
//     /// 
//     /// return `Ok(None)` if successfully added and waited no thread
//     /// 
//     /// return `Ok(Some(Result<T>))` if successfully added after waiting for
//     /// another thread
//     /// 
//     pub(crate) fn push<F>(&mut self, spawner: F) -> Result<Option<Result<T>>> 
//     where
//         F: FnOnce() -> Result<T>,
//         F: Send + 'static,
//     {
//         let result_finished = 
//             if self.is_busy() {
//                 None
//             } else {
//                 match self.wait_one() {
//                     Ok(r) => Some(r),
//                     Err(e) => return Err(e),
//                 }
//             };
//         self.threads.push(std::thread::spawn(spawner));
//         Ok(result_finished)
//     }

//     pub(crate) fn wait_all(&mut self) -> Vec<Result<T>> {
//         self.threads.into_iter().map(|thread|
//             match thread.join() {
//                 Ok(r) => r,
//                 Err(e) => 
//                     Err(Error::ThreadFailure(Some(e))),
//             }).collect()
//     }

//     pub(crate) fn wait_all_check(&mut self) -> (Vec<Result<T>>, Result<()>) {
//         let results = self.wait_all();
//         for result in results.iter() {
//             if result.is_err() {
//                 return (results, Err(Error::ThreadFailure(None)))
//             }
//         }
//         (results, Ok(()))
//     }
// }

// struct ThreadMap<T: Send + 'static>

// pub(crate) struct ThreadPool<T: Send + 'static> 
// {
//     threads: Vec<JoinHandle<Result<T>>>,
//     max: usize,
//     job: String,
// }


pub(crate) fn wait_if_too_busy<T>(
    threads: &mut Vec<JoinHandle<Result<T>>>,
    max_threads: usize,
    job: &str,
) -> Result<()>
{
    if threads.len() >= max_threads {
        if max_threads > 1 {
            log::info!("Waiting for any one of {} threads {} ...",
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
                log::info!("One of {} threads {} ended", threads.len(), job);
            }
            match threads
                        .swap_remove(thread_id_finished)
                        .join()
            {
                Ok(r) => return r,
                Err(e) => {
                    log::error!("Failed to join finished thread: {:?}", e);
                    return Err(Error::ThreadFailure(Some(e)))
                },
            }
        } else {
            log::error!("Failed to get finished thread ID");
            return Err(Error::ThreadFailure(None))
        }
    }
    Ok(())
}

pub(crate) fn wait_remaining(
    mut threads: Vec<JoinHandle<Result<()>>>, job: &str
) -> Result<()>
{
    if threads.len() == 0 {
        return Ok(())
    }
    let mut changed = true;
    let mut bad_threads = 0;
    while threads.len() > 0 {
        if changed {
            log::info!("Waiting for {} threads {} ...", threads.len(), job);
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
                log::info!("One of {} threads {} ended", threads.len(), job);
                match threads
                    .swap_remove(thread_id)
                    .join()
                {
                    Ok(r) => match r {
                        Ok(_) => (),
                        Err(_) => bad_threads += 1,
                    },
                    Err(e) => {
                        log::error!(
                            "Failed to join finished thread: {:?}", e);
                        bad_threads += 1;
                    },
                };
                changed = true;
            },
            None => sleep(Duration::from_millis(10)),
        }
    }
    log::info!("Finished waiting for all threads {}", job);
    if bad_threads > 0 {
        log::error!("{} threads {} has bad return", bad_threads, job);
        Err(Error::ThreadFailure(None))
    } else {
        Ok(())
    }
}

pub(crate) fn wait_thread_map<T>(
    map: &mut HashMap<T, Vec<JoinHandle<Result<()>>>>, job: &str
) -> Result<()>
{
    let mut bad = false;
    for threads in map.values_mut() {
        if threads.len() == 0 {
            continue
        }
        loop {
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
                    log::info!("One of {} threads {} ended", threads.len(), job);
                    match threads
                        .swap_remove(thread_id)
                        .join()
                    {
                        Ok(r) => match r {
                            Ok(_) => (),
                            Err(_) => bad = true,
                        },
                        Err(e) => {
                            log::error!(
                                "Failed to join finished thread: {:?}", e);
                            bad = true;
                        },
                    };
                },
                None => break,
            }
        }
    }
    if bad {
        Err(())
    } else {
        Ok(())
    }
}