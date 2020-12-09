# S3MultiFrameSink - A Gstreamer plugin for uploading video frames to an S3 bucket

## Purpose
This is a [gstreamer](https://gstreamer.freedesktop.org/) plugin written using the [Rust](https://www.rust-lang.org/) bindings provided by the [gstreamer-rs](https://gitlab.freedesktop.org/gstreamer/gstreamer-rs)  and [gst-plugin-rs](https://gitlab.freedesktop.org/gstreamer/gst-plugins-rs) libraries.
This plugin is intended to be used with other existing gstreamer plugins that extract frames from a video stream. Each frame the plugin receives
is uploaded into an [Amazon S3](https://aws.amazon.com/s3/) bucket using [Rusoto](https://github.com/rusoto/rusoto).

## Background
This plugin was originally designed as a supplement to an animation-based machine learning project. The intent of the project was to produce a model that could accept
two key frames of animation and produce an in-between frame. To produce the training data for this model, existing animations needed to be broken down into
component frames. There would have been other processing steps in the extraction pipeline, including removing similar frames that were part of a loop or hue-shifted copies of another frame,
but the project was put on hold and so further development has ceased.

## Usage
To use this plugin, it is recommended that you run the Dockerfile provided in the project, and execute the gstreamer pipeline from a shell inside of the resulting container. This saves you from
installing all of the gstreamer dependencies and creating the appropriate environment variables. However, you will need to create a .aws folder in the project with the appropriate 
[credentials](https://github.com/rusoto/rusoto/blob/master/AWS-CREDENTIALS.md) for the S3 bucket, as documented [here](https://github.com/rusoto/rusoto/blob/master/AWS-CREDENTIALS.md) .
If you wish to apply the plugin to a new video file, ensure that the file ends up in the running container, either via docker cp or by having it within the project directory before the image is generated. The plugin's [sink cap](https://gstreamer.freedesktop.org/documentation/additional/design/caps.html?gi-language=c) 
only accepts image data, so one of the preceding plugins in the pipeline should be one that produces [image data](https://gstreamer.freedesktop.org/documentation/plugin-development/advanced/media-types.html?gi-language=c), like ```pngenc``` or ```jpegenc```. 



### Build the Container
```
docker build -t s3multiframesink .
```
### Run the Container
```
docker build -t s3multiframesink .
```
### Upload frames from a video file in png format
```
gst-launch-1.0 filesrc location=/s3multiframesink/deja_vu.mp4 ! decodebin ! queue ! videoconvert ! videoscale ! pngenc ! s3multiframesink bucket=example-bucket-rusoto key=deja_vu region=us-west-2 extension=png
```

## Properties

* **Bucket** 
  * The name of the S3 bucket.
  * If the bucket does not exist, the plugin will attempt to create it in the same region specified in the region property.
* **Region**
  * The AWS region where the S3 bucket exists or should be created.
  * The proper format for the property is a hyphenated string, e.g. ```us-central-1``` 
  * Specifying the wrong region for a bucket that already exists will result in a 301 response from AWS that the plugin does not currently handle.
* **Key**
  * The prefix for the name of each frame object in S3.
  * The name of each frame will follow the format ```{key}/frame{frame_number}.{extension}```. Single digit frames will be padded with a zero for better lexical sorting.
* **Extension**
  * The file extension for the output frames.
  * This property should match the input file type, and should not contain a dot.
  * Valid options are ```jpeg``` , ```png```, ```tiff```, ```gif``` or one of the appropriate variants for the same file types, e.g. ```jpg``` for JPEG files
  
    



