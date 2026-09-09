//! Encode frames and mixed audio into an mp4. One implementation for every platform.
//!
//! The reader next door already made this choice and stated why: one implementation beats four,
//! because three of the four are not exercised daily and break in silence. Writing was the last
//! piece still done per-platform.
//!
//! **The encoder is hardware, chosen by name.** Not a licence detail — a hard constraint: this crate
//! ships against an LGPL FFmpeg build, so `libx264` is not available and never will be. What IS
//! available in a stock LGPL build is the platform's own encoder, reached through FFmpeg like any
//! other codec:
//!
//!   * `h264_videotoolbox` — macOS, iOS
//!   * `h264_mediacodec`   — Android
//!   * `libopenh264`       — fallback, BSD, software, slower
//!
//! So this stays one code path: same muxing, same frame conversion, same timing; only the encoder
//! name differs. A machine with none of them is not an error — it is `NoEncoder`, and the caller
//! sends the job to a server.
//!
//! **Timestamps come from the frame count**, never from the caller. Two places adding up time is two
//! places for it to drift, and drift between picture and sound is the kind of fault nobody sees
//! until the video is out.
use std::path::Path;

use ffmpeg_next as ff;

use crate::Rgba;

/// What one export needs. Mirrors the core's `VideoSpec` without depending on it: this crate stays
/// usable on its own, and the thin adapter in the product repo does the conversion.
#[derive(Clone, Debug)]
pub struct Spec {
    pub width: u32,
    pub height: u32,
    pub fps: u32,
    pub video_bitrate: u32,
    /// Mixed audio, or `None` for a silent export — the preview path uses that.
    pub audio: Option<AudioSpec>,
}

#[derive(Clone, Debug)]
pub struct AudioSpec {
    pub sample_rate: u32,
    pub channels: u16,
    pub bitrate: u32,
}

#[derive(Debug)]
pub enum Error {
    /// No usable encoder on this machine. Not a failure — the caller falls back to a server queue.
    NoEncoder(String),
    /// Broke midway: disk full, file locked, hardware refused.
    Failed(String),
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NoEncoder(e) => write!(f, "no usable video encoder: {e}"),
            Self::Failed(e) => write!(f, "writing video failed: {e}"),
        }
    }
}

impl std::error::Error for Error {}

/// Encoders to try, best first. Only names a stock LGPL build can carry.
///
/// Ordered by what the machine can actually do rather than by platform: asking "which OS is this"
/// gets the answer wrong on the cases that matter — a Mac without VideoToolbox permission, an
/// Android build whose MediaCodec has no h264 profile. Trying in order and taking the first that
/// opens answers the real question.
const ENCODERS: &[&str] = &["h264_videotoolbox", "h264_mediacodec", "libopenh264"];

/// The audio half, absent for a silent export.
struct Audio {
    encoder: ff::encoder::Audio,
    stream: usize,
    /// Interleaved samples not yet handed to the encoder. AAC only accepts whole frames of a fixed
    /// size, so a caller pushing arbitrary chunks leaves a remainder every time.
    pending: Vec<f32>,
    channels: usize,
    /// Samples per channel already encoded — the only source of audio timestamps, for the same
    /// reason the frame count is the only source of video ones.
    samples: i64,
    time_base: ff::Rational,
}

pub struct Writer {
    octx: ff::format::context::Output,
    video: ff::encoder::Video,
    video_stream: usize,
    audio: Option<Audio>,
    scaler: ff::software::scaling::Context,
    /// Frames pushed so far — the only source of presentation timestamps.
    frames: i64,
    /// The ENCODER's time base, 1/fps. Not the stream's: the mp4 muxer overwrites the stream time
    /// base with its own timescale (1/15360 is typical), so rescaling from the stream converts a
    /// value into the units it is already in and the whole video collapses to a few milliseconds.
    /// Caught by `writes_a_file_the_reader_can_open`: 25 frames at 25 fps came out as 0.0019s.
    time_base: ff::Rational,
    path: std::path::PathBuf,
}

/// Open a writer for `path`. An existing file is REMOVED first: the muxer will not overwrite, and a
/// user who pressed "build" expects the new file, not a refusal about the old one.
pub fn open(path: &Path, spec: &Spec) -> Result<Writer, Error> {
    ff::init().map_err(|e| Error::Failed(e.to_string()))?;
    if path.exists() {
        std::fs::remove_file(path).map_err(|e| Error::Failed(e.to_string()))?;
    }
    if spec.width % 2 != 0 || spec.height % 2 != 0 {
        // h264 in 4:2:0 cannot express odd dimensions. Saying so here beats a muxer error that
        // reads like a bug in the caller's maths.
        return Err(Error::Failed(format!("dimensions must be even, got {}×{}", spec.width, spec.height)));
    }

    let mut octx = ff::format::output(&path).map_err(|e| Error::Failed(e.to_string()))?;
    let (codec, video) = open_encoder(spec)?;

    let mut stream = octx
        .add_stream(codec)
        .map_err(|e| Error::Failed(e.to_string()))?;
    stream.set_parameters(&video);
    stream.set_time_base(ff::Rational::new(1, spec.fps as i32));
    let video_stream = stream.index();

    // Both streams have to exist BEFORE the header goes out — an mp4 header lists them, and adding
    // one afterwards is not a thing the muxer will do.
    let audio = match &spec.audio {
        Some(a) => Some(open_audio(&mut octx, a)?),
        None => None,
    };

    octx.write_header().map_err(|e| Error::Failed(e.to_string()))?;

    // Frames arrive RGBA from the renderer; h264 wants YUV420P. One scaler for the whole run — it
    // holds tables, and rebuilding it per frame is the difference between real time and not.
    let scaler = ff::software::scaling::Context::get(
        ff::format::Pixel::RGBA,
        spec.width,
        spec.height,
        ff::format::Pixel::YUV420P,
        spec.width,
        spec.height,
        ff::software::scaling::Flags::BILINEAR,
    )
    .map_err(|e| Error::Failed(e.to_string()))?;

    let time_base = ff::Rational::new(1, spec.fps as i32);
    Ok(Writer { octx, video, video_stream, audio, scaler, frames: 0, time_base, path: path.to_path_buf() })
}

/// Open the AAC encoder and add its stream.
///
/// AAC and not something simpler: it is what mp4 carries everywhere, it is in a stock LGPL build,
/// and FFmpeg's own encoder needs no external library. The alternative — writing raw PCM into mp4 —
/// plays in almost nothing.
fn open_audio(octx: &mut ff::format::context::Output, spec: &AudioSpec) -> Result<Audio, Error> {
    let codec = ff::encoder::find(ff::codec::Id::AAC)
        .ok_or_else(|| Error::NoEncoder("aac: not in this build".into()))?;
    let mut enc = ff::codec::context::Context::new_with_codec(codec)
        .encoder()
        .audio()
        .map_err(|e| Error::Failed(e.to_string()))?;
    let channels = spec.channels.max(1) as u16;
    enc.set_rate(spec.sample_rate as i32);
    enc.set_channel_layout(ff::channel_layout::ChannelLayout::default(channels as i32));
    // FLTP: AAC wants planar floats. The caller hands over interleaved samples, and `push_audio`
    // deinterleaves — one small copy in a place that owns the format, rather than a rule every
    // caller has to remember.
    enc.set_format(ff::format::Sample::F32(ff::format::sample::Type::Planar));
    enc.set_bit_rate(spec.bitrate as usize);
    enc.set_time_base(ff::Rational::new(1, spec.sample_rate as i32));
    let encoder = enc.open_as(codec).map_err(|e| Error::Failed(e.to_string()))?;

    let mut stream = octx.add_stream(codec).map_err(|e| Error::Failed(e.to_string()))?;
    stream.set_parameters(&encoder);
    stream.set_time_base(ff::Rational::new(1, spec.sample_rate as i32));
    let index = stream.index();

    Ok(Audio {
        encoder,
        stream: index,
        pending: Vec::new(),
        channels: channels as usize,
        samples: 0,
        time_base: ff::Rational::new(1, spec.sample_rate as i32),
    })
}

impl Audio {
    /// Hand one whole frame to the encoder, deinterleaving on the way.
    fn send(&mut self, block: &[f32], per_frame: usize) -> Result<(), Error> {
        let mut frame = ff::frame::Audio::new(
            ff::format::Sample::F32(ff::format::sample::Type::Planar),
            per_frame,
            self.encoder.channel_layout(),
        );
        for c in 0..self.channels {
            let plane: &mut [f32] = frame.plane_mut(c);
            for (i, slot) in plane.iter_mut().enumerate().take(per_frame) {
                *slot = block[i * self.channels + c];
            }
        }
        frame.set_pts(Some(self.samples));
        self.samples += per_frame as i64;
        self.encoder.send_frame(&frame).map_err(|e| Error::Failed(e.to_string()))
    }
}

/// Try each encoder in turn and keep the first that opens.
fn open_encoder(spec: &Spec) -> Result<(ff::codec::codec::Codec, ff::encoder::Video), Error> {
    let mut tried: Vec<String> = Vec::new();
    for name in ENCODERS {
        let Some(codec) = ff::encoder::find_by_name(name) else {
            tried.push(format!("{name}: not in this build"));
            continue;
        };
        let ctx = match ff::codec::context::Context::new_with_codec(codec).encoder().video() {
            Ok(c) => c,
            Err(e) => {
                tried.push(format!("{name}: {e}"));
                continue;
            }
        };
        let mut enc = ctx;
        enc.set_width(spec.width);
        enc.set_height(spec.height);
        enc.set_format(ff::format::Pixel::YUV420P);
        enc.set_time_base(ff::Rational::new(1, spec.fps as i32));
        enc.set_frame_rate(Some(ff::Rational::new(spec.fps as i32, 1)));
        enc.set_bit_rate(spec.video_bitrate as usize);
        // One keyframe every two seconds: seeking in the editor lands close, and the file stays
        // small enough to upload. Without this some encoders emit a single keyframe for the whole
        // video and every seek decodes from the start.
        enc.set_gop(spec.fps * 2);
        match enc.open_as(codec) {
            Ok(opened) => return Ok((codec, opened)),
            Err(e) => tried.push(format!("{name}: {e}")),
        }
    }
    Err(Error::NoEncoder(tried.join(" · ")))
}

impl Writer {
    /// Push one RGBA frame. Its timestamp is the frame index — see the module note.
    pub fn push(&mut self, frame: &Rgba) -> Result<(), Error> {
        let mut src = ff::frame::Video::new(ff::format::Pixel::RGBA, frame.width, frame.height);
        // `data_mut` hands back a padded plane: FFmpeg aligns each row, so copying the whole buffer
        // in one go writes into the padding and shears the picture. Copy row by row.
        let stride = src.stride(0);
        let row = (frame.width * 4) as usize;
        for y in 0..frame.height as usize {
            let dst = &mut src.data_mut(0)[y * stride..y * stride + row];
            dst.copy_from_slice(&frame.pixels[y * row..(y + 1) * row]);
        }

        let mut yuv = ff::frame::Video::empty();
        self.scaler.run(&src, &mut yuv).map_err(|e| Error::Failed(e.to_string()))?;
        yuv.set_pts(Some(self.frames));
        self.frames += 1;

        self.video.send_frame(&yuv).map_err(|e| Error::Failed(e.to_string()))?;
        self.drain()
    }

    /// Move whatever the encoder has produced into the file.
    fn drain(&mut self) -> Result<(), Error> {
        let mut packet = ff::Packet::empty();
        while self.video.receive_packet(&mut packet).is_ok() {
            packet.set_stream(self.video_stream);
            packet.rescale_ts(
                self.time_base,
                self.octx.stream(self.video_stream).unwrap().time_base(),
            );
            packet
                .write_interleaved(&mut self.octx)
                .map_err(|e| Error::Failed(e.to_string()))?;
        }
        Ok(())
    }

    /// Push interleaved samples. Their timestamp is the running sample count — see the module note.
    ///
    /// Any length is accepted. AAC only takes whole frames of a fixed size, so whatever does not
    /// fill one waits here for the next call; `finish` flushes the remainder.
    pub fn push_audio(&mut self, samples: &[f32]) -> Result<(), Error> {
        let Some(audio) = self.audio.as_mut() else {
            return Err(Error::Failed("this writer carries no audio: open it with a spec that has one".into()));
        };
        audio.pending.extend_from_slice(samples);
        let per_frame = audio.encoder.frame_size().max(1) as usize;
        let chunk = per_frame * audio.channels;
        while audio.pending.len() >= chunk {
            let block: Vec<f32> = audio.pending.drain(..chunk).collect();
            audio.send(&block, per_frame)?;
            Self::drain_audio(&mut self.octx, audio)?;
        }
        Ok(())
    }

    /// Move whatever the audio encoder has produced into the file.
    ///
    /// A free function over `&mut` fields rather than a method: `push_audio` already holds a mutable
    /// borrow of the audio half, and the borrow checker will not lend out `self` again.
    fn drain_audio(octx: &mut ff::format::context::Output, audio: &mut Audio) -> Result<(), Error> {
        let mut packet = ff::Packet::empty();
        while audio.encoder.receive_packet(&mut packet).is_ok() {
            packet.set_stream(audio.stream);
            packet.rescale_ts(audio.time_base, octx.stream(audio.stream).unwrap().time_base());
            packet.write_interleaved(octx).map_err(|e| Error::Failed(e.to_string()))?;
        }
        Ok(())
    }

    /// Close the file. Returns bytes written.
    ///
    /// Takes `self`, not `&mut self`: once the trailer is out the writer is done, and the type says
    /// so instead of leaving someone to push into a closed file.
    pub fn finish(mut self) -> Result<u64, Error> {
        // Tell the encoder no more frames are coming, then move out what it still holds. Skipping
        // this loses the last frames — and the video simply ends early, with nothing to see wrong.
        self.video.send_eof().map_err(|e| Error::Failed(e.to_string()))?;
        self.drain()?;

        if let Some(audio) = self.audio.as_mut() {
            // The remainder is padded with silence rather than dropped. Dropping it cuts the last
            // fraction of a second off the sound, which lands exactly on the final word.
            if !audio.pending.is_empty() {
                let per_frame = audio.encoder.frame_size().max(1) as usize;
                let mut block = std::mem::take(&mut audio.pending);
                block.resize(per_frame * audio.channels, 0.0);
                audio.send(&block, per_frame)?;
            }
            audio.encoder.send_eof().map_err(|e| Error::Failed(e.to_string()))?;
            Self::drain_audio(&mut self.octx, audio)?;
        }

        self.octx.write_trailer().map_err(|e| Error::Failed(e.to_string()))?;
        std::fs::metadata(&self.path)
            .map(|m| m.len())
            .map_err(|e| Error::Failed(e.to_string()))
    }
}
