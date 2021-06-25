mod build;
mod oneshot;

pub use build::BuildCtx;
pub use oneshot::OneShotCtx;

use pkger_core::docker;

use async_trait::async_trait;
use std::time::{Duration, Instant};

#[async_trait]
pub trait Ctx {
    type JobResult;

    fn id(&self) -> &str;
    async fn run(&mut self) -> Self::JobResult;
}

pub enum JobResult {
    Success {
        id: String,
        duration: Duration,
        output: String,
    },
    Failure {
        id: String,
        duration: Duration,
        reason: String,
    },
}

impl JobResult {
    pub fn success<I, O>(id: I, duration: Duration, output: O) -> Self
    where
        I: Into<String>,
        O: Into<String>,
    {
        Self::Success {
            id: id.into(),
            duration,
            output: output.into(),
        }
    }

    pub fn failure<I, E>(id: I, duration: Duration, err: E) -> Self
    where
        I: Into<String>,
        E: Into<String>,
    {
        Self::Failure {
            id: id.into(),
            duration,
            reason: err.into(),
        }
    }
}

pub enum JobCtx<'job> {
    Build(BuildCtx),
    OneShot(OneShotCtx<'job>),
}

impl<'job> JobCtx<'job> {
    pub async fn run(self) -> JobResult {
        let start = Instant::now();
        match self {
            JobCtx::Build(mut ctx) => match ctx.run().await {
                Err(e) => {
                    let duration = start.elapsed();
                    let reason = match e.downcast::<docker::Error>() {
                        Ok(err) => match err {
                            docker::Error::Fault { code: _, message } => message,
                            e => e.to_string(),
                        },
                        Err(e) => e.to_string(),
                    };
                    JobResult::failure(ctx.id(), duration, reason)
                }
                Ok(output) => JobResult::success(
                    ctx.id(),
                    start.elapsed(),
                    output.to_string_lossy().to_string(),
                ),
            },
            JobCtx::OneShot(mut ctx) => {
                if let Err(e) = ctx.run().await {
                    let duration = start.elapsed();
                    let reason = match e.downcast::<docker::Error>() {
                        Ok(err) => match err {
                            docker::Error::Fault { code: _, message } => message,
                            e => e.to_string(),
                        },
                        Err(e) => e.to_string(),
                    };
                    JobResult::failure(ctx.id(), duration, reason)
                } else {
                    JobResult::success(ctx.id(), start.elapsed(), "".to_string())
                }
            }
        }
    }
}
