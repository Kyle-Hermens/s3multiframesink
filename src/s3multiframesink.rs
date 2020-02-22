// Licensed under the Apache License, Version 2.0 <LICENSE-APACHE or
// http://www.apache.org/licenses/LICENSE-2.0> or the MIT license
// <LICENSE-MIT or http://opensource.org/licenses/MIT>, at your
// option. This file may not be copied, modified, or distributed
// except according to those terms.

use std::fs::File;
use std::sync::Mutex;

use glib;
use glib::subclass;
use glib::subclass::prelude::*;
use gst;
use gst::prelude::*;
use gst::subclass::prelude::*;
use gst_base;
use gst_base::subclass::prelude::*;
// use file_location::FileLocation;
use once_cell::sync::{Lazy};
use rusoto_core::Region;
use rusoto_s3::{S3Client, PutObjectRequest, S3};
use std::thread;
use std::thread::JoinHandle;
use std::time::Duration;

// use url::Url;

// use tokio::runtime;
const DEFAULT_BUFFER_SIZE: u64 = 5 * 1024 * 1024;
// const DEFAULT_LOCATION: Option<FileLocation> = None;

#[derive(Debug)]
struct Settings {
    bucket: Option<String>,
    key: Option<String>,
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            bucket: Default::default(),
            key: Default::default(),
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
            glib::ParamFlags::READWRITE, /* + GST_PARAM_MUTABLE_READY) */
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
            glib::ParamFlags::READWRITE, /* + GST_PARAM_MUTABLE_READY) */
        )
    }),
];


enum State {
    Stopped,
    Started { join_handles: Vec<JoinHandle<()>>, frame_num: u64 },
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

static CLIENT: Lazy<S3Client> = Lazy::new(|| {
    S3Client::new(Region::UsWest2)
});

impl ObjectSubclass for S3MultiFrameSink {
    const NAME: &'static str = "RsS3MultiFrameSink";
    type ParentType = gst_base::BaseSink;
    type Instance = gst::subclass::ElementInstanceStruct<Self>;
    type Class = subclass::simple::ClassStruct<Self>;

    glib_object_subclass!();

    fn new() -> Self {
        Self {
            settings: Mutex::new(Default::default()),
            state: Mutex::new(Default::default()),
        }
    }

    // fn type_init(type_: &mut subclass::InitializingType<Self>) {
    //     type_.add_interface::<gst::URIHandler>();
    // }

    fn class_init(klass: &mut subclass::simple::ClassStruct<Self>) {
        klass.set_metadata(
            "s3 sink intended for png frame data",
            "Sink/S3",
            "Write individual frames to S3",
            "?",
        );

        let caps = gst::Caps::new_any();
        let sink_pad_template = gst::PadTemplate::new(
            "sink",
            gst::PadDirection::Sink,
            gst::PadPresence::Always,
            &caps,
        )
            .unwrap();
        klass.add_pad_template(sink_pad_template);

        klass.install_properties(&PROPERTIES);
    }
}

impl ObjectImpl for S3MultiFrameSink {
    glib_object_impl!();

    fn set_property(&self, obj: &glib::Object, id: usize, value: &glib::Value) {
        let prop = &PROPERTIES[id];
        match *prop {
            subclass::Property("bucket", ..) => {
                // let element = obj.downcast_ref::<gst_base::BaseSink>().unwrap();
                let res = match value.get::<String>() { //TODO: redo this so it just sets a bucket;
                    Ok(Some(bucket)) => {
                        self.settings.lock().unwrap().bucket = Some(bucket);
                    }
                    Ok(None) => {panic!("S3 bucket name not set!")} //TODO: more sensible flow control
                    Err(_) => unreachable!("type checked upstream"),
                };
            },
            subclass::Property("key", ..) => {
                let res = match value.get::<String>() {
                    Ok(Some(key)) => {
                        self.settings.lock().unwrap().key = Some(key);
                    }
                    Ok(None) => { panic!("S3 key name not set!") } //TODO: more sensible flow control
                    Err(_) => unreachable!("type checked upstream"),
                };
            },
            _ => unimplemented!(),
        };
    }

    fn get_property(&self, _obj: &glib::Object, id: usize) -> Result<glib::Value, ()> {
        let prop = &PROPERTIES[id];
        match *prop {
            subclass::Property("bucket", ..) => {
                let settings = self.settings.lock().unwrap();
                let location = settings
                    .bucket
                    .as_ref()
                    .map(|location| location.to_string());

                Ok(location.to_value())
            },
            subclass::Property("key", ..) => {
                let settings = self.settings.lock().unwrap();
                let location = settings
                    .key
                    .as_ref()
                    .map(|location| location.to_string());

                Ok(location.to_value())
            }

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

        *state = State::Started {
            join_handles: Vec::with_capacity(512),
            frame_num: 0,

        };
        gst_info!(CAT, obj: element, "Started");

        Ok(())
    }

    fn stop(&self, element: &gst_base::BaseSink) -> Result<(), gst::ErrorMessage> {
        let mut state = self.state.lock().unwrap();

        let join_handles = match *state {
            State::Started {
                ref mut join_handles,
                ..
            } => join_handles,
            State::Stopped => {
                return Err(gst_error_msg!(
                gst::ResourceError::Settings,
                ["S3MultiFrameSink not started"]
            ));
            }
        };

        let handles = std::mem::replace(join_handles, vec![]);
        for handle in handles {
            handle.join().expect("A joined task has failed");
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
        let (join_handles, frame_num) = match *state {
            State::Started {
                ref mut join_handles,
                ref mut frame_num,
            } => (join_handles, frame_num, ),
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
        let frame_count = *frame_num;
        *frame_num += 1;
        let settings = self.settings.lock().unwrap();
        let bucket =   settings.bucket.as_ref().unwrap().clone();
        let key  = settings.key.as_ref().unwrap().clone();
        let handle = thread::spawn(move || {
            let mut put_request = PutObjectRequest {
                bucket: bucket.to_owned(), //TODO: get these from frame
                key: format!("{}/frame0{}.png", key, frame_count),
                body: Some(vec.clone().into()),
                ..Default::default()
            };
            while let Err(e) = CLIENT.put_object(put_request).sync() {
                println!("Image upload failed for frame {}, {:?}", frame_count, e);
                thread::sleep(Duration::from_secs(1));
                put_request = PutObjectRequest {
                    bucket: "example-bucket-rusoto".to_owned(), //TODO: get these from frame
                    key: format!("deja_vu/frame0{}.png", frame_count),
                    body: Some(vec.clone().into()),
                    ..Default::default()
                };
            }
        }
        );
        join_handles.push(handle);
        Ok(gst::FlowSuccess::Ok)
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
