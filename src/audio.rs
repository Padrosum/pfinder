use rodio::{OutputStream, OutputStreamHandle, Sink, Source};
use std::time::Duration;

pub struct Audio {
    _stream: OutputStream,
    handle: OutputStreamHandle,
}

impl Audio {
    pub fn new() -> Option<Self> {
        match OutputStream::try_default() {
            Ok((stream, handle)) => Some(Audio { _stream: stream, handle }),
            Err(_) => None,
        }
    }

    pub fn shoot(&self)       { self.tone(180.0, 90,  WaveKind::Square, 0.25); }
    pub fn damage(&self)      { self.tone(880.0, 130, WaveKind::Square, 0.30); }
    pub fn enemy_alert(&self) { self.tone(440.0, 80,  WaveKind::Square, 0.15); }

    pub fn pickup(&self) {
        for (f, d) in [(523.0, 60u64), (659.0, 60), (784.0, 100)] {
            self.tone(f, d, WaveKind::Sine, 0.20);
        }
    }

    pub fn gameover(&self) {
        for (f, d) in [(330.0, 200u64), (220.0, 300), (110.0, 500)] {
            self.tone(f, d, WaveKind::Square, 0.30);
        }
    }

    pub fn victory(&self) {
        for (f, d) in [(523.0, 100u64), (659.0, 100), (784.0, 100), (1047.0, 300)] {
            self.tone(f, d, WaveKind::Sine, 0.25);
        }
    }

    fn tone(&self, freq: f32, ms: u64, wave: WaveKind, vol: f32) {
        if let Ok(sink) = Sink::try_new(&self.handle) {
            sink.append(ToneSource::new(freq, ms, wave, vol));
            sink.detach();
        }
    }
}

#[derive(Clone, Copy)]
enum WaveKind { Sine, Square }

struct ToneSource {
    freq: f32,
    total: u64,
    pos: u64,
    wave: WaveKind,
    vol: f32,
}

impl ToneSource {
    const RATE: u32 = 44100;

    fn new(freq: f32, ms: u64, wave: WaveKind, vol: f32) -> Self {
        ToneSource { freq, total: Self::RATE as u64 * ms / 1000, pos: 0, wave, vol }
    }
}

impl Iterator for ToneSource {
    type Item = f32;
    fn next(&mut self) -> Option<f32> {
        if self.pos >= self.total { return None; }
        let t = self.pos as f32 / Self::RATE as f32;
        let env = {
            let fade = (Self::RATE as f32 * 0.005) as u64;
            let fi = if self.pos < fade { self.pos as f32 / fade as f32 } else { 1.0 };
            let fo = if self.pos > self.total.saturating_sub(fade) {
                (self.total - self.pos) as f32 / fade as f32
            } else { 1.0 };
            fi * fo
        };
        let raw = match self.wave {
            WaveKind::Sine   => (2.0 * std::f32::consts::PI * self.freq * t).sin(),
            WaveKind::Square => if (self.freq * t) % 1.0 < 0.5 { 1.0 } else { -1.0 },
        };
        self.pos += 1;
        Some(raw * self.vol * env)
    }
}

impl Source for ToneSource {
    fn current_frame_len(&self) -> Option<usize> { None }
    fn channels(&self) -> u16 { 1 }
    fn sample_rate(&self) -> u32 { Self::RATE }
    fn total_duration(&self) -> Option<Duration> {
        Some(Duration::from_millis(self.total * 1000 / Self::RATE as u64))
    }
}
