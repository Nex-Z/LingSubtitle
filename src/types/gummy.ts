export interface AsrConfig {
  base_url: string;
  api_key: string;
  model: string;
  sample_rate: number;
  language: string;
  vad_silence_ms: number;
  vocabulary_id: string;
}

export interface TranslationConfig {
  enabled: boolean;
  target_language: string;
}

export interface SaveConfig {
  auto_save: boolean;
  save_path: string;
}

export interface CaptureConfig {
  source: string;
  app_pid?: number | null;
  app_name?: string;
}

export interface AppConfig {
  asr: AsrConfig;
  translation: TranslationConfig;
  save: SaveConfig;
  capture: CaptureConfig;
  filter_fillers: boolean;
}

export interface GummyLanguageOption {
  code: string;
  label: string;
}

export interface GummyDefaults {
  base_url: string;
  model: string;
  sample_rate: number;
  vad_silence_ms: number;
  source_language: string;
  target_language: string;
}

export interface GummyCapabilities {
  sourceLanguages: GummyLanguageOption[];
  targetLanguagesBySource: Record<string, GummyLanguageOption[]>;
  defaults: GummyDefaults;
}

export interface GummyConfigCheckResult {
  ready: boolean;
  message: string;
}

export interface GummyConnectivityResult {
  ok: boolean;
  provider: string;
  resolvedUrl: string;
  model: string;
  errorKind?: string | null;
  message: string;
}
