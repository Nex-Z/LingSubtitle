use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{SampleFormat, Stream};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use tokio::sync::mpsc;

const TARGET_SAMPLE_RATE: u32 = 16000;

pub struct AudioCapture {
    is_running: Arc<AtomicBool>,
    stream: Option<Stream>,
}

// cpal::Stream is not Send, but we manage it carefully
unsafe impl Send for AudioCapture {}
unsafe impl Sync for AudioCapture {}

impl AudioCapture {
    pub fn new() -> Self {
        Self {
            is_running: Arc::new(AtomicBool::new(false)),
            stream: None,
        }
    }

    /// Start capturing system audio and sending raw PCM (16-bit signed LE, mono, 16kHz)
    /// chunks every ~50ms via the provided channel.
    pub fn start(
        &mut self,
        audio_sender: mpsc::UnboundedSender<Vec<u8>>,
    ) -> Result<u32, String> {
        if self.is_running.load(Ordering::SeqCst) {
            return Err("Audio capture is already running".to_string());
        }

        let host = cpal::default_host();
        let device = host
            .default_output_device()
            .ok_or("No output device available")?;

        let config = device
            .default_output_config()
            .map_err(|e| format!("Failed to get default output config: {}", e))?;

        let device_sample_rate = config.sample_rate().0;
        let channels = config.channels() as usize;
        let sample_format = config.sample_format();

        println!(
            "[Audio] Device: rate={}Hz, channels={}, format={:?}",
            device_sample_rate, channels, sample_format
        );

        // Buffer to accumulate raw interleaved f32 samples
        let buffer: Arc<Mutex<Vec<f32>>> =
            Arc::new(Mutex::new(Vec::with_capacity(device_sample_rate as usize / 10)));
        let buffer_clone = buffer.clone();
        let is_running = self.is_running.clone();

        is_running.store(true, Ordering::SeqCst);
        let is_running_clone = is_running.clone();

        // Flush thread: every 50ms, mix to mono, resample to 16kHz, convert to PCM16
        let flush_buffer = buffer.clone();
        let flush_running = is_running.clone();
        std::thread::spawn(move || {
            let interval = std::time::Duration::from_millis(50);
            while flush_running.load(Ordering::SeqCst) {
                std::thread::sleep(interval);
                if !flush_running.load(Ordering::SeqCst) {
                    break;
                }

                let raw_samples: Vec<f32> = {
                    let mut buf = flush_buffer.lock().unwrap();
                    if buf.is_empty() {
                        continue;
                    }
                    std::mem::take(&mut *buf)
                };

                // Step 1: Mix down to mono (average all channels per frame)
                let mono_samples = mix_to_mono(&raw_samples, channels);

                // Step 2: Resample from device rate to 16kHz
                let resampled = resample(&mono_samples, device_sample_rate, TARGET_SAMPLE_RATE);

                // Step 3: Convert f32 → i16 → little-endian bytes (raw PCM)
                let pcm_bytes = f32_to_pcm16_bytes(&resampled);
                if !pcm_bytes.is_empty() {
                    let _ = audio_sender.send(pcm_bytes);
                }
            }
        });

        let err_fn = move |err: cpal::StreamError| {
            eprintln!("Audio stream error: {}", err);
        };

        let stream = match sample_format {
            SampleFormat::F32 => device.build_input_stream(
                &config.into(),
                move |data: &[f32], _: &cpal::InputCallbackInfo| {
                    if is_running_clone.load(Ordering::SeqCst) {
                        let mut buf = buffer_clone.lock().unwrap();
                        buf.extend_from_slice(data);
                    }
                },
                err_fn,
                None,
            ),
            SampleFormat::I16 => device.build_input_stream(
                &config.into(),
                move |data: &[i16], _: &cpal::InputCallbackInfo| {
                    if is_running_clone.load(Ordering::SeqCst) {
                        let float_data: Vec<f32> =
                            data.iter().map(|&s| s as f32 / i16::MAX as f32).collect();
                        let mut buf = buffer_clone.lock().unwrap();
                        buf.extend_from_slice(&float_data);
                    }
                },
                err_fn,
                None,
            ),
            SampleFormat::U16 => device.build_input_stream(
                &config.into(),
                move |data: &[u16], _: &cpal::InputCallbackInfo| {
                    if is_running_clone.load(Ordering::SeqCst) {
                        let float_data: Vec<f32> = data
                            .iter()
                            .map(|&s| (s as f32 / u16::MAX as f32) * 2.0 - 1.0)
                            .collect();
                        let mut buf = buffer_clone.lock().unwrap();
                        buf.extend_from_slice(&float_data);
                    }
                },
                err_fn,
                None,
            ),
            _ => return Err(format!("Unsupported sample format: {:?}", sample_format)),
        }
        .map_err(|e| format!("Failed to build input stream: {}", e))?;

        stream
            .play()
            .map_err(|e| format!("Failed to start stream: {}", e))?;

        self.stream = Some(stream);
        // Always return 16000 since we resample to 16kHz
        Ok(TARGET_SAMPLE_RATE)
    }

    pub fn stop(&mut self) {
        self.is_running.store(false, Ordering::SeqCst);
        self.stream = None;
    }

    pub fn is_running(&self) -> bool {
        self.is_running.load(Ordering::SeqCst)
    }
}

/// Mix interleaved multi-channel f32 samples down to mono by averaging channels per frame
fn mix_to_mono(samples: &[f32], channels: usize) -> Vec<f32> {
    if channels <= 1 {
        return samples.to_vec();
    }

    let frame_count = samples.len() / channels;
    let mut mono = Vec::with_capacity(frame_count);

    for frame in 0..frame_count {
        let offset = frame * channels;
        let mut sum = 0.0f32;
        for ch in 0..channels {
            sum += samples[offset + ch];
        }
        mono.push(sum / channels as f32);
    }
    mono
}

/// Simple linear-interpolation resampler from `from_rate` to `to_rate`
fn resample(samples: &[f32], from_rate: u32, to_rate: u32) -> Vec<f32> {
    if from_rate == to_rate || samples.is_empty() {
        return samples.to_vec();
    }

    let ratio = from_rate as f64 / to_rate as f64;
    let output_len = (samples.len() as f64 / ratio).ceil() as usize;
    let mut output = Vec::with_capacity(output_len);

    for i in 0..output_len {
        let src_pos = i as f64 * ratio;
        let idx = src_pos as usize;
        let frac = src_pos - idx as f64;

        if idx + 1 < samples.len() {
            // Linear interpolation between two adjacent samples
            let sample =
                samples[idx] as f64 * (1.0 - frac) + samples[idx + 1] as f64 * frac;
            output.push(sample as f32);
        } else if idx < samples.len() {
            output.push(samples[idx]);
        }
    }

    output
}

/// Convert f32 samples (range -1.0 to 1.0) to 16-bit signed PCM little-endian bytes
fn f32_to_pcm16_bytes(samples: &[f32]) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(samples.len() * 2);
    for &sample in samples {
        let clamped = sample.max(-1.0).min(1.0);
        let int_sample = (clamped * i16::MAX as f32) as i16;
        bytes.extend_from_slice(&int_sample.to_le_bytes());
    }
    bytes
}
