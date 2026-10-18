# Plan 056: Voice input for Speak with Alfred

## Status and scope

- Status: TODO.
- Priority: P2. Effort: M.
  - Step 0, the decision spike: about 30 minutes.
  - Track A, macOS on-device dictation: 1–1½ days.
  - Track B, Windows and Linux: 2–3 days, optional and later.
- Requested on: 2026-10-16.
- Baseline: desktop commit `4894951` plus the uncommitted Speak with Alfred
  work. Commit that work before starting.
- Depends on: the Speak with Alfred composer
  (`src/features/builder/components/builder-page/builder-page.tsx`).
- Independent of: [Plan 055](055-speak-with-alfred-memory.md).

Add a microphone button to the Speak with Alfred composer. The user clicks it,
talks, and watches their words appear live in the message box. When they stop,
the text stays editable and is sent only when they press Enter. Recognition
runs on the Mac whenever the OS supports it, and no audio is ever stored.

### Non-goals

- No wake word or always-on listening.
- No voice output in this plan. "Alfred reads replies aloud" is a separate
  follow-up (see the end of this plan).
- No audio upload to a third-party transcription API.
- No voice input inside workflows or other text fields. Only the Speak with
  Alfred composer.

## Current state (verified 2026-10-16)

| Fact | Where |
| --- | --- |
| The webview already grants microphone and camera requests from the page (`WKPermissionDecision::Grant`) | wry 0.55.1 `src/wkwebview/class/wry_web_view_ui_delegate.rs:126-136` |
| The app's `Info.plist` is an empty `<dict/>`. macOS refuses microphone or speech access without `NSMicrophoneUsageDescription` and `NSSpeechRecognitionUsageDescription` | `src-tauri/Info.plist` |
| Rust bindings exist in Alfred's objc2 family: `objc2-speech` 0.3.2 and `objc2-avf-audio` 0.3.2, matching `objc2` 0.6.4 and `objc2-foundation` 0.3.2; `block2` 0.6.2 is already in `Cargo.lock` | crates.io, `src-tauri/Cargo.lock` |
| Alfred already calls AppKit through objc2 on macOS only (`cfg(target_os = "macos")` dependencies) | `src-tauri/Cargo.toml` |
| macOS Dictation (the dictation key, or Fn pressed twice) types into any focused text field. It is the zero-code interim answer; not yet checked in this composer | macOS |
| The composer already has an icon button (attach images) and a send/stop button; new controls go between them | `builder-page.tsx`, `builder.css` |

## User experience

```text
┌─────────────────────────────────────────────────────────────┐
│ Ask Alfred, or describe a workflow…                          │
│ every weekday at nine summarize my ▍                         │  ← live partial text
├─────────────────────────────────────────────────────────────┤
│ [img] [● ▁▃▅▂▆ 0:07]  Claude Code ▾  Default model ▾   (↑)  │
└─────────────────────────────────────────────────────────────┘
```

- **Idle:** a Phosphor `microphone` button. Its tooltip names the shortcut.
- **Listening:**
  - The button becomes a pill with a pulsing dot, five live level bars driven
    by input loudness, and an elapsed timer.
  - Partial words appear at the cursor in a muted style and settle into normal
    text when final.
  - The composer border beam (the existing `--builder-angle` animation) runs in
    the brand teal.
- **Stopping:** click the pill, press Esc (cancel: discard this session's
  words), or press Enter (stop, then send). Silence does not auto-stop in v1.
- **Denied or unsupported:**
  - If permission was denied, the button stays visible, and clicking it
    explains how to allow access in System Settings, with a button that opens
    the right pane.
  - On platforms without support, the button is hidden.
- **Accessibility:** the button uses `aria-pressed`, and a polite live region
  announces "Listening" and "Stopped". All motion honors the existing
  reduced-motion rule.

## Step 0: decision spike (about 30 minutes, required first)

WebKit exposes `SpeechRecognition` in Safari, but whether it works inside this
WKWebView is unverified. Settle it before writing native code:

1. Add both usage strings to `src-tauri/Info.plist` (they are needed in either
   case):
   - `NSMicrophoneUsageDescription`: "Alfred listens only while the microphone
     button in Speak with Alfred is on."
   - `NSSpeechRecognitionUsageDescription`: "Alfred turns your speech into text
     on this Mac."
2. In a `bun run dev` build, temporarily log
   `typeof (window.SpeechRecognition ?? window.webkitSpeechRecognition)`, start
   one recognition session, and record:
   - whether `onresult` fires with interim results;
   - whether a permission prompt appears, and which app it names;
   - whether it works with Wi-Fi off. This tells you whether it runs on the Mac
     or uses Apple's servers.
3. Decide, and record the evidence in this plan:
   - **Web route:** if interim results work offline, implement the UX against
     the Web Speech API, frontend only. Skip Track A's Rust module. Keep the
     same command-free design and the same UX.
   - **Native route:** otherwise implement Track A as written.
4. Remove the probe code.

STOP if the dev build cannot prompt for the microphone at all. An unbundled
`target/debug/alfred` may attribute permission to the terminal. Repeat the
spike in a bundled build (`bun run build`, app target) before choosing.

## Track A: macOS native, on-device recognition

### Rust module `src-tauri/src/voice/` (macOS only)

- Dependencies, macOS-only target section:
  `objc2-speech = "0.3.2"`, `objc2-avf-audio = "0.3.2"`, `block2 = "0.6"`.
  Enable only the framework features the code uses.
- State: one session at a time behind a `Mutex`, holding the session id, the
  `AVAudioEngine`, the `SFSpeechAudioBufferRecognitionRequest`, the recognition
  task, and the start time.
- Commands, registered in `lib.rs`:
  - `voice_status() -> { supported, microphone, speech, onDevice }`. Use the
    authorization states `not_determined`, `denied`, `restricted`, and
    `authorized`. `onDevice` comes from
    `SFSpeechRecognizer.supportsOnDeviceRecognition` for the current locale.
  - `voice_start(sessionId)`:
    - Request any missing authorization (`SFSpeechRecognizer.requestAuthorization`
      and microphone access).
    - Create the recognizer for the current locale.
    - Set `requiresOnDeviceRecognition = true` when supported; otherwise
      continue, and report `onDevice: false` so the UI can say so.
    - Set `shouldReportPartialResults = true`.
    - Install a tap on `inputNode` that appends each buffer to the request and
      computes RMS for the level meter.
    - Start the engine.
  - `voice_stop(sessionId)` ends audio and waits for the final result (with a
    2 s timeout). `voice_cancel(sessionId)` tears down and discards.
- Events:
  - `voice://transcript { sessionId, text, isFinal }`: the full best
    transcription so far, not a delta.
  - `voice://level { sessionId, level }`: 0–1, throttled to about 20 Hz.
  - `voice://ended { sessionId, reason }`, where `reason` is one of
    `stopped`, `cancelled`, `error`, `timeout`, or `device_changed`.
- Limits and lifecycle:
  - Hard-stop after 120 s with reason `timeout`. Server-based recognition
    sessions are about a minute; when `onDevice` is false, cap at 55 s.
  - Stop on `AVAudioEngineConfigurationChange`, for example when AirPods
    connect, with reason `device_changed`.
  - Stop when the window hides and on app exit, and stop if a new session
    starts.
- Threading: recognition callbacks arrive on arbitrary queues. Never hold the
  session mutex while emitting. Every Objective-C object stays inside the
  module; nothing objc2 crosses into other modules.
- Non-macOS builds compile a stub whose `voice_status` returns
  `supported: false`.

### Frontend

- `src/features/builder/voice.ts`:
  - It owns the subscriptions and exposes
    `start / stop / cancel / status`.
  - It merges each partial transcript into the draft at the cursor captured at
    start, replacing the previous partial text from this session rather than
    appending it again.
  - Keep the merge logic pure and unit-tested.
- Add the Phosphor `microphone` SVG to `src/assets/icons/phosphor/` and run
  `bun run scripts/generate-phosphor-icons.mjs` (iconography rules).
- In `builder-page.tsx`:
  - Add the mic button and listening pill.
  - Esc cancels, and Enter stops and then sends.
  - Disable the mic while the agent is working.
- In `builder.css`:
  - Build the level bars and pulsing dot from tokens only, with no literals
    except `geometry:`-tagged sizes.
  - Reuse `builder-turn` for the border beam.

## Track B: Windows and Linux (optional, later)

- Capture with `getUserMedia` in the page. WebView2 and WebKitGTK permission
  behavior must be checked per platform. Resample to 16 kHz mono PCM and
  stream it to Rust.
- Transcribe locally with whisper.cpp (`whisper-rs`). The model downloads on
  first use, with explicit consent, into app data with a pinned SHA-256:
  `base.en` is about 142 MB and `small` about 466 MB.
- Use the same events and UX as Track A. macOS keeps Track A.
- STOP and re-plan if the binary-size or model-size budget is not approved.

## Privacy

- No audio is written to disk or sent anywhere by Alfred.
- On-device recognition is preferred. When the OS can only recognize on
  Apple's servers, the pill shows "Uses Apple's speech service" before the
  first session and remembers that the user acknowledged it.
- Transcripts become ordinary chat text, persisted only if the user sends them.

## Files expected to change

| Area | Files |
| --- | --- |
| Native | `src-tauri/Cargo.toml`, `src-tauri/Info.plist`, new `src-tauri/src/voice/mod.rs`, `src-tauri/src/lib.rs` |
| Frontend | new `src/features/builder/voice.ts` (+ test), `components/builder-page/builder-page.tsx`, `builder.css`, `src/assets/icons/phosphor/microphone.svg`, `src/components/icon/phosphor.ts` (generated) |
| Docs | `docs/` note on voice input and permissions; this plan's status |

## Acceptance tests

| Scenario | Expected result |
| --- | --- |
| First use | macOS asks for microphone, then speech; both prompts name Alfred, not the terminal (bundled build) |
| Speak "build a workflow that…" | Words appear live within about 300 ms of speaking; final text replaces the partial text without duplication |
| Cursor in the middle of existing text | Dictation inserts at the cursor; surrounding text intact |
| Esc while listening | Session's words removed; prior draft intact |
| Enter while listening | Stops, waits for final text, sends once |
| Permission denied | Clicking the mic explains and opens System Settings; no crash |
| Wi-Fi off, on-device supported | Recognition still works |
| AirPods connect mid-session | Session ends with a notice; text so far kept |
| Two minutes of speech | Auto-stops with a notice; text kept |
| Agent working | Mic disabled |
| Linux or Windows build (Track A only) | No mic button; nothing else changes |
| Both themes, reduced motion, keyboard only | Pill legible, motion reduced, button reachable, live region announces state |

## Verification

- Run `bun run check` and `git diff --check`.
- Unit tests cover the transcript merge logic and the frontend state machine.
- The Rust stub compiles on non-macOS targets.
- Live verification must happen in a **bundled** macOS build. TCC prompts and
  usage strings do not behave the same in `bun run dev`.

## Risks and mitigations

- **TCC in development:** do the Step 0 spike in a bundled build if prompts
  misbehave.
- **objc2 framework API drift:** pin 0.3.2 alongside the existing 0.3.2 crates.
  Isolate all Objective-C calls in `voice/`.
- **Server-only recognition for some languages:** surface `onDevice: false`
  plainly and use the shorter cap.
- **Duplicate or flickering partial text:** transcripts are full snapshots, and
  the merge replaces this session's span.
- **Audio device churn:** stop on configuration change rather than trying to
  rebuild the engine mid-session.

## Follow-up (separate small plan): Alfred speaks back

- Add a "Read replies aloud" switch in Setup.
- On macOS, speak each finished assistant reply with `AVSpeechSynthesizer`
  (through `objc2-avf-audio`, already added by Track A), skipping code blocks
  and tool output.
- The voice comes from the Plan 055 persona.
- About ½ day.

## Completion definition

Done when a macOS user can click the mic in Speak with Alfred, see their speech
appear live, edit it, and send it, with recognition on the Mac where supported,
clear handling of denied permissions, and no audio stored.
