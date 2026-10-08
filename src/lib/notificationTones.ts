// The short tones for finished tasks, failures, permission requests and usage alerts.
//
// Each note has its own attack and decay and ends at silence (a shared envelope with a hard stop made
// the notes click), the sound is scheduled a moment after the context runs so the start is not clipped,
// and a tone that arrives while another is playing waits for it instead of overlapping.
// A fresh context is used per tone: a long-lived one can be suspended by the system while idle and then
// refuses to resume without a user gesture, which silenced every later tone.
export type ToneKind = "completed" | "failed" | "permission" | "usage";

interface Note { frequency: number; at: number; length: number }
interface Pattern { type: OscillatorType; notes: Note[] }

const patterns: Record<ToneKind, Pattern> = {
  completed: { type: "sine", notes: [{ frequency: 620, at: 0, length: 0.2 }, { frequency: 820, at: 0.11, length: 0.3 }] },
  failed: { type: "sine", notes: [{ frequency: 330, at: 0, length: 0.22 }, { frequency: 250, at: 0.12, length: 0.34 }] },
  permission: { type: "triangle", notes: [{ frequency: 440, at: 0, length: 0.14 }, { frequency: 440, at: 0.19, length: 0.16 }] },
  usage: { type: "sine", notes: [{ frequency: 740, at: 0, length: 0.14 }, { frequency: 554, at: 0.15, length: 0.14 }, { frequency: 740, at: 0.3, length: 0.2 }] },
};

const ATTACK = 0.012;
const LEAD = 0.06;
const GAP = 0.14;
const SILENCE = 0.0001;

export const toneDuration = (kind: ToneKind) => Math.max(...patterns[kind].notes.map((note) => note.at + note.length));

/** Schedules one tone on the context starting at `startAt` and returns when it ends. */
export function scheduleTone(context: AudioContext, kind: ToneKind, volume: number, startAt: number): number {
  const peak = Math.max(SILENCE * 2, 0.09 * Math.max(0, Math.min(100, volume)) / 100);
  const { type, notes } = patterns[kind];
  for (const note of notes) {
    const begin = startAt + note.at;
    const end = begin + note.length;
    const gain = context.createGain();
    gain.gain.setValueAtTime(SILENCE, begin);
    gain.gain.linearRampToValueAtTime(peak, begin + ATTACK);
    gain.gain.exponentialRampToValueAtTime(SILENCE, end);
    gain.connect(context.destination);
    const oscillator = context.createOscillator();
    oscillator.type = type;
    oscillator.frequency.setValueAtTime(note.frequency, begin);
    oscillator.connect(gain);
    oscillator.start(begin);
    // The envelope reaches silence at `end`; stopping a little later never cuts an audible sound.
    oscillator.stop(end + 0.03);
    oscillator.onended = () => { oscillator.disconnect(); gain.disconnect(); };
  }
  return startAt + toneDuration(kind);
}

const sleep = (ms: number) => new Promise<void>((resolve) => setTimeout(resolve, ms));

/** Plays one tone on a fresh context, which is closed once the tone has ended. */
async function playOnce(kind: ToneKind, volume: number) {
  const Context = window.AudioContext ?? (window as unknown as { webkitAudioContext?: typeof AudioContext }).webkitAudioContext;
  if (!Context) return;
  const context = new Context({ latencyHint: "interactive" });
  try {
    // A context can start suspended; give it a moment to run so the first notes are not clipped.
    if (context.state !== "running") await Promise.race([context.resume().catch(() => undefined), sleep(300)]);
    scheduleTone(context, kind, volume, context.currentTime + LEAD);
    await sleep(toneDuration(kind) * 1000 + 450);
  } finally {
    void context.close().catch(() => undefined);
  }
}

// Tones play one after another: overlapping ones sounded cut, and browsers cap live contexts.
let queue: Promise<void> = Promise.resolve();

export function playTone(kind: ToneKind, volume: number) {
  queue = queue
    .then(() => playOnce(kind, volume))
    .catch(() => undefined) // Audio is optional and may be blocked until the first interaction.
    .then(() => sleep(GAP * 1000));
}
