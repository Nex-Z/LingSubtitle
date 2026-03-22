use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{SampleFormat, Stream};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::sync::mpsc;

#[cfg(target_os = "windows")]
use std::collections::HashSet;
#[cfg(target_os = "windows")]
use std::io::Cursor;
#[cfg(target_os = "windows")]
use std::path::Path;
#[cfg(target_os = "windows")]
use base64::Engine;
#[cfg(target_os = "windows")]
use image::codecs::png::PngEncoder;
#[cfg(target_os = "windows")]
use image::{ColorType, ImageEncoder};
#[cfg(target_os = "windows")]
use windows::core::{implement, Interface, PCWSTR, HRESULT};
#[cfg(target_os = "windows")]
use windows::Win32::Foundation::CloseHandle;
#[cfg(target_os = "windows")]
use windows::Win32::Graphics::Gdi::{
    CreateCompatibleDC, CreateDIBSection, DeleteDC, DeleteObject, SelectObject, BITMAPINFO,
    BITMAPINFOHEADER, BI_RGB, DIB_RGB_COLORS, HGDIOBJ,
};
#[cfg(target_os = "windows")]
use windows::Win32::Media::Audio::*;
#[cfg(target_os = "windows")]
use windows::Win32::Media::Audio::IActivateAudioInterfaceCompletionHandler_Impl;
#[cfg(target_os = "windows")]
use windows::Win32::Media::KernelStreaming::KSDATAFORMAT_SUBTYPE_PCM;
#[cfg(target_os = "windows")]
use windows::Win32::Media::Multimedia::KSDATAFORMAT_SUBTYPE_IEEE_FLOAT;
#[cfg(target_os = "windows")]
use windows::Win32::System::Com::{
    CoCreateInstance, CoInitializeEx, CoTaskMemFree, CoUninitialize, CLSCTX_ALL, COINIT_MULTITHREADED,
};
#[cfg(target_os = "windows")]
use windows::Win32::System::Com::StructuredStorage::InitPropVariantFromBuffer;
#[cfg(target_os = "windows")]
use windows::Win32::System::ProcessStatus::K32GetProcessImageFileNameW;
#[cfg(target_os = "windows")]
use windows::Win32::System::Threading::{OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION};
#[cfg(target_os = "windows")]
use windows::Win32::UI::Shell::{SHGetFileInfoW, SHFILEINFOW, SHGFI_ICON, SHGFI_LARGEICON};
#[cfg(target_os = "windows")]
use windows::Win32::UI::WindowsAndMessaging::{DestroyIcon, DrawIconEx, DI_NORMAL};

const TARGET_SAMPLE_RATE: u32 = 16000;
// Simple noise gate to reduce low-level background noise.
// Tune this if quiet speech gets clipped (lower) or noise leaks (higher).
const NOISE_GATE_THRESHOLD: f32 = 0.02; // ~ -34 dBFS

pub struct AudioCapture {
    is_running: Arc<AtomicBool>,
    stream: Option<Stream>,
    process_thread: Option<std::thread::JoinHandle<()>>,
}

// cpal::Stream is not Send, but we manage it carefully
unsafe impl Send for AudioCapture {}
unsafe impl Sync for AudioCapture {}

impl AudioCapture {
    pub fn new() -> Self {
        Self {
            is_running: Arc::new(AtomicBool::new(false)),
            stream: None,
            process_thread: None,
        }
    }

    /// Start capturing system audio (or a specific app on Windows) and sending raw PCM (16-bit signed LE, mono, 16kHz)
    /// chunks every ~50ms via the provided channel.
    pub fn start(
        &mut self,
        audio_sender: mpsc::UnboundedSender<Vec<u8>>,
        process_id: Option<u32>,
    ) -> Result<u32, String> {
        if self.is_running.load(Ordering::SeqCst) {
            return Err("Audio capture is already running".to_string());
        }

        if let Some(pid) = process_id {
            #[cfg(target_os = "windows")]
            {
                return self.start_process_loopback(audio_sender, pid);
            }
            #[cfg(not(target_os = "windows"))]
            {
                let _ = audio_sender;
                return Err("Process audio capture is only supported on Windows".to_string());
            }
        }

        self.start_system(audio_sender)
    }

    fn start_system(&mut self, audio_sender: mpsc::UnboundedSender<Vec<u8>>) -> Result<u32, String> {
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

                // Step 1.5: Apply simple noise gate on the mono signal
                let gated = apply_noise_gate(&mono_samples, NOISE_GATE_THRESHOLD);

                // Step 2: Resample from device rate to 16kHz
                let resampled = resample(&gated, device_sample_rate, TARGET_SAMPLE_RATE);

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
        if let Some(handle) = self.process_thread.take() {
            let _ = handle.join();
        }
    }

    pub fn is_running(&self) -> bool {
        self.is_running.load(Ordering::SeqCst)
    }
}

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AudioApp {
    pub pid: u32,
    pub name: String,
    pub icon_data_url: Option<String>,
}

#[cfg(target_os = "windows")]
pub fn list_audio_apps() -> Result<Vec<AudioApp>, String> {
    unsafe {
        let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
        let enumerator: IMMDeviceEnumerator =
            CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL)
                .map_err(|e| format!("Failed to create MMDeviceEnumerator: {}", e))?;
        let device = enumerator
            .GetDefaultAudioEndpoint(eRender, eConsole)
            .map_err(|e| format!("Failed to get default audio endpoint: {}", e))?;
        let manager: IAudioSessionManager2 = device
            .Activate(CLSCTX_ALL, None)
            .map_err(|e| format!("Failed to activate AudioSessionManager2: {}", e))?;
        let session_enum = manager
            .GetSessionEnumerator()
            .map_err(|e| format!("Failed to get session enumerator: {}", e))?;
        let count = session_enum
            .GetCount()
            .map_err(|e| format!("Failed to get session count: {}", e))?;

        let mut seen = HashSet::new();
        let mut apps = Vec::new();

        for i in 0..count {
            let control = session_enum
                .GetSession(i)
                .map_err(|e| format!("Failed to get session: {}", e))?;
            let control2: IAudioSessionControl2 = control
                .cast()
                .map_err(|e| format!("Failed to cast to IAudioSessionControl2: {}", e))?;
            let pid = control2
                .GetProcessId()
                .map_err(|e| format!("Failed to get process id: {}", e))?;
            if pid == 0 || !seen.insert(pid) {
                continue;
            }

            if let Some(path) = process_path_from_pid(pid) {
                let name = process_name_from_path(&path);
                if name.eq_ignore_ascii_case("audiodg.exe") {
                    continue;
                }
                apps.push(AudioApp {
                    pid,
                    name,
                    icon_data_url: process_icon_data_url(&path),
                });
            }
        }

        apps.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
        CoUninitialize();
        Ok(apps)
    }
}

#[cfg(not(target_os = "windows"))]
pub fn list_audio_apps() -> Result<Vec<AudioApp>, String> {
    Ok(Vec::new())
}

#[cfg(target_os = "windows")]
fn process_path_from_pid(pid: u32) -> Option<String> {
    unsafe {
        let handle = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid).ok()?;
        let mut buf = vec![0u16; 260];
        let len = K32GetProcessImageFileNameW(handle, &mut buf) as usize;
        let _ = CloseHandle(handle);
        if len == 0 {
            return None;
        }
        Some(String::from_utf16_lossy(&buf[..len]))
    }
}

#[cfg(target_os = "windows")]
fn process_name_from_path(path: &str) -> String {
    Path::new(path)
        .file_name()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_else(|| path.to_string())
}

#[cfg(target_os = "windows")]
fn process_icon_data_url(path: &str) -> Option<String> {
    unsafe {
        let wide_path: Vec<u16> = Path::new(path)
            .as_os_str()
            .to_string_lossy()
            .encode_utf16()
            .chain(std::iter::once(0))
            .collect();

        let mut file_info = SHFILEINFOW::default();
        let result = SHGetFileInfoW(
            PCWSTR(wide_path.as_ptr()),
            windows::Win32::Storage::FileSystem::FILE_FLAGS_AND_ATTRIBUTES(0),
            Some(&mut file_info),
            std::mem::size_of::<SHFILEINFOW>() as u32,
            SHGFI_ICON | SHGFI_LARGEICON,
        );
        if result == 0 || file_info.hIcon.is_invalid() {
            return None;
        }

        let png_bytes = render_icon_to_png(file_info.hIcon, 32, 32);
        let _ = DestroyIcon(file_info.hIcon);
        png_bytes.map(|bytes| {
            format!(
                "data:image/png;base64,{}",
                base64::engine::general_purpose::STANDARD.encode(bytes)
            )
        })
    }
}

#[cfg(target_os = "windows")]
fn render_icon_to_png(
    icon: windows::Win32::UI::WindowsAndMessaging::HICON,
    width: i32,
    height: i32,
) -> Option<Vec<u8>> {
    unsafe {
        let dc = CreateCompatibleDC(None);
        if dc.is_invalid() {
            return None;
        }

        let mut pixels: *mut std::ffi::c_void = std::ptr::null_mut();
        let mut bitmap_info = BITMAPINFO::default();
        bitmap_info.bmiHeader = BITMAPINFOHEADER {
            biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
            biWidth: width,
            biHeight: -height,
            biPlanes: 1,
            biBitCount: 32,
            biCompression: BI_RGB.0,
            ..Default::default()
        };

        let bitmap =
            CreateDIBSection(dc, &bitmap_info, DIB_RGB_COLORS, &mut pixels, None, 0).ok()?;
        if bitmap.is_invalid() || pixels.is_null() {
            let _ = DeleteDC(dc);
            return None;
        }

        let old_bitmap = SelectObject(dc, HGDIOBJ(bitmap.0));
        let draw_ok = DrawIconEx(dc, 0, 0, icon, width, height, 0, None, DI_NORMAL).is_ok();

        let result = if draw_ok {
            let pixel_len = (width * height * 4) as usize;
            let bgra = std::slice::from_raw_parts(pixels as *const u8, pixel_len);
            let mut rgba = Vec::with_capacity(pixel_len);
            for chunk in bgra.chunks_exact(4) {
                rgba.push(chunk[2]);
                rgba.push(chunk[1]);
                rgba.push(chunk[0]);
                rgba.push(chunk[3]);
            }

            let mut png = Vec::new();
            let mut cursor = Cursor::new(&mut png);
            PngEncoder::new(&mut cursor)
                .write_image(&rgba, width as u32, height as u32, ColorType::Rgba8.into())
                .ok()
                .map(|_| png)
        } else {
            None
        };

        let _ = SelectObject(dc, old_bitmap);
        let _ = DeleteObject(bitmap);
        let _ = DeleteDC(dc);
        result
    }
}

#[cfg(target_os = "windows")]
impl AudioCapture {
    fn start_process_loopback(
        &mut self,
        audio_sender: mpsc::UnboundedSender<Vec<u8>>,
        process_id: u32,
    ) -> Result<u32, String> {
        let is_running = self.is_running.clone();
        is_running.store(true, Ordering::SeqCst);

        let (ready_tx, ready_rx) = std::sync::mpsc::channel::<Result<u32, String>>();
        let handle = std::thread::spawn(move || {
            let result = run_process_loopback(process_id, audio_sender, is_running, ready_tx);
            if let Err(e) = result {
                eprintln!("Process loopback error: {}", e);
            }
        });

        let sample_rate = match ready_rx.recv_timeout(Duration::from_secs(5)) {
            Ok(Ok(rate)) => rate,
            Ok(Err(err)) => {
                let _ = handle.join();
                return Err(err);
            }
            Err(_) => {
                return Err("Process audio initialization timed out".to_string());
            }
        };

        self.process_thread = Some(handle);
        Ok(sample_rate)
    }
}

#[cfg(target_os = "windows")]
#[implement(IActivateAudioInterfaceCompletionHandler)]
struct ActivateHandler {
    sender: std::sync::mpsc::Sender<Result<IAudioClient, String>>,
}

#[cfg(target_os = "windows")]
impl IActivateAudioInterfaceCompletionHandler_Impl for ActivateHandler {
    fn ActivateCompleted(
        &self,
        operation: Option<&IActivateAudioInterfaceAsyncOperation>,
    ) -> windows::core::Result<()> {
        unsafe {
            let op = operation.ok_or_else(windows::core::Error::from_win32)?;
            let mut hr = HRESULT(0);
            let mut unk = None;
            op.GetActivateResult(&mut hr, &mut unk)?;
            if hr.is_ok() {
                let client: IAudioClient = unk
                    .ok_or_else(windows::core::Error::from_win32)?
                    .cast()?;
                let _ = self.sender.send(Ok(client));
            } else {
                let _ = self.sender.send(Err(format!(
                    "ActivateAudioInterfaceAsync failed: {:#x}",
                    hr.0
                )));
            }
        }
        Ok(())
    }
}

#[cfg(target_os = "windows")]
fn activate_process_loopback(process_id: u32) -> Result<IAudioClient, String> {
    unsafe {
        let _ = CoInitializeEx(None, COINIT_MULTITHREADED);

        let (tx, rx) = std::sync::mpsc::channel::<Result<IAudioClient, String>>();
        let handler = ActivateHandler { sender: tx };
        let handler: IActivateAudioInterfaceCompletionHandler = handler.into();

        let mut params = AUDIOCLIENT_ACTIVATION_PARAMS::default();
        params.ActivationType = AUDIOCLIENT_ACTIVATION_TYPE_PROCESS_LOOPBACK;
        params.Anonymous.ProcessLoopbackParams.TargetProcessId = process_id;
        params.Anonymous.ProcessLoopbackParams.ProcessLoopbackMode =
            PROCESS_LOOPBACK_MODE_INCLUDE_TARGET_PROCESS_TREE;

        let param_bytes = std::slice::from_raw_parts(
            &params as *const AUDIOCLIENT_ACTIVATION_PARAMS as *const u8,
            std::mem::size_of::<AUDIOCLIENT_ACTIVATION_PARAMS>(),
        );

        let propvariant = InitPropVariantFromBuffer(
            param_bytes.as_ptr() as *const _,
            param_bytes.len() as u32,
        )
        .map_err(|e| format!("InitPropVariantFromBuffer failed: {}", e))?;

        let _async_op = ActivateAudioInterfaceAsync(
            PCWSTR::from_raw(VIRTUAL_AUDIO_DEVICE_PROCESS_LOOPBACK.as_ptr()),
            &IAudioClient::IID,
            Some(&propvariant),
            &handler,
        )
        .map_err(|e| format!("ActivateAudioInterfaceAsync failed: {}", e))?;

        match rx.recv_timeout(Duration::from_secs(5)) {
            Ok(Ok(client)) => Ok(client),
            Ok(Err(err)) => Err(err),
            Err(_) => Err("ActivateAudioInterfaceAsync timed out".to_string()),
        }
    }
}

#[cfg(target_os = "windows")]
fn run_process_loopback(
    process_id: u32,
    audio_sender: mpsc::UnboundedSender<Vec<u8>>,
    running: Arc<AtomicBool>,
    ready_tx: std::sync::mpsc::Sender<Result<u32, String>>,
) -> Result<(), String> {
    unsafe {
        let _ = CoInitializeEx(None, COINIT_MULTITHREADED);

        let audio_client = match activate_process_loopback(process_id) {
            Ok(client) => client,
            Err(err) => {
                let _ = ready_tx.send(Err(err.clone()));
                return Err(err);
            }
        };

        let mix_format_ptr = audio_client
            .GetMixFormat()
            .map_err(|e| {
                let msg = format!("GetMixFormat failed: {}", e);
                let _ = ready_tx.send(Err(msg.clone()));
                msg
            })?;
        if mix_format_ptr.is_null() {
            let msg = "GetMixFormat returned null".to_string();
            let _ = ready_tx.send(Err(msg.clone()));
            return Err(msg);
        }

        let mix_format = &*mix_format_ptr;
        let channels = mix_format.nChannels as usize;
        let device_sample_rate = mix_format.nSamplesPerSec;

        let (is_float, bits_per_sample) = match mix_format.wFormatTag as u32 {
            windows::Win32::Media::Multimedia::WAVE_FORMAT_IEEE_FLOAT => {
                (true, mix_format.wBitsPerSample)
            }
            windows::Win32::Media::Audio::WAVE_FORMAT_PCM => (false, mix_format.wBitsPerSample),
            windows::Win32::Media::KernelStreaming::WAVE_FORMAT_EXTENSIBLE => {
                let ext = &*(mix_format_ptr as *const WAVEFORMATEXTENSIBLE);
                let sub_format = std::ptr::addr_of!(ext.SubFormat).read_unaligned();
                if sub_format == KSDATAFORMAT_SUBTYPE_IEEE_FLOAT {
                    (true, ext.Format.wBitsPerSample)
                } else if sub_format == KSDATAFORMAT_SUBTYPE_PCM {
                    (false, ext.Format.wBitsPerSample)
                } else {
                    let msg = "Unsupported mix format subformat".to_string();
                    let _ = ready_tx.send(Err(msg.clone()));
                    return Err(msg);
                }
            }
            _ => {
                let msg = "Unsupported mix format tag".to_string();
                let _ = ready_tx.send(Err(msg.clone()));
                return Err(msg);
            }
        };

        let buffer_duration: i64 = 10_000_000 / 10; // 100ms in 100ns units
        audio_client
            .Initialize(
                AUDCLNT_SHAREMODE_SHARED,
                AUDCLNT_STREAMFLAGS_LOOPBACK,
                buffer_duration,
                0,
                mix_format_ptr,
                None,
            )
            .map_err(|e| {
                let msg = format!("AudioClient Initialize failed: {}", e);
                let _ = ready_tx.send(Err(msg.clone()));
                msg
            })?;

        let capture_client: IAudioCaptureClient = audio_client
            .GetService()
            .map_err(|e| {
                let msg = format!("GetService(IAudioCaptureClient) failed: {}", e);
                let _ = ready_tx.send(Err(msg.clone()));
                msg
            })?;

        audio_client
            .Start()
            .map_err(|e| {
                let msg = format!("AudioClient Start failed: {}", e);
                let _ = ready_tx.send(Err(msg.clone()));
                msg
            })?;

        CoTaskMemFree(Some(mix_format_ptr as _));

        let _ = ready_tx.send(Ok(TARGET_SAMPLE_RATE));

        while running.load(Ordering::SeqCst) {
            let mut packet_frames = capture_client
                .GetNextPacketSize()
                .map_err(|e| format!("GetNextPacketSize failed: {}", e))?;
            if packet_frames == 0 {
                std::thread::sleep(Duration::from_millis(10));
                continue;
            }

            while packet_frames > 0 {
                let mut data_ptr: *mut u8 = std::ptr::null_mut();
                let mut num_frames: u32 = 0;
                let mut flags: u32 = 0;
                capture_client
                    .GetBuffer(&mut data_ptr, &mut num_frames, &mut flags, None, None)
                    .map_err(|e| format!("GetBuffer failed: {}", e))?;

                let frame_count = num_frames as usize;
                let sample_count = frame_count * channels;

                let mut samples_f32: Vec<f32> = Vec::with_capacity(sample_count);
                if flags & AUDCLNT_BUFFERFLAGS_SILENT.0 as u32 != 0 {
                    samples_f32.resize(sample_count, 0.0);
                } else if is_float && bits_per_sample == 32 {
                    let float_slice = std::slice::from_raw_parts(
                        data_ptr as *const f32,
                        sample_count,
                    );
                    samples_f32.extend_from_slice(float_slice);
                } else if !is_float && bits_per_sample == 16 {
                    let int_slice = std::slice::from_raw_parts(
                        data_ptr as *const i16,
                        sample_count,
                    );
                    samples_f32.extend(
                        int_slice
                            .iter()
                            .map(|&s| s as f32 / i16::MAX as f32),
                    );
                } else {
                    capture_client
                        .ReleaseBuffer(num_frames)
                        .ok();
                    return Err(format!(
                        "Unsupported audio format (float={}, bits={})",
                        is_float, bits_per_sample
                    ));
                }

                capture_client
                    .ReleaseBuffer(num_frames)
                    .map_err(|e| format!("ReleaseBuffer failed: {}", e))?;

                let mono_samples = mix_to_mono(&samples_f32, channels);
                let gated = apply_noise_gate(&mono_samples, NOISE_GATE_THRESHOLD);
                let resampled = resample(&gated, device_sample_rate, TARGET_SAMPLE_RATE);
                let pcm_bytes = f32_to_pcm16_bytes(&resampled);
                if !pcm_bytes.is_empty() {
                    let _ = audio_sender.send(pcm_bytes);
                }

                packet_frames = capture_client
                    .GetNextPacketSize()
                    .map_err(|e| format!("GetNextPacketSize failed: {}", e))?;
            }
        }

        let _ = audio_client.Stop();
        CoUninitialize();
        Ok(())
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

fn apply_noise_gate(samples: &[f32], threshold: f32) -> Vec<f32> {
    if samples.is_empty() {
        return Vec::new();
    }

    // Compute RMS for the chunk
    let mut sum_sq = 0.0f32;
    for &s in samples {
        sum_sq += s * s;
    }
    let rms = (sum_sq / samples.len() as f32).sqrt();

    // Soft gate: attenuate below threshold instead of hard mute
    let gain = if rms <= 0.0 {
        0.0
    } else {
        (rms / threshold).clamp(0.0, 1.0)
    };

    if gain >= 1.0 {
        return samples.to_vec();
    }

    samples.iter().map(|s| s * gain).collect()
}
