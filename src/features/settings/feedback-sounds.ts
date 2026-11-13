/**
 * Built-in feedback tones played inside Alfred while the window is focused.
 *
 * Desktop notifications own the native sound when Alfred is in the background.
 * These Web Audio cues fill the foreground gap without requiring notification
 * permission, and they stay intentionally short and quiet.
 */
export type SoundPreset = "system" | "chime" | "ping" | "pop" | "none";

type Tone = {
  frequency: number;
  /** Optional glide target; defaults to a constant frequency. */
  endFrequency?: number;
  /** Offset from the start of the cue, in seconds. */
  offset: number;
  duration: number;
  /** Peak gain, 0–1. */
  gain: number;
  type: OscillatorType;
};

const CUES: Record<Exclude<SoundPreset, "none">, Tone[]> = {
  system: [
    { frequency: 880, offset: 0, duration: 0.11, gain: 0.16, type: "sine" },
    { frequency: 660, offset: 0.1, duration: 0.16, gain: 0.13, type: "sine" },
  ],
  chime: [
    { frequency: 1046.5, offset: 0, duration: 0.35, gain: 0.12, type: "sine" },
    {
      frequency: 1318.5,
      offset: 0.07,
      duration: 0.4,
      gain: 0.1,
      type: "sine",
    },
  ],
  ping: [
    { frequency: 1760, offset: 0, duration: 0.2, gain: 0.14, type: "sine" },
  ],
  pop: [
    {
      frequency: 420,
      endFrequency: 180,
      offset: 0,
      duration: 0.08,
      gain: 0.2,
      type: "triangle",
    },
  ],
};

type AudioContextConstructor = typeof AudioContext;

let audioContext: AudioContext | null = null;

function getAudioContext(): AudioContext | null {
  if (typeof window === "undefined") return null;
  const webkitWindow = window as unknown as {
    webkitAudioContext?: AudioContextConstructor;
  };
  const Constructor = window.AudioContext ?? webkitWindow.webkitAudioContext;
  if (!Constructor) return null;

  try {
    audioContext ??= new Constructor();
    if (audioContext.state === "suspended") {
      void audioContext.resume().catch(() => {
        /* Playback stays deferred until a user gesture unlocks the context. */
      });
    }
    return audioContext;
  } catch {
    return null;
  }
}

/**
 * Play one of the built-in cues.
 *
 * Silent when the webview has no audio support, the cue is "none", or the
 * context is unavailable. Feedback must never be allowed to break a run.
 */
export function playSoundPreset(preset: SoundPreset): void {
  if (preset === "none") return;
  const context = getAudioContext();
  if (!context) return;

  const start = context.currentTime;
  for (const tone of CUES[preset]) {
    try {
      const oscillator = context.createOscillator();
      const gain = context.createGain();
      const toneStart = start + tone.offset;
      const attack = Math.min(0.01, tone.duration / 2);

      oscillator.type = tone.type;
      oscillator.frequency.setValueAtTime(tone.frequency, toneStart);
      if (tone.endFrequency) {
        oscillator.frequency.exponentialRampToValueAtTime(
          tone.endFrequency,
          toneStart + tone.duration,
        );
      }

      gain.gain.setValueAtTime(0, toneStart);
      gain.gain.linearRampToValueAtTime(tone.gain, toneStart + attack);
      gain.gain.exponentialRampToValueAtTime(
        0.0001,
        toneStart + tone.duration,
      );

      oscillator.connect(gain);
      gain.connect(context.destination);
      oscillator.start(toneStart);
      oscillator.stop(toneStart + tone.duration + 0.02);
    } catch {
      /* One failed cue must not block the others. */
    }
  }
}

/** Lower bound between two condition-skip cues, in milliseconds. */
export const CONDITION_SKIP_SOUND_INTERVAL_MS = 1500;
