use parking_lot::Mutex;
use std::collections::HashMap;
use std::sync::Arc;
use std::thread;
use std::time::Duration;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum SchedulerError {
    #[error("Invalid cron expression: {0}")]
    InvalidCron(String),
    #[error("Job already exists: {0}")]
    JobExists(String),
}

type JobFn = Arc<dyn Fn() + Send + Sync>;

#[derive(Clone)]
struct Job {
    schedule: cron::Schedule,
    callable: JobFn,
}

pub struct Scheduler {
    jobs: Arc<Mutex<HashMap<String, Job>>>,
    running: Arc<Mutex<bool>>,
    handle: Mutex<Option<thread::JoinHandle<()>>>,
}

impl Default for Scheduler {
    fn default() -> Self {
        Self::new()
    }
}

impl Scheduler {
    pub fn new() -> Self {
        Self {
            jobs: Arc::new(Mutex::new(HashMap::new())),
            running: Arc::new(Mutex::new(false)),
            handle: Mutex::new(None),
        }
    }

    pub fn register_cron<F>(
        &self,
        name: &str,
        schedule: &str,
        callable: F,
    ) -> Result<(), SchedulerError>
    where
        F: Fn() + Send + Sync + 'static,
    {
        let cron_schedule: cron::Schedule = schedule
            .parse()
            .map_err(|_| SchedulerError::InvalidCron(schedule.to_string()))?;

        let mut jobs = self.jobs.lock();

        if jobs.contains_key(name) {
            return Err(SchedulerError::JobExists(name.to_string()));
        }

        jobs.insert(
            name.to_string(),
            Job {
                schedule: cron_schedule,
                callable: Arc::new(callable),
            },
        );

        Ok(())
    }

    pub fn start(&self) {
        let mut running = self.running.lock();
        if *running {
            return;
        }
        *running = true;
        drop(running);

        let jobs = Arc::clone(&self.jobs);
        let running = Arc::clone(&self.running);

        let handle = thread::spawn(move || {
            let mut last_run: HashMap<String, chrono::DateTime<chrono::Utc>> = HashMap::new();

            while *running.lock() {
                let now = chrono::Utc::now();
                let jobs_snapshot: HashMap<String, Job> = jobs.lock().clone();

                for (name, job) in jobs_snapshot.iter() {
                    let should_run = if let Some(last) = last_run.get(name) {
                        if let Some(next) = job.schedule.after(last).next() {
                            next <= now
                        } else {
                            false
                        }
                    } else {
                        // First run - check if schedule matches current time
                        true
                    };

                    if should_run {
                        (job.callable)();
                        last_run.insert(name.clone(), now);
                    }
                }

                thread::sleep(Duration::from_secs(1));
            }
        });

        *self.handle.lock() = Some(handle);
    }

    pub fn stop(&self) {
        *self.running.lock() = false;

        if let Some(handle) = self.handle.lock().take() {
            let _ = handle.join();
        }
    }
}

impl Drop for Scheduler {
    fn drop(&mut self) {
        self.stop();
    }
}
