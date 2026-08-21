/**
 * Structured logging. Every record is raw material for an info panel in the
 * frontend, so log facts (counts, durations, paths, reasons) rather than prose.
 * A line that says "done" cannot become an explanation; {files: 412, ms: 240}
 * can.
 */
export function step(name: string, detail: Record<string, unknown> = {}): void {
    console.log(JSON.stringify({ t: Date.now(), step: name, ...detail }));
}
