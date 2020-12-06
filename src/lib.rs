#[macro_use]
extern crate glib;
#[macro_use]
extern crate gstreamer as gst;
extern crate gstreamer_base as gst_base;
extern crate gstreamer_video as gst_video;
extern crate once_cell;

mod s3multiframesink;
fn plugin_init(plugin: &gst::Plugin) -> Result<(), glib::BoolError> {
    s3multiframesink::register(plugin)?;
    Ok(())
}

gst_plugin_define!(
    s3multiframesink,
    env!("CARGO_PKG_DESCRIPTION"),
    plugin_init,
    concat!(env!("CARGO_PKG_VERSION"), "-", env!("COMMIT_ID")),
    "MIT/X11",
    env!("CARGO_PKG_NAME"),
    env!("CARGO_PKG_NAME"),
    "Fake Repository Field",
    env!("BUILD_REL_DATE")
);
