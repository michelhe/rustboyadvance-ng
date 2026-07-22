import assert from "node:assert/strict";
import { test } from "node:test";

import { scheduleAudioBuffer } from "./audioScheduler.mjs";

// A buffer is BUFFER_SECONDS long, but the driving JS loop wakes up with jitter
// so the AudioContext clock does not advance by exactly one buffer per chunk.
const BUFFER_SECONDS = 0.016;
// currentTime deltas between consecutive playAudio() calls. Each is strictly
// less than BUFFER_SECONDS, so the loop always keeps the audio buffered ahead
// of the clock (no producer underrun) while still jittering enough that the
// naive "start now" scheme is never contiguous.
const CLOCK_DELTAS = [0.01, 0.014, 0.008, 0.015, 0.012, 0.013, 0.009, 0.011];
const EPSILON = 1e-9;

function driveClock(deltas) {
	const times = [0];
	for (const d of deltas) times.push(times[times.length - 1] + d);
	return times;
}

// The previous implementation: every chunk starts at currentTime ("now").
function naiveWindows(clock) {
	return clock.map((currentTime) => ({
		start: currentTime,
		end: currentTime + BUFFER_SECONDS,
	}));
}

// The scheduled implementation driven through the pure scheduler.
function scheduledWindows(clock) {
	const windows = [];
	let playbackHead = 0;
	for (const currentTime of clock) {
		const { startTime, nextPlaybackHead } = scheduleAudioBuffer(
			currentTime,
			playbackHead,
			BUFFER_SECONDS,
		);
		windows.push({ start: startTime, end: nextPlaybackHead });
		playbackHead = nextPlaybackHead;
	}
	return windows;
}

test("naive immediate scheduling produces overlaps or gaps (the reported defect)", () => {
	const windows = naiveWindows(driveClock(CLOCK_DELTAS));
	let discontinuities = 0;
	for (let i = 1; i < windows.length; i++) {
		if (Math.abs(windows[i].start - windows[i - 1].end) > EPSILON) {
			discontinuities++;
		}
	}
	// Every boundary is discontinuous under the naive scheme -> choppy audio.
	assert.equal(discontinuities, windows.length - 1);
});

test("scheduled playback is gapless and never overlaps once buffered ahead", () => {
	const windows = scheduledWindows(driveClock(CLOCK_DELTAS));
	for (let i = 1; i < windows.length; i++) {
		// Contiguous: next buffer begins exactly where the previous ended.
		assert.ok(
			Math.abs(windows[i].start - windows[i - 1].end) <= EPSILON,
			`boundary ${i} not contiguous: prev.end=${windows[i - 1].end} start=${windows[i].start}`,
		);
	}
});

test("scheduling never targets a time in the past (underrun resyncs to now)", () => {
	// Force a long stall so the playback head falls behind the clock.
	const clock = [0, 0.016, 0.032, 5.0, 5.016];
	let playbackHead = 0;
	for (const currentTime of clock) {
		const { startTime, nextPlaybackHead } = scheduleAudioBuffer(
			currentTime,
			playbackHead,
			BUFFER_SECONDS,
		);
		assert.ok(
			startTime >= currentTime - EPSILON,
			`scheduled in the past: start=${startTime} currentTime=${currentTime}`,
		);
		playbackHead = nextPlaybackHead;
	}
});
