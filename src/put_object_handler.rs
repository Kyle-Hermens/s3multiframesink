use futures_retry::ErrorHandler;
use futures_retry::RetryPolicy;
use rand::prelude::StdRng;
use rand::Rng;
use rusoto_core::RusotoError;
use rusoto_s3::PutObjectError;
use std::convert::TryInto;
use std::ops::{Div, Mul};
use std::time::Duration;

pub struct PutObjectHandler {
    max_attempts: usize,
    frame_num: u64,
    jitter_max: Duration,
    jitter_base: Duration,
    rng: StdRng,
}

impl PutObjectHandler {
    pub fn new(max_attempts: usize, frame_num: u64) -> Self {
        PutObjectHandler {
            max_attempts,
            frame_num,
            jitter_max: Duration::from_secs(32),
            jitter_base: Duration::from_millis(5),
            rng: rand::SeedableRng::from_entropy(),
        }
    }
    pub fn jitter(&mut self, attempt: usize) -> Duration {
        let temp = self
            .jitter_max
            .min(self.jitter_base.mul((2_u32).pow(attempt as u32))); // integer conversion should be safe, unless an absurd amount of retries are expected
        temp / 2
            + Duration::from_millis(
                self.rng
                    .gen_range(0, temp.div(2).as_millis())
                    .try_into()
                    .unwrap_or(u64::MAX),
            )
    }
}

impl ErrorHandler<RusotoError<PutObjectError>> for PutObjectHandler {
    type OutError = RusotoError<PutObjectError>;

    fn handle(
        &mut self,
        attempt: usize,
        error: RusotoError<PutObjectError>,
    ) -> RetryPolicy<Self::OutError> {
        if attempt > self.max_attempts {
            eprintln!(
                "Attempts exhausted uploading frame {}. Error: {}",
                self.frame_num, error
            );
            RetryPolicy::ForwardError(error)
        } else {
            eprintln!(
                "Frame {} Attempt {}/{} has failed",
                self.frame_num, attempt, self.max_attempts
            );
            RetryPolicy::WaitRetry(self.jitter(attempt))
        }
    }
}
