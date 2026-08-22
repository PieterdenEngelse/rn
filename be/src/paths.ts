/**
 * Path formatting for display.
 *
 * Only ever for showing a path to a person. Anything that opens, spawns or
 * compares a path must keep the absolute form — a tilde is a shell convention,
 * not something the file system resolves, so a "~/..." string handed to fs or
 * to a child process is simply a wrong path.
 */

import { homedir } from "node:os";

/** Replace the user's home directory with `~`. */
export function display(p: string | null | undefined): string {
    if (!p) return "";
    const home = homedir();
    if (!home || home === "/") return p;
    if (p === home) return "~";
    return p.startsWith(home + "/") ? "~" + p.slice(home.length) : p;
}
