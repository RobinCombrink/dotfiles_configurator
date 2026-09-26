---
status: accepted
---

# Every program's output drops its NULs

Output read from any program the configurator runs has every NUL removed before anything matches
against it. The step is applied to every program's output alike, and there is no per-program
setting that turns it on.

Some Windows programs, `wsl.exe` among them, emit UTF-16LE the moment their output is redirected.
That output decodes as valid UTF-8 with a NUL between every character, so a declared check matching
`wsl --list` against `Ubuntu` never succeeds and the resource reads as drifted on every run.
Removing the NULs makes it matchable, and on output that holds none it changes nothing, so applying
it everywhere costs the programs that never needed it nothing.

Deciding it per program was rejected. It would be a setting that has to be correct for every
program that needs it, and a declared check can name any program at all, so the set of programs
needing it is open. Detecting and decoding UTF-16 through an encoding library was the other
alternative, and removing the NULs was chosen over it.
