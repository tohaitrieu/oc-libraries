//! Pull a frame — at any mark (`frame_at`), or reading a clip forward (`frame_forward`).
//!
//! Two doors because they are two different jobs, and picking the wrong one costs tens of times the
//! work while returning exactly the right pixels — so nothing shows it but a clock:
//!
//!   * **`frame_at`** — the timeline's preview strip. Seek, then decode: a frame at the end of a
//!     clip is asked for as often as one at the start, and reading forward to second 200 for a
//!     single frame is absurd.
//!   * **`frame_forward`** — the renderer, walking a clip start to end. Seeking before EVERY frame
//!     means falling back to the nearest keyframe and decoding up to the mark again, over and over,
//!     decoding the same stretch dozens of times. Measured on a real build (09/09): 243 ms a frame,
//!     4 frames a second — a five-minute episode would take thirty-seven minutes on a Mac, and
//!     would not finish at all on a phone.
//!
//! A seek always lands on the nearest keyframe BEFORE the mark, so after seeking there is still
//! decoding to do to reach it. Skip that and the frame comes back seconds away from where the
//! person is looking — wrong in a way that is hard to see, because it is still a valid picture.
use crate::Rgba;

/// The frame being held, and the two marks that bracket it — in the STREAM's own units, not seconds.
///
/// Held because a 25 fps clip feeding a 30 fps render is asked for the same frame several times
/// running, and decoding it again repeats the most expensive step for nothing.
///
/// The marks stay integers instead of seconds because comparing seconds is wrong in the last digit:
/// frame 14 of a 25 fps clip is `14 × 0.04 = 0.5600000000000001`, so mark 0.6 of a 30 fps render
/// falls INSIDE frame 14 instead of frame 15 — the picture freezes for one beat in every twenty, and
/// nothing reveals it except comparing frame by frame against the seeking path.
struct Held {
    /// This frame's `pts`.
    pts: i64,
    /// The `pts` of the frame right BEFORE it. This frame is the right answer for every mark in
    /// `(prev, pts]` — which is exactly `decode_until`'s rule: the first frame at or past the mark.
    prev: i64,
    rgba: Rgba,
}

pub struct Video {
    input: ffmpeg_next::format::context::Input,
    stream: usize,
    decoder: ffmpeg_next::decoder::Video,
    time_base: f64,
    fps: f64,
    duration: f64,
    /// The scaler and the height it serves. Rebuilding it per frame rebuilds a coefficient table per
    /// frame; it only changes when the caller changes size, which callers do not do mid-clip.
    scaler: Option<(u32, ffmpeg_next::software::scaling::Context)>,
    held: Option<Held>,
    /// Whether the held frame still sits where the decoder stands. After a seek it does not: the old
    /// frame stays as the last one successfully decoded, for reading past the end of a clip, but it
    /// no longer answers any mark.
    held_fresh: bool,
}

#[derive(Debug)]
pub enum Error {
    Open(String),
    NoVideoStream,
    Decode(String),
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Open(e) => write!(f, "không mở được tệp: {e}"),
            Self::NoVideoStream => write!(f, "tệp không có luồng hình"),
            Self::Decode(e) => write!(f, "giải mã hỏng: {e}"),
        }
    }
}

impl std::error::Error for Error {}

impl Video {
    pub fn open(path: &std::path::Path) -> Result<Self, Error> {
        ffmpeg_next::init().map_err(|e| Error::Open(e.to_string()))?;
        let input = ffmpeg_next::format::input(&path).map_err(|e| Error::Open(e.to_string()))?;
        let stream = input
            .streams()
            .best(ffmpeg_next::media::Type::Video)
            .ok_or(Error::NoVideoStream)?;
        let index = stream.index();
        let time_base = f64::from(stream.time_base());
        // The AVERAGE rate, not the nominal one: phone footage often has a variable rate, and its
        // nominal rate lies.
        let fps = {
            let r = stream.avg_frame_rate();
            let v = f64::from(r);
            if v > 0.0 { v } else { 30.0 }
        };
        let duration = if stream.duration() > 0 {
            stream.duration() as f64 * time_base
        } else {
            // Some files declare no stream duration; take the container's, in microseconds.
            input.duration() as f64 / 1_000_000.0
        };
        let decoder = ffmpeg_next::codec::context::Context::from_parameters(stream.parameters())
            .map_err(|e| Error::Decode(e.to_string()))?
            .decoder()
            .video()
            .map_err(|e| Error::Decode(e.to_string()))?;
        Ok(Self { input, stream: index, decoder, time_base, fps, duration, scaler: None, held: None, held_fresh: false })
    }

    pub fn width(&self) -> u32 {
        self.decoder.width()
    }

    pub fn height(&self) -> u32 {
        self.decoder.height()
    }

    /// Frames per second of the source clip.
    pub fn fps(&self) -> f64 {
        self.fps
    }

    /// Clip length, in seconds.
    pub fn duration(&self) -> f64 {
        self.duration
    }

    /// Output size for a requested height. `height = 0` keeps the clip's own size.
    pub fn output_size(&self, height: u32) -> (u32, u32) {
        let (w, h) = (self.decoder.width(), self.decoder.height());
        let out_h = if height == 0 { h } else { height };
        let out_w = ((w as f64 / h.max(1) as f64) * out_h as f64).round().max(1.0) as u32;
        (out_w, out_h)
    }

    /// The frame at `at` seconds, scaled to the requested height (aspect kept). `height = 0` keeps
    /// the clip's own size.
    ///
    /// SEEKS on every call — for asking at random, not for reading forward. Reading forward is
    /// `frame_forward`.
    pub fn frame_at(&mut self, at: f64, height: u32) -> Result<Rgba, Error> {
        self.seek_to(at);
        self.decode_until(at, height)?;
        Ok(self.held.as_ref().expect("vừa giải mã xong").rgba.clone())
    }

    /// The frame at `at` seconds, read FORWARD from wherever the last call stopped.
    ///
    /// Seeks only when it must: going backwards, or jumping far ahead. Otherwise it keeps decoding —
    /// which is the whole difference between 4 frames a second and a usable speed.
    ///
    /// Hands back a BORROWED slice rather than a copy: a 1080p frame is 8.3 MB, and the same frame is
    /// asked for repeatedly whenever the clip's rate differs from the render's.
    pub fn frame_forward(&mut self, at: f64, height: u32) -> Result<&Rgba, Error> {
        // A mark past the end takes the last frame instead of seeking outside the file: a scene
        // outlasting its clip is ordinary, and seeking past the end leaves no packets to decode.
        let at = if self.duration > 0.0 { at.min(self.duration - 1.0 / self.fps.max(1.0)) } else { at };
        let ts = self.stream_time(at);
        let out_h = self.output_size(height).1;
        let fresh = self.held_fresh;
        if let Some(h) = &self.held {
            if fresh && h.rgba.height == out_h && ts > h.prev && ts <= h.pts {
                return Ok(&self.held.as_ref().expect("vừa kiểm xong").rgba);
            }
        }

        // Seek when going backwards, or jumping more than a second ahead. Decoding through a long
        // jump decodes a stretch nobody watches; going backwards without seeking has no way back.
        let ahead_limit = self.stream_time(1.0);
        let jump = match (&self.held, fresh) {
            (Some(h), true) => ts <= h.prev || ts > h.pts + ahead_limit,
            _ => true,
        };
        if jump {
            self.seek_to(at);
        }
        self.decode_until(at, height)?;
        Ok(&self.held.as_ref().expect("vừa giải mã xong").rgba)
    }

    /// Seconds → the stream's units, by exactly the arithmetic `decode_until` compares `pts` with.
    fn stream_time(&self, at: f64) -> i64 {
        (at / self.time_base) as i64
    }

    /// Seek to BEFORE the mark and let the caller walk forward — overshooting grabs the next shot.
    ///
    /// TWO different time units, and mixing them fails silently: the file's `seek` takes MICROSECONDS
    /// while a frame's `pts` counts in the stream's own units. Feed stream units to `seek` and the
    /// jump goes nowhere, so the decoder runs from the start of the file — still the right picture,
    /// just slower the further in you go, so nothing about the result looks wrong. Measured on a
    /// 1080×1920 clip: the frame at second 10 took 895 ms, a few milliseconds after the fix.
    fn seek_to(&mut self, at: f64) {
        let micros = (at * 1_000_000.0) as i64;
        let _ = self.input.seek(micros, ..micros);
        self.decoder.flush();
        // The old frame stays but stops answering marks: it is now only the last frame successfully
        // decoded, kept for reading past the end of a clip.
        self.held_fresh = false;
    }

    /// Decode on until the first frame AT or past the mark, then hold it.
    fn decode_until(&mut self, at: f64, height: u32) -> Result<(), Error> {
        let (out_w, out_h) = self.output_size(height);
        if self.scaler.as_ref().map(|(h, _)| *h) != Some(out_h) {
            let (w, h) = (self.decoder.width(), self.decoder.height());
            let scaler = ffmpeg_next::software::scaling::Context::get(
                self.decoder.format(), w, h,
                ffmpeg_next::format::Pixel::RGBA, out_w, out_h,
                ffmpeg_next::software::scaling::Flags::BILINEAR,
            ).map_err(|e| Error::Decode(e.to_string()))?;
            self.scaler = Some((out_h, scaler));
            // A size change makes the held frame useless.
            self.held = None;
            self.held_fresh = false;
        }
        let ts = self.stream_time(at);
        // The held frame is the one right BEFORE whatever comes next — unless we just seeked, in
        // which case nothing is known about where the decoder stands.
        let mut prev = match (&self.held, self.held_fresh) {
            (Some(h), true) => h.pts,
            _ => i64::MIN,
        };

        let mut decoded = ffmpeg_next::util::frame::video::Video::empty();
        // Borrow the fields SEPARATELY instead of collecting packets into a `Vec`.
        //
        // An earlier version collected `input.packets()` into a `Vec` to dodge the borrow — which
        // means **reading the whole file into memory** before decoding one frame. Measured on a
        // 1080×1920 clip: the frame at second 10 took 931 ms, growing linearly with the mark, which
        // is to say the seek had stopped doing anything at all.
        let Self { input, decoder, stream: want, scaler, held, held_fresh, .. } = self;
        let (_, scaler) = scaler.as_mut().expect("vừa dựng xong");
        for (stream, packet) in input.packets() {
            if stream.index() != *want {
                continue;
            }
            if decoder.send_packet(&packet).is_err() {
                continue;
            }
            while decoder.receive_frame(&mut decoded).is_ok() {
                let pts = decoded.pts().unwrap_or(0);
                // The first frame AT or past the mark is the one wanted. Stop there — decoding on
                // spends work on frames nobody will look at.
                if pts >= ts {
                    let mut rgba = ffmpeg_next::util::frame::video::Video::empty();
                    scaler.run(&decoded, &mut rgba).map_err(|e| Error::Decode(e.to_string()))?;
                    *held = Some(Held { pts, prev, rgba: pack(&rgba, out_w, out_h) });
                    *held_fresh = true;
                    return Ok(());
                }
                prev = pts;
            }
        }
        // End of file before the mark: hold the last frame. A scene outlasting its clip is ordinary,
        // and failing the whole build over the tail of one clip is not.
        if held.is_some() {
            return Ok(());
        }
        Err(Error::Decode(format!("không có khung nào ở mốc {at}s")))
    }
}

/// Drop the padding at the end of each row.
///
/// Decoders align rows to a multiple for speed, so `stride` is usually LARGER than `width × 4`.
/// Copying the padding along with the pixels shears the picture progressively down the frame — a
/// classic fault, obvious on sight, but only when the size does not happen to divide evenly.
fn pack(frame: &ffmpeg_next::util::frame::video::Video, w: u32, h: u32) -> Rgba {
    let stride = frame.stride(0);
    let row = (w as usize) * 4;
    let data = frame.data(0);
    let mut pixels = Vec::with_capacity(row * h as usize);
    for y in 0..h as usize {
        let start = y * stride;
        pixels.extend_from_slice(&data[start..start + row]);
    }
    Rgba { width: w, height: h, pixels }
}
