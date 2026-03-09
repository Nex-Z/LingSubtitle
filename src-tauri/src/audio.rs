use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{SampleFormat, Stream};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use tokio::sync::mpsc;

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

    /// Start capturing system audio and sending raw PCM (16-bit signed LE) chunks
    /// every ~100ms via the provided channel.
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

        let sample_rate = config.sample_rate().0;
        let channels = config.channels();
        let sample_format = config.sample_format();

        // Buffer to accumulate samples (~100ms worth)
        let samples_per_chunk = (sample_rate as usize / 10) * channels as usize;
        let buffer: Arc<Mutex<Vec<f32>>> =
            Arc::new(Mutex::new(Vec::with_capacity(samples_per_chunk)));
        let buffer_clone = buffer.clone();
        let is_running = self.is_running.clone();

        is_running.store(true, Ordering::SeqCst);
        let is_running_clone = is_running.clone();

        // Flush thread: every 100ms, convert buffered f32 samples to 16-bit PCM bytes
        let flush_buffer = buffer.clone();
        let flush_running = is_running.clone();
        std::thread::spawn(move || {
            let interval = std::time::Duration::from_millis(100);
            while flush_running.load(Ordering::SeqCst) {
                std::thread::sleep(interval);
                if !flush_running.load(Ordering::SeqCst) {
                    break;
                }
                let samples: Vec<f32> = {
                    let mut buf = flush_buffer.lock().unwrap();
                    if buf.is_empty() {
                        continue;
                    }
                    let data = buf.clone();
                    buf.clear();
                    data
                };
                // Convert f32 → i16 → little-endian bytes (raw PCM)
                let pcm_bytes = f32_to_pcm16_bytes(&samples);
                let _ = audio_sender.send(pcm_bytes);
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
        Ok(sample_rate)
    }

    pub fn stop(&mut self) {
        self.is_running.store(false, Ordering::SeqCst);
        self.stream = None;
    }

    pub fn is_running(&self) -> bool {
        self.is_running.load(Ordering::SeqCst)
    }
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
