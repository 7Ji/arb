use std::{
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

pub(crate) struct ThreadPool<T: Send + 'static> 
{
    threads: Vec<JoinHandle<Result<T>>>,
    max: usize,
    job: String,
}

impl<T: Send + 'static> ThreadPool<T> {
    pub(crate) fn new<S: AsRef<str>>(max: usize, job: S) -> Self {
        Self {
            threads: Vec::new(),
            max,
            job: job.as_ref().to_string()
        }
    }

    fn is_busy(&self) -> bool {
        self.threads.len() >= self.max
    }

    /// Wait for any one thread
    fn wait_one(&mut self) -> Result<Result<T>> {
        if self.threads.is_empty() {
            log::error!("Trying to wait for thread {} when no thread available", 
                        &self.job);
            return Err(Error::ThreadFailure)
        }
        let mut thread_id_finished = None;
        loop {
            for (thread_id, thread) in
                self.threads.iter().enumerate()
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
            match self.threads.swap_remove(
                thread_id_finished).join()
            {
                Ok(r) => Ok(r),
                Err(e) => {
                    log::error!("Failed to join finished thread {}: {:?}", 
                        &self.job, e);
                    Err(Error::ThreadFailure)
                },
            }
        } else {
            log::error!("Failed to get finished thread ID {}", &self.job);
            Err(Error::ThreadFailure)
        }
    }

    /// Try to spawn a thread and push it to the pool, wait first if too busy
    /// 
    /// return `Err(e)` if something bad happended
    /// 
    /// return `Ok(None)` if successfully added and waited no thread
    /// 
    /// return `Ok(Some(Result<T>))` if successfully added after waiting for
    /// another thread
    /// 
    pub(crate) fn push<F>(&mut self, spawner: F) -> Result<Option<Result<T>>> 
    where
        F: FnOnce() -> Result<T>,
        F: Send + 'static,
    {
        let result_finished = 
            if self.is_busy() {
                match self.wait_one() {
                    Ok(r) => Some(r),
                    Err(e) => {
                        log::error!("Failed to wait for existing thread before \
                            pushing to a busy thread pool {}: {}", &self.job,
                            e);
                        return Err(e)
                    },
                }
            } else {
                None
            };
        self.threads.push(std::thread::spawn(spawner));
        Ok(result_finished)
    }

    pub(crate) fn wait_all(self) -> Vec<Result<T>> {
        self.threads.into_iter().map(|thread|
            match thread.join() {
                Ok(r) => r,
                Err(e) => {
                    log::error!("Failed to join thread, artiface: {:?}", e);
                    Err(Error::ThreadFailure)
                },
            }).collect()
    }

    pub(crate) fn wait_all_check(self) -> (Vec<Result<T>>, Result<()>) {
        let job = self.job.clone();
        let results = self.wait_all();
        for result in results.iter() {
            if let Err(e) = result {
                log::error!("One of remaining threads {} failed: {}", &job, e);
                return (results, Err(Error::ThreadFailure))
            }
        }
        (results, Ok(()))
    }
}