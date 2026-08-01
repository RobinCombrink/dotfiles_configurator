# The configurator installs itself from its own release

Status: accepted (2026-08-01, grilling session on automatic convergence)

The configurator is declared like anything else it manages: an application whose source is the
latest release of its own repository. Its actual state is the version installed, read from the
binary, rather than whether a binary is present at all.

Cargo cannot be the route. This tool installs Rust, so installing it with Cargo requires Rust to
already be there, and the documented first step on a new machine becomes the very thing the tool
exists to do. A release asset inverts nothing: the machine fetches one file, and that file then
provisions everything including the toolchain.

Presence cannot be the reading. A check for the binary on the search path passes forever once it
has passed once, so the tool would install on a fresh machine and never update again. A check for
a literal version string is the copied derived value ADR 0007 exists to remove, one release
behind from the moment it is written. The release already carries the version and the build
already stamps it, so the comparison is between two things the machine can be asked for.

## Considered options

- **Install from the repository with Cargo**, exactly as the workspace tools are installed. The
  uniformity is real and it is why this was considered first. Rejected for the bootstrap
  inversion above, and because it puts a full dependency-tree build on every machine and every
  upgrade where a release asset is one download.
- **Declare it with a presence check only.** Cheapest, and it solves a fresh machine. Rejected
  because it solves only that: the tool that keeps everything else current would be the one thing
  that never updates.
- **Leave it undeclared.** The path to a configured machine then requires knowing, out of band,
  to fetch a tool the configuration never mentions — and the tool that reads the configuration is
  the worst possible thing for the configuration to omit.

## Consequences

- **Replacing the running binary requires renaming it aside first.** Measured on Windows,
  2026-08-01: copying over a running executable fails, renaming over it fails, and renaming the
  running executable aside succeeds. An installer that writes in place will fail every self
  upgrade.
- **An upgrade takes effect on the following run.** The run that performs it continues on the
  image it started with, which is the ordinary meaning of replacing a running program rather than
  a defect to work around.
- The first machine still bootstraps out of band. Declaring the tool makes every subsequent
  machine and every subsequent version automatic; it cannot make the first arrival automatic,
  because nothing is yet running to make it so.
