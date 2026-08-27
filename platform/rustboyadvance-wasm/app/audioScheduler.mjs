// Gapless scheduling for streamed WebAudio buffers.
//
// The emulator produces a small chunk of PCM samples every emulated frame.
// Playing each chunk with `AudioBufferSourceNode.start()` (no argument) starts
// it at `AudioContext.currentTime`, i.e. "now". Because the JS frame loop does
// not tick at exactly the audio-buffer duration, consecutive chunks either
// overlap (loop ran early) or leave a silent gap (loop ran late), which is
// heard as choppy audio (issue #80).
//
// The fix is the standard streaming pattern: keep a running "playback head"
// timestamp and schedule each buffer to begin exactly where the previous one
// ended, clamping to `currentTime` so we never schedule in the past after an
// underrun.

// Compute the start time for the next audio buffer and the advanced playback
// head. Pure function so the continuity behaviour is testable without a browser.
//
//   currentTime    - AudioContext.currentTime (monotonic, seconds)
//   playbackHead   - end time of the previously scheduled buffer (seconds)
//   bufferDuration - duration of the buffer to schedule (seconds)
//
// Returns { startTime, nextPlaybackHead }.
export function scheduleAudioBuffer(currentTime, playbackHead, bufferDuration) {
	const startTime = Math.max(currentTime, playbackHead);
	return { startTime, nextPlaybackHead: startTime + bufferDuration };
}
