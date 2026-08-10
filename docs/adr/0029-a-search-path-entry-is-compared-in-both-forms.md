# A search path entry is compared in both the form it is stored and the form it stands for

Status: accepted (2026-08-10, review of making an environment variable a resource)

ADR 0017 requires that actual state be read from the registry and never from this process's
environment, which is a snapshot taken at launch. Membership cannot be decided on the stored form
alone: an entry is stored as `%USERPROFILE%\.cargo\bin` and a resource resolves to
`C:\Users\alice\.cargo\bin`, and those are the same directory written two ways. Something has to
resolve one into the other.

Nothing in the registry can. `Path` lives under the user's environment key, but the references its
entries name do not: `USERPROFILE` is placed in the environment block by the session that logs a
person in, and `SystemRoot` by the system, and neither is stored beside the value that names them.
Resolving a reference from the registry alone would leave the entries this machine actually holds
unresolvable.

So a hive contributes each entry twice — once exactly as stored, once as this process expands it —
and a directory is carried when it matches either. Expansion reads the process environment, which
ADR 0017 excludes, and the exclusion holds where it was aimed: the *search path* is still never
read from the process, and the value written back is still the stored form, unexpanded and
untouched. What is borrowed from the process is only the meaning of a reference.

## Considered options

- **Expanding from the registry alone.** Keeps ADR 0017 whole and cannot resolve `%USERPROFILE%`,
  which every entry on the measured machine depends on.
- **Comparing the stored form only.** No expansion anywhere, and a resource resolving to a real
  path never matches an entry stored as a reference, so every such directory reads as drifted
  forever.
- **Comparing the expanded form only.** Loses a reference this process cannot resolve, which then
  matches nothing and reads as drifted.
- **Resolving the resource's directory back into a reference before comparing.** Requires knowing
  which of several references a path should be written as, and two of them nest.

## Consequences

- **A reference whose value moved since this process launched expands stale.** The entry then
  matches nothing and the resource reads as drifted, so a run puts the directory on the path
  again — and finds it already there, because the write is a membership postcondition rather than
  an append. The cost is a redundant write, never a duplicate entry.
- **The comparison is wider than either form alone.** A directory matches when it equals the stored
  text or the expanded text, so a path that happens to equal an unexpanded reference is carried.
  No such path exists, because `%` is not a legal character in a Windows path component.
