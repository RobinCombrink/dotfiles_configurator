# The configurator updates itself from an origin it carries

Status: accepted (2026-08-07, design session on self-provisioning). Refines ADR 0008 and ADR 0009.

The program carries where its own releases come from, and its currency is a resolved resource
present in every change set. Plan reports it, apply converges it, and no configuration declares
it. This is what applications do: the ones this tool installs ship their own updater, and knowing
where you came from is part of being a program rather than a fact about one deployment.

The origin is already carried. The default configuration source names a repository, so a build
that has been handed no arguments already knows where to look; carrying the release origin
alongside it adds no new class of fact to the binary.

Carrying it is also what keeps the fact in one place. A configuration declaring the origin as well
would state it twice, in two repositories, with nothing to keep them agreeing — and unlike two
copies of prose, two copies of a repository name are executable in different directions.

It is what lets a generation floor be escaped. A configuration stating a generation above the
build cannot be read, and a resource declaring where a newer build comes from would be inside it,
so a declared origin makes the deadlock unbreakable by construction. A carried origin needs
nothing read. Apply obtains a newer build once, then re-runs; if the newest release still does not
meet the floor, the run ends saying so rather than trying again. Plan reports and changes nothing,
so ADR 0006 stands: only apply changes a machine.

Refusing to read therefore has two causes with two closures, where ADR 0009 has one. A
**malformed** configuration is a fault in the repository it was read from, and a person resolves
it. A **too new** configuration is a fault in the build reading it, and the program resolves it.
ADR 0009 is unchanged in what it refuses — every unreadable configuration is reported and none is
applied — and gains a distinction in what happens next.

## Considered options

- **A stable envelope naming the origin inside the configuration**, unversioned and always
  parseable, with the versioned body inside it. It keeps every fact in the configuration where the
  model otherwise puts them. Rejected because the envelope's shape would be frozen permanently: it
  is the one part of the document no migration can ever repair, since it is what a build that
  cannot read the document must still read.
- **An opt-in self reference**, declared by a configuration without an origin, which the program
  supplies. It preserves pinning and declining. Rejected because it does not survive the case the
  mechanism exists for: an opt-in inside an unreadable document is not read.
- **No self-update, with the bootstrap document as the recovery path.** Zero new machinery, and
  that document is reachable from a machine with nothing working. Rejected because it leaves the
  one thing this design exists to automate — keeping current the tool that keeps everything else
  current — requiring a person at precisely the moment it has stopped working.

## Consequences

- **There is no pinning and no opting out.** A build pointed at a local source for testing still
  converges its own currency, and a machine cannot hold a version deliberately.
- **A fork updates to the origin its build carries**, which is the origin it was built from rather
  than the fork's own, unless the fork changes it.
- **The first arrival on a machine is still out of band.** Declaring nothing changes what ADR 0008
  already records: nothing is running yet to make the first fetch automatic, and the bootstrap
  document lives in this repository because it is the only one reachable before anything is
  authenticated.
- **The run that upgrades continues on the image it started with**, as ADR 0008 records.
