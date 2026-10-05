---
status: accepted
---

# What the machine refuses without elevation runs in one elevated batch

Where Windows refuses a symlink with os error 1314, because the apply holds neither elevation nor
Developer Mode, or refuses to start a downloaded installer with os error 740, because the installer
demands elevation, the entry is collected rather than failed. At the end of the pass that collected
them, every collected entry runs in one elevated batch, so a pass costs at most one UAC prompt. The
batch is the configurator relaunching itself under a hidden subcommand, through its own
`windows-sys` call to `ShellExecuteExW` with the `runas` verb and `SEE_MASK_NOCLOSEPROCESS`, which
hands back the process to wait on and read the exit code of.

Observed on the work machine on 2026-10-05, in one apply:

| Entries | Refusal |
| --- | --- |
| 11 directory symlinks | `A required privilege is not held by the client. (os error 1314)` |
| Steam's installer | `The requested operation requires elevation. (os error 740)` |

Developer Mode was off, and all eleven links and the installer went through on a second apply that
was elevated, so neither refusal is specific to a managed machine.

A refused link stays a symlink, for a directory or a file. A junction would need no privilege, but
it serves directories only, so file links would still need the batch, and the resource would then
place one of two kinds of link depending on what the apply held.

The batch travels on the elevated process's command line, which is fixed once the process starts.
A file the user can write could be swapped by a same-user process while the prompt is open, and
the elevated side would then do whatever the swapped file says. A batch longer than the command
line accepts fails every entry in it loudly rather than splitting, since a split costs a second
prompt. The batch and its results are one serde type shared by both sides: the elevated side
settles each entry on its own and writes every entry's outcome to a results file the parent names,
so one entry's failure decides no other's, and the next pass reads the machine again regardless.

A declined prompt is `ShellExecuteExW` failing with `ERROR_CANCELLED` (1223), and holds every entry
of the batch with "elevation declined". An apply that is already elevated never relaunches: a link
it is still refused is reported failed with the os error it was refused with. On other platforms
neither refusal is matched, so the batch never forms. The run log of a Windows apply opens by
recording whether the apply is elevated and whether Developer Mode is on, read from
`AllowDevelopmentWithoutDevLicense` under
`HKLM\SOFTWARE\Microsoft\Windows\CurrentVersion\AppModelUnlock`, a missing value counting as off.

## Considered options

- **Hold each entry with the reason and leave it to the person.** Honest, and no elevated code at
  all. Rejected because the only remedy left is to re-run the whole apply elevated, which runs
  every child of it, cargo and git included, as an administrator.
- **Relaunch the whole apply elevated.** Rejected on the same ground: elevation is wanted for a
  dozen entries, not for the run.
- **Elevate each entry as it is refused.** Rejected because the observed apply would have asked
  twelve times.
- **Fall back to a junction for a directory link.** Rejected above.
- **sudo for Windows.** Rejected because it ships only with recent builds of Windows 11 and is off
  until a person enables it, so it cannot be relied on by the program that sets the machine up.
- **gsudo.** Rejected because it is a third-party install the configurator would have to provision
  before it could provision anything that needs elevation.
- **A named pipe between the two processes.** Rejected because a same-user process can connect to
  the pipe while the prompt is open, and it needs a server on one side and a protocol on both.
- **A batch file, or the batch written to any file.** Rejected because a same-user process can
  rewrite it between the parent writing it and the elevated side reading it.
- **The `runas`, `deelevate`, `privilege` and `elevated-command` crates.** Rejected because each is
  stale, loses `ERROR_CANCELLED`, or returns no exit code, and the decline and the exit code are
  both what the parent reports from.

## Consequences

- **A collected entry stays unfinished until the batch settles it**, and is then reported once:
  converged, failed with its own error or exit code, or held with "elevation declined".
- **A declined prompt is not asked again in a later pass.** It is held, and ADR 0022 does not retry
  what is held, so a person who declines is asked once per apply.
- **The elevated side writes a run log of its own**, so every child it runs has its exit recorded,
  as every child of the apply does.
- **No test can answer a UAC prompt.** The relaunch, the decline and the command-line length the
  `runas` path accepts are checked by a person on a machine with Developer Mode off. The length
  measured through the default verb on the personal machine on 2026-10-05 was 30,426 characters,
  and the batch is refused above it.
