use anyhow::Result;
use tokio_cron_scheduler::{Job, JobScheduler};
use tracing::info;

pub struct CronScheduler {
    scheduler: JobScheduler,
}

impl CronScheduler {
    pub async fn new() -> Result<Self> {
        Ok(Self {
            scheduler: JobScheduler::new().await?,
        })
    }

    pub async fn start(&self) -> Result<()> {
        self.scheduler.start().await?;
        info!("WorkBuddy cron scheduler started");
        Ok(())
    }

    pub async fn add_job<F>(
        &self,
        cron_expr: &str,
        task_id: String,
        callback: F,
    ) -> Result<uuid::Uuid>
    where
        F: Fn(
                uuid::Uuid,
                JobScheduler,
            ) -> std::pin::Pin<Box<dyn std::future::Future<Output = ()> + Send>>
            + Send
            + Sync
            + 'static,
    {
        let full_cron = if cron_expr.split_whitespace().count() == 5 {
            format!("0 {cron_expr}")
        } else {
            cron_expr.to_string()
        };
        let callback = std::sync::Arc::new(tokio::sync::Mutex::new(callback));
        let job = Job::new_async(full_cron.as_str(), move |uuid, scheduler| {
            let task_id = task_id.clone();
            let callback = callback.clone();
            Box::pin(async move {
                info!("Running scheduled task: {task_id}");
                callback.lock().await(uuid, scheduler).await;
            })
        })?;
        Ok(self.scheduler.add(job).await?)
    }

    pub async fn remove_job(&self, job_id: uuid::Uuid) -> Result<()> {
        self.scheduler.remove(&job_id).await?;
        Ok(())
    }
}
