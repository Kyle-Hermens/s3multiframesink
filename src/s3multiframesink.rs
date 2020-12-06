// Licensed under the Apache License, Version 2.0 <LICENSE-APACHE or
// http://www.apache.org/licenses/LICENSE-2.0> or the MIT license
// <LICENSE-MIT or http://opensource.org/licenses/MIT>, at your
// option. This file may not be copied, modified, or distributed
// except according to those terms.

use futures_retry::{ErrorHandler, FutureRetry, RetryPolicy};
use glib::subclass;
use glib::subclass::prelude::*;
use gst::prelude::*;
use gst::subclass::prelude::*;
use gst_base::subclass::prelude::*;
use once_cell::sync::Lazy;
use rand::prelude::StdRng;
use rand::Rng;
use rusoto_core::{Region, RusotoError};
use rusoto_s3::{
    CreateBucketConfiguration, CreateBucketError, CreateBucketRequest, PutObjectError,
    PutObjectRequest, S3Client, S3,
};
use std::convert::TryInto;
use std::ops::{Div, Mul};
use std::str::FromStr;
use std::sync::Mutex;
use std::time::Duration;
use tokio::runtime;

#[derive(Debug)]
struct Settings {
    bucket: Option<String>,
    key: Option<String>,
    region: Region,
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            bucket: Default::default(),
            key: Default::default(),
            region: Region::default(),
        }
    }
}

static PROPERTIES: [subclass::Property; 3] = [
    subclass::Property("bucket", |name| {
        glib::ParamSpec::string(
            name,
            "S3 Bucket",
            "The bucket of the file to write",
            None,
            glib::ParamFlags::READWRITE,
        )
    }),
    subclass::Property("key", |name| {
        glib::ParamSpec::string(
            name,
            "Object Key Prefix",
            "The prefix for each object's key, usually the name of the filename",
            None,
            glib::ParamFlags::READWRITE,
        )
    }),
    subclass::Property("region", |name| {
        glib::ParamSpec::string(
            name,
            "AWS Region",
            "An AWS region (e.g. eu-west-2).",
            None,
            glib::ParamFlags::READWRITE,
        )
    }),
];

static RUNTIME: Lazy<runtime::Runtime> = Lazy::new(|| {
    runtime::Builder::new()
        .threaded_scheduler()
        .enable_all()
        .thread_name("gst-s3sink-runtime")
        .build()
        .unwrap()
});

enum State {
    Stopped,
    Started { frame_num: u64, s3client: S3Client },
}

impl Default for State {
    fn default() -> State {
        State::Stopped
    }
}

pub struct S3MultiFrameSink {
    settings: Mutex<Settings>,
    state: Mutex<State>,
}

static CAT: Lazy<gst::DebugCategory> = Lazy::new(|| {
    gst::DebugCategory::new(
        "rsS3MultiFrameSink",
        gst::DebugColorFlags::empty(),
        Some("File Sink"),
    )
});

impl ObjectSubclass for S3MultiFrameSink {
    const NAME: &'static str = "RusotosS3MultiFrameSink";
    type ParentType = gst_base::BaseSink;
    type Instance = gst::subclass::ElementInstanceStruct<Self>;
    type Class = subclass::simple::ClassStruct<Self>;

    glib_object_subclass!();

    fn class_init(klass: &mut subclass::simple::ClassStruct<Self>) {
        klass.set_metadata(
            "s3 sink intended for png frame data",
            "Sink/S3",
            "Writes individual video frames to an S3 bucket",
            "This has to be provided",
        );

        let png_cap = create_image_cap("image/png");
        let sink_pad_template = gst::PadTemplate::new(
            "sink",
            gst::PadDirection::Sink,
            gst::PadPresence::Always,
            &png_cap,
        )
        .unwrap();
        klass.add_pad_template(sink_pad_template);
        klass.install_properties(&PROPERTIES);
    }

    fn new() -> Self {
        Self {
            settings: Mutex::new(Default::default()),
            state: Mutex::new(Default::default()),
        }
    }
}

fn create_image_cap(name: &str) -> gst::Caps {
    gst::Caps::new_simple(
        name,
        &[
            ("width", &gst::IntRange::<i32>::new(0, i32::MAX)),
            ("height", &gst::IntRange::<i32>::new(0, i32::MAX)),
            (
                "framerate",
                &gst::FractionRange::new(gst::Fraction::new(0, 1), gst::Fraction::new(i32::MAX, 1)),
            ),
        ],
    )
}

impl ObjectImpl for S3MultiFrameSink {
    glib_object_impl!();

    fn set_property(&self, _: &glib::Object, id: usize, value: &glib::Value) {
        let prop = &PROPERTIES[id];
        let mut settings = self.settings.lock().unwrap();
        match *prop {
            subclass::Property("bucket", ..) => {
                settings.bucket = value.get::<String>().expect("type checked upstream");
            }
            subclass::Property("key", ..) => {
                settings.key = value.get::<String>().expect("Type checked upstream");
            }
            subclass::Property("region", ..) => {
                settings.region = Region::from_str(
                    &value
                        .get::<String>()
                        .expect("Type checked upstream")
                        .expect("region value not provided"),
                )
                .expect("invalid region provided");
            }
            _ => unimplemented!(),
        };
    }

    fn get_property(&self, _: &glib::Object, id: usize) -> Result<glib::Value, ()> {
        let prop = &PROPERTIES[id];

        let settings = self.settings.lock().unwrap();
        match *prop {
            subclass::Property("bucket", ..) => {
                let bucket = settings
                    .bucket
                    .as_ref()
                    .map(|location| location.to_string());
                Ok(bucket.to_value())
            }
            subclass::Property("key", ..) => {
                let key = settings.key.as_ref().map(|location| location.to_string());
                Ok(key.to_value())
            }
            subclass::Property("region", ..) => Ok(settings.region.name().to_value()),
            _ => unimplemented!(),
        }
    }
}

impl ElementImpl for S3MultiFrameSink {}

impl BaseSinkImpl for S3MultiFrameSink {
    fn start(&self, element: &gst_base::BaseSink) -> Result<(), gst::ErrorMessage> {
        let mut state = self.state.lock().unwrap();
        if let State::Started { .. } = *state {
            unreachable!("S3MultiFrameSink already started");
        }

        let settings = self.settings.lock().unwrap();
        let s3client = S3Client::new(settings.region.clone());
        drop(settings);
        self.create_bucket_if_extant(&s3client)?;

        *state = State::Started {
            frame_num: 0,
            s3client,
        };
        gst_info!(CAT, obj: element, "Started");

        Ok(())
    }

    fn stop(&self, element: &gst_base::BaseSink) -> Result<(), gst::ErrorMessage> {
        let mut state = self.state.lock().unwrap();
        if let State::Stopped = *state {
            return Err(gst_error_msg!(
                gst::ResourceError::Settings,
                ["S3MultiFrameSink not started"]
            ));
        }
        *state = State::Stopped;
        gst_info!(CAT, obj: element, "Stopped");

        Ok(())
    }


    fn render(
        &self,
        element: &gst_base::BaseSink,
        buffer: &gst::Buffer,
    ) -> Result<gst::FlowSuccess, gst::FlowError> {
        let mut state = self.state.lock().unwrap();
        let (frame_num, s3client) = match *state {
            State::Started {
                ref mut frame_num,
                ref s3client,
            } => (frame_num, s3client),
            State::Stopped => {
                gst_element_error!(element, gst::CoreError::Failed, ["Not started yet"]);
                return Err(gst::FlowError::Error);
            }
        };

        gst_trace!(CAT, obj: element, "Rendering {:?}", buffer);

        let map = buffer.map_readable().map_err(|_| {
            gst_element_error!(element, gst::CoreError::Failed, ["Failed to map buffer"]);
            gst::FlowError::Error
        })?;
        let vec: Vec<u8> = map.as_ref().to_vec();
        self.upload_image_frame(s3client, frame_num, vec)
    }
}

pub fn register(plugin: &gst::Plugin) -> Result<(), glib::BoolError> {
    gst::Element::register(
        Some(plugin),
        "s3multiframesink",
        gst::Rank::None,
        S3MultiFrameSink::get_type(),
    )
}

struct PutObjectHandler {
    max_attempts: usize,
    frame_num: u64,
    jitter_max: Duration,
    jitter_base: Duration,
    rng: StdRng,
}

impl PutObjectHandler {
    fn new(max_attempts: usize, frame_num: u64) -> Self {
        PutObjectHandler {
            max_attempts,
            frame_num,
            jitter_max: Duration::from_secs(32),
            jitter_base: Duration::from_millis(5),
            rng: rand::SeedableRng::from_entropy(),
        }
    }
    fn jitter(&mut self, attempt: usize) -> Duration {
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

impl S3MultiFrameSink {
    fn upload_image_frame(
        &self,
        s3client: &S3Client,
        frame_num: &mut u64,
        vec: Vec<u8>,
    ) -> Result<gst::FlowSuccess, gst::FlowError> {
        *frame_num += 1;
        let settings = self.settings.lock().unwrap();
        let bucket = settings.bucket.as_ref().unwrap().clone();
        let key = settings.key.as_ref().unwrap().clone();
        RUNTIME
            .handle()
            .block_on(FutureRetry::new(
                || {
                    let put_request = S3MultiFrameSink::create_put_object_request(
                        *frame_num, &vec, &bucket, &key,
                    );
                    s3client.put_object(put_request)
                },
                PutObjectHandler::new(5, *frame_num),
            ))
            .map(|_| gst::FlowSuccess::Ok)
            .map_err(|_| gst::FlowError::Error)
    }

    fn create_put_object_request(
        frame_count: u64,
        vec: &Vec<u8>,
        bucket: &str,
        key: &str,
    ) -> PutObjectRequest {
        PutObjectRequest {
            bucket: bucket.to_owned(),
            key: format!("{}/frame{:0>2}.png", key, frame_count.clone()),
            body: Some(vec.clone().into()),
            ..Default::default()
        }
    }

    fn create_bucket_if_extant(&self, s3client: &S3Client) -> Result<(), gst::ErrorMessage> {
        let settings = self.settings.lock().unwrap();
        let bucket = settings
            .bucket
            .as_ref()
            .expect("Bucket should be set by start time")
            .clone();
        RUNTIME.handle().block_on(async {
            let bucket_creation = s3client
                .create_bucket(CreateBucketRequest {
                    acl: None,
                    bucket,
                    create_bucket_configuration: Some(CreateBucketConfiguration {
                        location_constraint: Some(settings.region.name().to_string()),
                    }),
                    grant_full_control: None,
                    grant_read: None,
                    grant_read_acp: None,
                    grant_write: None,
                    grant_write_acp: None,
                    object_lock_enabled_for_bucket: None,
                })
                .await;

            bucket_creation.map(|_| ()).or_else(|error| {
                if let RusotoError::Service(CreateBucketError::BucketAlreadyOwnedByYou(_)) = error {
                    Ok(())
                } else {
                    Err(gst_error_msg!(
                        gst::ResourceError::Settings,
                        [&format!("{}", error)]
                    ))
                }
            })
        })
    }
}
