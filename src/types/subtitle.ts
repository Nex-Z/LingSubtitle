export type SubtitleSegmentState = "streaming" | "stablePreview" | "final";
export type TranslationStatus = "idle" | "streaming" | "completed" | "failed";

export interface SubtitleSegmentPayload {
  segmentId: number;
  sessionId: number;
  timestamp: string;
  originalText: string;
  translatedText?: string | null;
  translatedDraftText?: string | null;
  state: SubtitleSegmentState;
  isFinal: boolean;
  revision: number;
  translationError?: boolean;
  translationStatus?: TranslationStatus;
  translationStartedAt?: number | null;
  translationFinishedAt?: number | null;
}

export interface SubtitleTranslationPayload {
  segmentId: number;
  revision: number;
  translatedText: string;
}

export interface SubtitleTranslationStartedPayload {
  segmentId: number;
  revision: number;
}

export interface SubtitleTranslationDeltaPayload {
  segmentId: number;
  revision: number;
  deltaText: string;
  accumulatedText: string;
}

export interface SubtitleTranslationFailedPayload {
  segmentId: number;
  revision: number;
  message: string;
  errorKind?: string | null;
  provider?: string | null;
  resolvedUrl?: string | null;
}

export interface SubtitleErrorPayload {
  scope: "asr" | "translation" | "capture" | "system";
  message: string;
  segmentId?: number | null;
  errorKind?: string | null;
  provider?: string | null;
  resolvedUrl?: string | null;
}
