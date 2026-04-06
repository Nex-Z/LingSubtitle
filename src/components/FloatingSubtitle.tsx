import { useEffect, useMemo, useRef, useState } from "react";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { getCurrentWindow } from "@tauri-apps/api/window";
import "./FloatingSubtitle.css";
import type { SubtitleSegmentPayload } from "../types/subtitle";

interface FloatingSettings {
  fontSize: number;
  bgOpacity: number;
  showOriginal: boolean;
}

interface FloatingSegment extends SubtitleSegmentPayload {
  firstSeenAt: number;
  lastUpdatedAt: number;
  stateChangedAt: number;
}

const DEFAULT_SETTINGS: FloatingSettings = {
  fontSize: 18,
  bgOpacity: 0.85,
  showOriginal: true,
};

const HANDOFF_KEEP_MILLIS = 700;
const STREAMING_FRESH_MILLIS = 1200;
const DRAFT_SWITCH_CHARS = 6;

function upsertSegment(
  segments: FloatingSegment[],
  payload: SubtitleSegmentPayload
): FloatingSegment[] {
  const now = Date.now();
  if (segments.length > 0 && segments[0].sessionId !== payload.sessionId) {
    return [
      {
        ...payload,
        firstSeenAt: now,
        lastUpdatedAt: now,
        stateChangedAt: now,
      },
    ];
  }
  const index = segments.findIndex((item) => item.segmentId === payload.segmentId);
  if (index === -1) {
    return [
      ...segments,
      {
        ...payload,
        firstSeenAt: now,
        lastUpdatedAt: now,
        stateChangedAt: now,
      },
    ].sort((left, right) => left.segmentId - right.segmentId);
  }

  const current = segments[index];
  const next = [...segments];
  next[index] = {
    ...current,
    ...payload,
    firstSeenAt: current.firstSeenAt,
    lastUpdatedAt: now,
    stateChangedAt: current.state === payload.state ? current.stateChangedAt : now,
  };
  return next;
}

function charLen(text?: string | null): number {
  return (text || "").length;
}

function translatedMain(segment: FloatingSegment | null): string {
  if (!segment) return "";
  return segment.translatedText || segment.translatedDraftText || "";
}

function shouldShowTranslation(segment: FloatingSegment | null): boolean {
  if (!segment) return false;
  return Boolean(segment.translatedText) || charLen(segment.translatedDraftText) >= DRAFT_SWITCH_CHARS;
}

export default function FloatingSubtitle() {
  const [segments, setSegments] = useState<FloatingSegment[]>([]);
  const [settings, setSettings] = useState<FloatingSettings>(() => {
    try {
      const saved = localStorage.getItem("floating-subtitle-settings");
      return saved ? { ...DEFAULT_SETTINGS, ...JSON.parse(saved) } : DEFAULT_SETTINGS;
    } catch {
      return DEFAULT_SETTINGS;
    }
  });
  const [showSettings, setShowSettings] = useState(false);
  const [displaySegmentId, setDisplaySegmentId] = useState<number | null>(null);
  const [handoffUntil, setHandoffUntil] = useState(0);
  const [nowTick, setNowTick] = useState(Date.now());
  const settingsRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    localStorage.setItem("floating-subtitle-settings", JSON.stringify(settings));
  }, [settings]);

  useEffect(() => {
    const timer = window.setInterval(() => setNowTick(Date.now()), 200);
    return () => window.clearInterval(timer);
  }, []);

  useEffect(() => {
    const handleClickOutside = (event: MouseEvent) => {
      if (settingsRef.current && !settingsRef.current.contains(event.target as Node)) {
        setShowSettings(false);
      }
    };
    if (showSettings) document.addEventListener("mousedown", handleClickOutside);
    return () => document.removeEventListener("mousedown", handleClickOutside);
  }, [showSettings]);

  useEffect(() => {
    let unlistenUpsert: UnlistenFn | null = null;

    const setupListeners = async () => {
      try {
        unlistenUpsert = await listen<SubtitleSegmentPayload>("subtitle-segment-upsert", (event) => {
          setSegments((prev) => {
            const previous = prev.find((item) => item.segmentId === event.payload.segmentId);
            const next = upsertSegment(prev, event.payload);

            if (prev.length > 0 && prev[0].sessionId !== event.payload.sessionId) {
              setDisplaySegmentId(event.payload.segmentId);
              setHandoffUntil(0);
              return next;
            }

            if (previous?.state === "streaming" && event.payload.state !== "streaming") {
              setDisplaySegmentId(event.payload.segmentId);
              setHandoffUntil(Date.now() + HANDOFF_KEEP_MILLIS);
            } else if (displaySegmentId === null) {
              setDisplaySegmentId(event.payload.segmentId);
            }

            return next;
          });
        });
      } catch (err) {
        console.error("Failed to attach listeners:", err);
      }
    };

    setupListeners();

    return () => {
      if (unlistenUpsert) unlistenUpsert();
    };
  }, [displaySegmentId]);

  const displaySegment = useMemo(() => {
    const current = displaySegmentId
      ? segments.find((segment) => segment.segmentId === displaySegmentId) || null
      : null;

    const latestStreaming = [...segments]
      .filter(
        (segment) =>
          segment.state === "streaming" &&
          !!segment.originalText &&
          nowTick - segment.lastUpdatedAt <= STREAMING_FRESH_MILLIS
      )
      .sort((left, right) => right.lastUpdatedAt - left.lastUpdatedAt)[0];

    const latestCommitted = [...segments]
      .filter((segment) => segment.state !== "streaming")
      .sort((left, right) => {
        if (right.segmentId !== left.segmentId) {
          return right.segmentId - left.segmentId;
        }
        return right.stateChangedAt - left.stateChangedAt;
      })[0];

    if (latestStreaming) {
      return latestStreaming;
    }

    if (current && current.state !== "streaming" && nowTick < handoffUntil) {
      return current;
    }

    if (latestCommitted) return latestCommitted;
    if (current) return current;
    return [...segments].sort((left, right) => right.segmentId - left.segmentId)[0] ?? null;
  }, [displaySegmentId, handoffUntil, nowTick, segments]);

  useEffect(() => {
    if (displaySegment && displaySegment.segmentId !== displaySegmentId) {
      setDisplaySegmentId(displaySegment.segmentId);
    }
  }, [displaySegment, displaySegmentId]);

  const handleDrag = () => {
    try {
      getCurrentWindow().startDragging();
    } catch {}
  };

  const handleClose = () => {
    try {
      getCurrentWindow().close();
    } catch {}
  };

  const updateSetting = <K extends keyof FloatingSettings>(
    key: K,
    value: FloatingSettings[K]
  ) => {
    setSettings((prev) => ({ ...prev, [key]: value }));
  };

  const preferTranslation = shouldShowTranslation(displaySegment);
  const mainText = displaySegment
    ? preferTranslation
      ? translatedMain(displaySegment)
      : displaySegment.originalText
    : "";
  const subText = displaySegment && preferTranslation ? displaySegment.originalText : "";
  const isDualLine = Boolean(subText) && settings.showOriginal;

  return (
    <div
      className="floating-container"
      style={{ background: `rgba(20, 20, 22, ${settings.bgOpacity})` }}
    >
      <div className="floating-drag-area" onMouseDown={handleDrag} title="拖动" />

      <div className="floating-toolbar">
        <button
          className="floating-btn"
          onClick={() => setShowSettings((prev) => !prev)}
          title="设置"
        >
          ⚙
        </button>
        <button className="floating-btn close" onClick={handleClose} title="关闭">
          ✕
        </button>
      </div>

      {showSettings && (
        <div className="floating-settings-panel" ref={settingsRef}>
          <div className="fs-row">
            <span className="fs-label">字号</span>
            <div className="fs-size-btns">
              {[14, 16, 18, 22, 26].map((size) => (
                <button
                  key={size}
                  className={`fs-size-btn ${settings.fontSize === size ? "active" : ""}`}
                  onClick={() => updateSetting("fontSize", size)}
                >
                  {size}
                </button>
              ))}
            </div>
          </div>
          <div className="fs-row">
            <span className="fs-label">透明度</span>
            <input
              type="range"
              className="fs-slider"
              min="0.3"
              max="1"
              step="0.05"
              value={settings.bgOpacity}
              onChange={(event) => updateSetting("bgOpacity", parseFloat(event.target.value))}
            />
          </div>
          <div className="fs-row">
            <span className="fs-label">显示原文</span>
            <label className="fs-toggle">
              <input
                type="checkbox"
                checked={settings.showOriginal}
                onChange={(event) => updateSetting("showOriginal", event.target.checked)}
              />
              <span className="fs-toggle-slider" />
            </label>
          </div>
        </div>
      )}

      <div className={`floating-subtitle-area ${isDualLine ? "dual" : "single"}`}>
        {mainText ? (
          <>
            <div
              className={`subtitle-line main ${preferTranslation ? "has-translation" : ""}`}
              style={{ fontSize: `${settings.fontSize}px` }}
            >
              {mainText}
            </div>
            <div
              className={`subtitle-line sub ${subText ? "visible" : ""}`}
              style={{ fontSize: `${Math.max(12, settings.fontSize - 4)}px` }}
            >
              {subText || "\u00A0"}
            </div>
          </>
        ) : (
          <div className="floating-empty">等待字幕...</div>
        )}
      </div>
    </div>
  );
}
