use pkger_core::build::{self, Context};
use pkger_core::docker;
use pkger_core::oneshot::{self, OneShotCtx};

use std::time::{Duration, Instant};

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
    Build(Context),
    #[allow(dead_code)]
    OneShot(OneShotCtx<'job>),
}

impl<'job> JobCtx<'job> {
    pub async fn run(self) -> JobResult {
        let start = Instant::now();
        match self {
            JobCtx::Build(mut ctx) => match build::run(&mut ctx).await {
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
                if let Err(e) = oneshot::run(&mut ctx).await {
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
