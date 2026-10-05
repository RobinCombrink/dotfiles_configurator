---
status: accepted
---

# A workspace member's currency is read from the build its binaries report

Whether a workspace member needs installing is decided by the build each of its declared binaries
reports, not by cargo's install record. Every binary the member declares, resolved as ADR 0021
resolves them, is judged on two separate facts, and the two are matched as a pair:

- **Its own copy** in the directory cargo installs into, asked for `--version`: **absent**;
  **current**, where the commit it reports fingerprints, under ADR 0007, the same as the checkout;
  or **stale**, where the fingerprint differs, the commit is one the clone cannot answer for, the
  stamp is `unrecorded`, or the output is not `<name> <commit>` at all — a build from before the
  stamp printing its package version, a failed request, another binary's name.
- **What the search path resolves** for its name: **ours**, **another** file, which shadows it,
  or **nothing**. The search path is the one read from the registry, the machine's entries before
  the user's, each matched stored and expanded as ADR 0029 matches them, never this process's own.

The member installs when any binary's own copy is absent or stale, and the reason names each such
binary with its verdict. A shadowed binary is reported as a finding and never as drift, since no
apply can change what another file earlier on the search path does; one whose own copy is also
stale is still installed. A member is unassessable while cargo's bin directory is missing from
the search path, a requirement observed under ADR 0004 like any other.

Cargo's install record is a record of past runs, which the configurator's actual state is never
read from. The binary is the machine's own answer: dotfiles ADR 0049 stamps every binary in that
workspace with the commit it was built from, and dotfiles' workspace test holds every bin crate to
it. Measured 2026-10-05 on Windows 11, `stop-gate --version` printed
`stop-gate e54928664eb4222b2ea79c7ef56126c5c58c7e01`.

This supersedes ADR 0021's rule that a member drifts by a stat of each binary beside the commit
the install record names, and ADR 0007's consequence that a member whose installed commit the
clone cannot resolve is unreadable. That member is now stale, and installing over it is the
repair.

## Considered options

- **Keep the install record as the comparand and stat each binary beside it**, as ADR 0021 did.
  Rejected: the record survives a rolled-back install, a binary replaced by hand and a build
  made elsewhere, and in each case it answers for a file it never looked at.
- **Read an unresolvable commit as unreadable**, as ADR 0007 did. Rejected: it blocks the one
  act that repairs it, so the member stays blocked until a person reinstalls it.
- **Treat a shadowed binary as drift.** Rejected: installing changes the copy cargo owns and
  leaves the file that shadows it where it was, so the drift would reappear on every run.
- **Resolve the name against this process's search path.** Rejected under ADR 0017: it is a
  snapshot taken at launch and misses whatever changed since.

## Consequences

- **Planning runs every declared binary once**, asking for its version, and reads the workspace
  once more at each distinct commit those binaries report other than the checkout's.
- **A binary built without the stamp is reinstalled once**, and from then on reports its build.
- **Cargo's install record no longer decides whether a member installs.** It still names which
  binaries a member's earlier installs left behind, which ADR 0041 removes.
- **Every member is unassessable where the registry cannot be read**, since that is where the
  search path comes from. That is every machine that is not Windows.
