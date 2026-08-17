# A binary is displaced, never the process holding it

Status: accepted (2026-08-09, grilling session on locked binaries and partial installs)

Where a file cannot be written because the machine is executing it, the running image is moved
aside and the new file is written over the freed name. The process holding it is never stopped,
under any flag.

Measured 2026-08-09 on Windows 11 Home 10.0.26200, against a copy of `ping.exe` left running from
a scratch directory: overwriting the file in place was denied; renaming it aside succeeded; writing
a fresh file at the freed name succeeded; deleting the renamed file was denied; and the process
survived all four. The install a lock aborts is therefore available, and only the removal of what
was replaced is not.

Stopping the holder instead is refused because convergence is additive and a running process is
undeclared state. The single instance to hand argues the same way from consequence: the receiver
that holds `tool-use-statistics` open writes its decision log one request at a time, opening,
appending, and closing within each call rather than holding the file open across requests, so
killing it to upgrade it costs whatever request was in flight and nothing already written to
disk.

Displacement happens on denial rather than before it. The tool that was refused names the path it
could not write, so the narrowest possible act is available at exactly the moment it is needed;
displacing ahead of every install touches files that were never in the way. Any other failure
after a displacement restores the moved image to its name, because a run that removes a working
binary and then stops has left the machine worse than it found it.

What is left behind keeps the same stem in the same directory with a suffix appended after the
extension. The directory cargo installs into is on the search path by construction, so a name
ending in `.exe` would become an invocable stale command. Displacement first deletes any earlier
image of that exact name, which succeeds precisely when the old holder has exited, and `apply`
sweeps the same suffix as a prepass over the directories this program displaces into. `plan`
reports what survives as a notice and deletes nothing, per ADR 0006.

A resource whose displacement is itself refused reports `held`, which is neither `blocked` —
something missing rather than in use — nor `failed`, which is a fault rather than an obstruction.
It leaves the machine unconverged.

The capability belongs to the machine rather than to the path that first needed it. Three
consumers need it: a package manager writing its own directory, the released binary shape of ADR
0016 writing the tool directory, and the self-update of ADR 0019, which cannot overwrite its own
running image at all and for which displacement is the whole mechanism. It is expressed as scoped
operations with no raw file placement beside them, so that reaching a destination without
displacing, or displacing without restoring, is unavailable rather than discouraged: restoring is
not a call a caller makes, it is what the operation does on its own failure path.

## Considered options

- **Stop the holder**, optionally behind a flag. It converges the case nothing else can. Rejected
  on ADR 0005 and on the consequence above.
- **Report `held` and leave it to a person.** Honest, and the smallest possible change. Rejected
  because the holder need never exit: a receiver holds its own binary for as long as recording is
  on, so "stop it and re-run" is advice with no moment at which it can be taken.
- **Retry the move within the pass without displacing.** Converges wherever the holder exits inside
  the run, which is the case that was observed. Rejected because the case that motivated this is
  the one that does not.
- **Displace every declared binary before invoking the installer.** Removes the collision rather
  than recovering from it. Rejected because it needs the names up front to buy nothing, and leaves
  a moved image behind for files that were never in the way.
- **Move what was replaced to the tool directory**, where this program already owns what it leaves.
  Rejected on the same measurement: a move across volumes is a copy followed by a delete, and the
  delete is the one act denied while the holder runs.
- **Reap only immediately before the next displacement of that name.** Bounded, and it claims
  nothing beyond the path being written. Rejected because reaping then requires that binary to be
  held a second time, so in practice nothing is ever collected and each held binary leaves a
  permanent copy.

## Consequences

- **A replaced image survives for as long as something executes it.** The machine carries one stale
  copy per held binary until an apply sweeps it, and the sweep deletes only the suffix this program
  invented, ignoring every denial.
- **`held` is not retried across passes.** Nothing another resource converges can make a third
  party release a file, and the only unblocker is a person, so re-attempting would teach the run to
  report having tried.
- **A held resource exits non-zero**, like everything else that leaves drift open. What separates
  it from breakage is the category in the summary, not the exit status.
- **A running process keeps executing the image that was moved.** The file on disk is current while
  the process is not, and since convergence's interest ends at the file, that staleness is not
  drift and belongs to the program holding it.
- **The self-update displaces the image its own run is executing**, and continues on it, as ADR
  0019 already records of the run that upgrades.
- **Nothing is listed or removed that was not named.** Displacement touches the path being
  converged; the sweep touches one suffix in a fixed set of directories.
