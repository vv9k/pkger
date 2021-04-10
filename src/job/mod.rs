mod build;
mod oneshot;

pub use build::BuildCtx;
pub use oneshot::OneShotCtx;

use crate::Result;

pub trait Ctx {
    fn id(&self) -> &str;
}

pub enum JobCtx<'j> {
    Build(BuildCtx),
    OneShot(OneShotCtx<'j>),
}

pub struct JobRunner<'j> {
    pub ctx: JobCtx<'j>,
}

impl<'j> JobRunner<'j> {
    pub fn new<J: Into<JobCtx<'j>>>(ctx: J) -> JobRunner<'j> {
        JobRunner { ctx: ctx.into() }
    }

    pub async fn run(mut self) -> Result<()> {
        match &mut self.ctx {
            JobCtx::Build(ctx) => {
                ctx.run().await?;
            }
            JobCtx::OneShot(ctx) => {
                ctx.run().await?;
            }
        }

        Ok(())
    }
}
