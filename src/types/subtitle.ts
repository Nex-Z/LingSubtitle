export type SubtitleSegmentState = "streaming" | "final";
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

export interface SubtitleErrorPayload {
  scope: "asr" | "translation" | "capture" | "system";
  message: string;
  segmentId?: number | null;
  errorKind?: string | null;
  provider?: string | null;
  resolvedUrl?: string | null;
}
