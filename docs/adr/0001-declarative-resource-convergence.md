# Configuration is a declared desired state, applied as a change set

Status: accepted (2026-07-28, grilling session following the multi-angle repo review)

Configuration declares the **desired state** of a machine as a set of **resources**. The tool
reads each resource's **actual state** from the machine, compares the two to produce a
**change set**, and applies only that. Idempotency is therefore not a property implemented per
item kind — it is what remains when nothing has drifted. The configuration format takes a
breaking revision to carry this, and `Configuration.version`, which until now was parsed and
discarded, becomes the gate that rejects the old shape with a real message.

The move that makes it work is that *installed* stops being a filesystem question. The
previous model asked whether `steam_installer.exe` was present in the download directory,
which answers nothing about whether Steam is installed; a resource is instead asked for its
own presence, by whatever means suits its kind.

## Considered options

- **Fix idempotency per item kind**: the obvious reading of the review's findings — make
  symlinking tolerate an existing link, make cloning check out, stop re-running installers.
  Rejected: four unrelated patches, each re-deriving "have I already done this?" in its own
  way, and no answer at all for the question of what the tool would *do* on the next run.
- **A recorded receipt of past applies** (Terraform's state file): actual state falls back to
  what the tool wrote down last time. Gives an answer for anything unobservable with no author
  effort. Rejected: the receipt can lie — uninstall something by hand and the tool still
  believes it is there — and it becomes a file that must not be lost and that drifts per
  machine. The machine stays the single source of truth.
- **Package-manager-first applications** (winget ids instead of installer downloads): would
  have made the largest group of resources exactly observable and deleted the download,
  resume, and asset-matching code entirely. Rejected: the long tail with no package id still
  needs a second path, and the download defects are worth fixing rather than routing around.
- **Observation only, unobservable kinds always drift**: simplest and most honest, but every
  command would run on every apply, which makes an automatic apply either noisy or dangerous.

## Consequences

- **Applications keep download-and-run and gain a declared presence check.** The download
  defects the review found — the asset-matching `unwrap`, the resume arithmetic that appends a
  duplicate byte, the unauthenticated `octocrab::instance()` — remain real work rather than
  being deleted along with the code path.
- **Commands remain, as the escape hatch, deliberately.** Every command in the live
  configuration turned out to be a proto-resource: package installs, binaries built from
  source, a registration in another tool's config, and reminders to a person. That is the
  pipeline by which resource kinds are discovered, and closing it would stop new capabilities
  appearing. A command may declare a presence check; one without a check has drift on every
  run, which is the honest default for something not yet understood.
- **Notices are not resources.** The reminders currently expressed as interactive `echo`
  commands stop pretending to be work.
- **Ordering becomes load-bearing.** A change set is ordered, so the `HashMap` that currently
  determines execution order has to go — it also made dry-run output unstable and therefore
  undiffable, which a change set cannot be.
- A resource kind is only as good as its state reader. Adding a kind means answering "how is
  this read back?" first; a kind that cannot answer belongs as a command until it can.
