# Workspace membership and revision are resolved, not declared

Status: accepted (2026-08-01, grilling session on automatic convergence); amended 2026-08-02 on
what an unresolvable workspace does to a run

A cargo package built from a repository this configuration already has cloned declares the
workspace it belongs to. Which crates exist, and which revision each installs from, are read from
that workspace rather than written down beside it.

A declared revision is a copy of something the repository already holds — the crate's content —
and the copy rots. Measured on 2026-08-01 against four declared crates: two named content older
than the repository's own, and all four reported converged, because the comparison ran between
the declared revision and the installed one and never reached the repository at all. dotfiles ADR
0007 describes the subtree comparison that would have caught it; this decision is that comparison
existing.

Only a crate whose repository this configuration has cloned can be declared this way, because
only then is its revision derivable. A third-party crate from git cannot: nothing local answers
what its content should be, and installing whatever its default branch points at is not
reproducible. That case has no instances, so under ADR 0001 it stays a command until it earns a
kind, and the registry covers the ordinary way to install someone else's crate.

## Considered options

- **One resource for the whole workspace**, reinstalling every member whenever any of them
  changes. Simpler, and it makes a new crate need no edit at all. Rejected on cost: installing
  from git rebuilds from a clean checkout, and dotfiles ADR 0007 measured `ci-checks` cold at
  6m23s against `stop-gate`'s 11s, so an eleven-second change would drag six and a half minutes
  behind it on a pull. Per-crate fingerprints keep that granularity; membership is still resolved
  from the workspace, so adding a crate still needs no edit.
- **Keep the declared revision and report when it is stale.** Closes the gap between the decision
  and the code without removing the copy, so the value can still be wrong — only now it announces
  itself. Rejected as fixing the symptom.
- **Derive the revision as the repository's current commit.** Every commit would then make every
  member drift and reinstall, which is precisely what comparing content rather than commits
  exists to prevent.
- **Keep a general git source alongside the resolved one**, so a third-party crate stays
  expressible. Rejected: it retains the shape that produced the stale revisions, in order to serve
  no declaration that exists.

## Consequences

- **Desired state is the crate's content at the tracked remote branch, not at the local commit.**
  An unpushed commit is not installable, so treating it as desired would produce drift no run
  could close. This makes the reading of dotfiles ADR 0007's "committed and pushed" into
  something the model enforces rather than something an author remembers.
- **A crate whose installed commit is absent from the local clone cannot be assessed.** Cargo
  records the commit it resolved; comparing content requires that commit locally. Where it is
  missing — installed elsewhere, or force-pushed away — the crate reports as unreadable rather
  than converged.
- **Membership is resolved, so it requires the repository.** On a machine that has not cloned it
  yet, the crates are blocked until it is, and readiness already expresses this.
- Adding a crate to the workspace declares it; removing one withdraws it. Withdrawal ends the
  tool's interest without uninstalling anything, per ADR 0005.
- The fingerprint covers the crate's own subtree together with the workspace manifest and
  lockfile, because a dependency change alters the built binary without touching the crate.
- **A workspace that cannot be read refuses the whole run.** Cloned but with no tracked remote
  branch, an unparseable manifest, a member named by a glob, or no lockfile: each leaves which
  crates exist unknown, and a source that decides which resources exist cannot fail softly the way
  a source that describes one resource can. Reporting drift instead would apply a change set built
  from a membership nobody established. ADR 0009 already refuses to apply any configuration it
  could not read; this is that rule reaching the resolved half of the configuration.
- **A workspace whose repository is not cloned contributes no members, and says nothing about
  them.** They cannot be named, so they cannot be reported. The clone is itself a declared
  repository resource and appears in the change set as a change, so a first run on a bare machine
  still reads correctly and the crates arrive on the pass after the clone. The cost is that
  planning alone, on a machine that has not cloned yet, understates how much work an apply will do.
- **Membership is the members that build a binary**, decided from the manifest's `[[bin]]` sections
  and from whether the tree holds `src/main.rs` or `src/bin/`. A library-only member cannot be
  installed at all, so admitting one would fail on every run forever rather than once; adding a
  shared library crate to a workspace is ordinary, and it costs nothing here.
