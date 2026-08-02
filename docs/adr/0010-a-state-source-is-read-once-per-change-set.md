# A state source is read once per change set

Status: accepted (2026-08-02)

A source that can answer about a whole set of resources at once is interrogated once while a
change set is produced, before any resource is assessed. A source that cannot is interrogated per
resource, and says which it is in the invocation that names it.

The cost of a change set is then bounded by the number of sources it touches rather than the
number of resources declared against them.

Measured 2026-08-02 on `20-THREADS`, warm, three runs of `dotfiles plan` against a configuration
of 44 resources declaring 3 winget packages and 6 cargo crates: **3.16s before, 1.45s after**
(medians). The saving is seven process launches removed — two winget and five cargo — which agrees
with what their startup costs predict, winget at ~0.85s each and cargo at ~0.05s. An earlier
reading on 2026-08-01, of a larger configuration, put the same shape at 4.8s against a floor of
~1.2s.

`cargo install --list` already answered every crate at once and was still run once per crate, so
the waste grew with the manifest rather than staying fixed.

This matters because of what reads the change set. The triggers that converge after a pull run
unattended and often, so a cost that scales with the manifest is paid on every commit by a person
who did not ask for it.

## Considered options

- **A cache inside the machine, keyed by invocation.** The smallest change: assessment and change
  set production would both be untouched. Rejected on two counts. It needs interior mutability in
  a type whose whole purpose is to be the read-only half of the capability split ADR 0006 rests
  on. And it makes the property invisible — nothing would stop a later resource kind being read
  per resource, because there would be no place where reading once is stated. A cache also cannot
  be held across an apply, since installing something makes it wrong.
- **A reader abstraction per source**, with a whole-set method and a declared per-resource
  fallback. This is the shape the property eventually wants. Rejected for now as machinery ahead
  of its instances: it would have two implementations and one fallback, and the distinction it
  would encode is already carried by whether an invocation names a resource.
- **Leave winget interrogated per package.** Its filter is free — `winget list` and `winget list
  --id <one>` both take about 0.85s, because process startup dominates — so only the process count
  matters, and closing cargo alone would leave the larger of the two gaps open.

## Consequences

- **A source is read only when something declares against it.** A configuration with no cargo
  package never launches cargo, so the floor for a manifest is the sources it actually uses.
- **A source whose tool is absent is not read at all.** Readiness is established before a source is
  consulted, so a machine without winget reports its packages blocked without a failed spawn, as
  ADR 0004 already required of assessment.
- **One unreadable source is one failure reported against every resource that needed it.** The
  reason is held with the reading rather than re-derived, so a change set says the same thing about
  each of them instead of interrogating a broken tool once per resource to find out.
- **Matching moves out of winget and into this program.** winget publishes no machine-readable
  listing, so its `Id` column is located from the header row. Two consequences follow, and both
  fail loudly rather than reporting a package as missing: a listing whose columns cannot be located
  is unreadable, which is what a machine reporting them in another language would produce; and a
  listing that cut an identifier short is unreadable too. Measured 2026-08-02: winget sizes the
  table to the data, not to the console, whenever its output is redirected — at an 80-column
  console the redirected output was still 196 characters wide with nothing cut short — and
  redirected is the only way this program runs it.
- **The Claude MCP registration stays a per-resource read.** `claude mcp list` is not a cheaper
  form of asking about each server: measured 2026-08-02, it health-checks every server it reports,
  connecting to each one. It costs more than the reads it would replace and reaches the network
  while a change set is being produced, which is not something producing one should do.
