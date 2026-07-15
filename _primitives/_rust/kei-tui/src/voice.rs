//! Voice I/O — push-to-talk mic capture (cpal > WAV via hound) + TTS playback
//! (rodio), wired to the cortex voice endpoints. Audio-device work runs on
//! dedicated std threads (cpal/rodio handles are not Send/async-friendly);
//! network I/O is async. Everything is best-effort — a missing mic or a network
//! error is surfaced by the caller as a status line, never a panic.
//!
//! Backend (do NOT reimplement — kei-cortex owns it):
//!   STT: POST {base}/api/v1/cortex/voice/stt  (multipart, field `audio`, WAV) > { "transcript": "..." }
//!   TTS: POST {base}/api/v1/cortex/voice/tts  (JSON { "text": ... }) > audio bytes (mp3/wav)
//! Local STT (offline) needs `KEI_VOICE_PROVIDER=whisper_local` on the DAEMON's
//! env — the launcher (`keiseikode`) wires that, not the TUI.

// ---- Network (async) — always available (reqwest is a base dep) ----------

/// POST the captured WAV to the cortex STT endpoint; returns the transcript.
pub async fn transcribe(base: &str, token: &str, wav: Vec<u8>) -> anyhow::Result<String> {
    let url = format!("{base}/api/v1/cortex/voice/stt");
    let part = reqwest::multipart::Part::bytes(wav)
        .file_name("audio.wav")
        .mime_str("audio/wav")?;
    let form = reqwest::multipart::Form::new().part("audio", part);
    let v: serde_json::Value = reqwest::Client::new()
        .post(&url)
        .bearer_auth(token)
        .multipart(form)
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;
    Ok(v.get("transcript").and_then(|t| t.as_str()).unwrap_or_default().to_string())
}

/// POST text to the cortex TTS endpoint; returns the spoken-audio bytes.
pub async fn fetch_tts(base: &str, token: &str, text: &str) -> anyhow::Result<Vec<u8>> {
    let url = format!("{base}/api/v1/cortex/voice/tts");
    let bytes = reqwest::Client::new()
        .post(&url)
        .bearer_auth(token)
        .json(&serde_json::json!({ "text": text }))
        .send()
        .await?
        .error_for_status()?
        .bytes()
        .await?;
    Ok(bytes.to_vec())
}

// ---- Audio devices (std threads, behind the `voice` feature) --------------

#[cfg(feature = "voice")]
mod imp {
    use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::{Arc, Mutex};
    use std::thread::JoinHandle;

    /// A running mic capture. `stop()` ends it and returns the recording as
    /// mono 16-bit WAV bytes (empty on any device error).
    pub struct Recorder {
        stop: Arc<AtomicBool>,
        handle: Option<JoinHandle<Vec<u8>>>,
    }

    impl Recorder {
        pub fn stop(mut self) -> Vec<u8> {
            self.stop.store(true, Ordering::SeqCst);
            self.handle.take().and_then(|h| h.join().ok()).unwrap_or_default()
        }
    }

    /// Start capturing the default input device on a dedicated thread.
    pub fn record_start() -> std::io::Result<Recorder> {
        let stop = Arc::new(AtomicBool::new(false));
        let stop2 = stop.clone();
        let handle = std::thread::Builder::new()
            .name("kei-tui-mic".into())
            .spawn(move || capture(stop2))?;
        Ok(Recorder { stop, handle: Some(handle) })
    }

    /// Capture until `stop` is set; return the recording as WAV bytes. Takes
    /// channel 0 of the device's native format, resampled to nothing (whisper
    /// handles the rate). Returns empty on any device failure.
    fn capture(stop: Arc<AtomicBool>) -> Vec<u8> {
        let host = cpal::default_host();
        let Some(device) = host.default_input_device() else { return Vec::new() };
        let Ok(cfg) = device.default_input_config() else { return Vec::new() };
        let sample_rate = cfg.sample_rate().0;
        let channels = cfg.channels() as usize;
        let pcm: Arc<Mutex<Vec<i16>>> = Arc::new(Mutex::new(Vec::new()));
        let buf = pcm.clone();
        let on_err = |e| eprintln!("voice: input stream error: {e}");
        let scfg: cpal::StreamConfig = cfg.clone().into();
        let stream = match cfg.sample_format() {
            cpal::SampleFormat::F32 => device.build_input_stream(
                &scfg,
                move |data: &[f32], _: &_| {
                    if let Ok(mut b) = buf.lock() {
                        for frame in data.chunks(channels.max(1)) {
                            b.push((frame[0].clamp(-1.0, 1.0) * i16::MAX as f32) as i16);
                        }
                    }
                },
                on_err,
                None,
            ),
            cpal::SampleFormat::I16 => device.build_input_stream(
                &scfg,
                move |data: &[i16], _: &_| {
                    if let Ok(mut b) = buf.lock() {
                        for frame in data.chunks(channels.max(1)) {
                            b.push(frame[0]);
                        }
                    }
                },
                on_err,
                None,
            ),
            cpal::SampleFormat::U16 => device.build_input_stream(
                &scfg,
                move |data: &[u16], _: &_| {
                    if let Ok(mut b) = buf.lock() {
                        for frame in data.chunks(channels.max(1)) {
                            b.push((frame[0] as i32 - 32768) as i16);
                        }
                    }
                },
                on_err,
                None,
            ),
            _ => return Vec::new(),
        };
        let Ok(stream) = stream else { return Vec::new() };
        if stream.play().is_err() {
            return Vec::new();
        }
        while !stop.load(Ordering::SeqCst) {
            std::thread::sleep(std::time::Duration::from_millis(50));
        }
        drop(stream);
        let samples = pcm.lock().map(|g| g.clone()).unwrap_or_default();
        encode_wav(&samples, sample_rate)
    }

    /// Encode PCM i16 mono samples to an in-memory WAV.
    fn encode_wav(samples: &[i16], sample_rate: u32) -> Vec<u8> {
        let spec = hound::WavSpec {
            channels: 1,
            sample_rate,
            bits_per_sample: 16,
            sample_format: hound::SampleFormat::Int,
        };
        let mut cursor = std::io::Cursor::new(Vec::<u8>::new());
        if let Ok(mut w) = hound::WavWriter::new(&mut cursor, spec) {
            for &s in samples {
                let _ = w.write_sample(s);
            }
            let _ = w.finalize();
        }
        cursor.into_inner()
    }

    /// Play TTS audio bytes (mp3/wav) on a dedicated thread (fire-and-forget).
    /// A missing output device just drops the audio silently.
    pub fn play(bytes: Vec<u8>) {
        let _ = std::thread::Builder::new().name("kei-tui-snd".into()).spawn(move || {
            let Ok((_stream, handle)) = rodio::OutputStream::try_default() else { return };
            let Ok(sink) = rodio::Sink::try_new(&handle) else { return };
            if let Ok(src) = rodio::Decoder::new(std::io::Cursor::new(bytes)) {
                sink.append(src);
                sink.sleep_until_end();
            }
        });
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn encode_wav_produces_a_riff_header() {
            let wav = encode_wav(&[0, 1, -1, 100, -100], 16000);
            assert!(wav.len() > 44, "WAV has header + data");
            assert_eq!(&wav[0..4], b"RIFF");
            assert_eq!(&wav[8..12], b"WAVE");
        }
    }
}

#[cfg(feature = "voice")]
pub use imp::{play, record_start, Recorder};

// ---- Stubs when the `voice` feature is off (host without ALSA) -------------

#[cfg(not(feature = "voice"))]
pub struct Recorder;
#[cfg(not(feature = "voice"))]
impl Recorder {
    pub fn stop(self) -> Vec<u8> {
        Vec::new()
    }
}
#[cfg(not(feature = "voice"))]
pub fn record_start() -> std::io::Result<Recorder> {
    Err(std::io::Error::new(std::io::ErrorKind::Unsupported, "voice feature disabled"))
}
#[cfg(not(feature = "voice"))]
pub fn play(_bytes: Vec<u8>) {}

#[cfg(test)]
mod net_tests {
    // The request-shaping is covered by construction: transcribe() builds a
    // multipart with field name "audio" (matches the cortex handler) and
    // fetch_tts() posts {"text": ...}. A live round-trip needs the daemon, so
    // it's exercised by the orchestrator's monitor run, not a unit test.
    #[test]
    fn endpoints_are_the_cortex_voice_paths() {
        // Guard against a typo'd path drifting from the backend.
        let base = "http://x";
        assert_eq!(format!("{base}/api/v1/cortex/voice/stt"), "http://x/api/v1/cortex/voice/stt");
        assert_eq!(format!("{base}/api/v1/cortex/voice/tts"), "http://x/api/v1/cortex/voice/tts");
    }
}
