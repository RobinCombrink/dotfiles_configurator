---
status: accepted
---

# Each binary is its own record of the commit it was built from

A workspace member's binary is installed without cargo recording it, and the only record of
where it came from is the binary itself: the `<name> <commit>` its `--version` prints, as ADR
0040 reads it for currency. Provenance is that stamped commit resolved in a declared workspace's
clone. Nothing else on the machine, cargo's install record included, is read to learn which
binaries the configurator built.

A member is built with `cargo install --locked --no-track --root <staging> --git <url> --rev
<commit> <crate>`, into a staging directory of its own inside the build cache, and each binary
it leaves there is moved onto its name in cargo's bin directory as ADR 0022 places any binary:
the replacement lands beside the name, and a binary being executed is moved aside first. Cargo
never writes cargo's bin directory for a member, so it records nothing there and refuses nothing
there.

Measured 2026-11-10 against cargo 1.99.1 on Windows 11:

| What was done | What cargo did |
| --- | --- |
| `cargo install --no-track --root <staging>` | Wrote only the binaries, into `<staging>\bin`, and no record |
| The same again over the binaries already in `<staging>\bin` | Refused, "binary `X.exe` already exists in destination", exit 101 |

So the staging directory is cleared before each build.

**Attribution.** Every binary in cargo's bin directory that no cargo record claims is asked its
`--version`, within a deadline, and is one of four things:

- **attributed** to a declared workspace, when its stamp names a commit a declared clone holds;
- **in no clone**, when the stamp names a commit no declared clone holds;
- **undecided**, when a declared clone could not be read or could not be asked for the commit;
- **unstamped**, when it printed anything but `<name> <forty-hex commit>`, failed, or did not
  finish in time.

A binary cargo records is never run: it is cargo's, and a registry crate is never withdrawn.

**Withdrawal.** An apply removes an attributed binary that no current member of any declared
workspace declares. It deletes the file, and moves aside one being executed, as ADR 0022 does.
A binary in no clone is kept, and every plan and apply names it with its stamp and the command
that removes it. One undecided binary decides nothing that run, for any binary, since the
unreadable clone may be the one the others belong to. An unstamped binary is left alone. A
workspace withdrawn from the configuration is never read, so nothing built from it is withdrawn,
per ADR 0005.

**Switch-over.** A machine whose members cargo recorded under ADR 0041 still holds those
records. Every cargo git record whose source is a declared workspace's repository, whichever
account fetched it, is uninstalled, with the files it names kept in place, so the binaries pass
from cargo's record to their own. That such records exist is read from the machine on each
plan; nothing records that the switch happened.

This supersedes ADR 0041, whose removal of a binary no member declares it keeps while replacing
its reading of cargo's install record to learn what a member installed, and continues ADR 0040,
whose stamp it makes the only record.

## Considered options

- **A final `cargo install` to rewrite cargo's record after the swap.** Rejected: the install
  is refused while a binary it would overwrite is executing, and the record it leaves then is
  stale on exactly the apply that needed it.
- **Copying the running binary out and installing over it.** Rejected: two cargo runs per
  member, and the record still names what cargo last wrote rather than what is in place.
- **A record of installs the configurator owns.** Rejected for the reason ADRs 0039 and 0043
  reject a receipt of past runs: it is a second fact about the machine that drifts from the
  machine, and the binaries already say what they are.
- **Declaring the repository in the stamp.** Rejected: the commit already names the build, and
  a repository written beside it is a second fact the binary could get wrong; the declared
  clones answer which repository holds the commit.
- **Deleting a binary in no clone.** Rejected: it deletes on a guess. A commit no clone holds
  may be an unpushed build, another clone's, or a history rewritten under it, and none of those
  is the configurator's to remove.

## Consequences

- **A removal is a change of its own kind**, beside the change a resource makes, with its own
  count, as under ADR 0041. A pending removal or hand-over leaves the plan unconverged, and the
  plan says which commit of which workspace each binary was built from.
- **A crate merged into another keeps the binaries the surviving member declares**, since a
  binary a current member declares is never withdrawn.
- **The hand-over takes two applies to finish.** The apply that untracks a record leaves its
  binaries claimed for that run; the next plan reads them by their stamps and withdraws any no
  member declares.
- **An unrecorded binary is read on every plan.** Every binary in cargo's bin directory that no
  cargo record claims is started once per plan, rustup's proxies among them, each bounded by the
  deadline.
- **A binary is withdrawn only with every declared clone read.** A clone that cannot be opened
  defers every withdrawal to an apply that can read it.
- **A kept binary is named until a person acts.** The notice gives the path to remove; nothing
  removes it automatically.
- **`cargo install --list` no longer lists the members**, and `cargo uninstall` no longer
  removes them; deleting the file is the uninstall.
