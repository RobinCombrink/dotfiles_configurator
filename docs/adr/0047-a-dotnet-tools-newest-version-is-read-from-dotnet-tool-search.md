---
status: accepted
---

# A .NET tool's newest version is read from `dotnet tool search`

A .NET global tool declared without a version is kept at the newest release NuGet publishes, so its
assessment needs two facts: the version installed, and the newest one published. The first is read
once per change set from `dotnet tool list --global`, as ADR 0010 requires of a source answering for
a whole set. The second has no whole-set reader: the SDK has no outdated listing and no dry-run
update, so it is read per tool from `dotnet tool search <id>`, and only for tools declaring no
version. A tool installed at or above that version is converged, and one below it is updated with
`dotnet tool update --global <id>`, which also installs a tool dotnet does not hold.

The search matches its term against package ids and descriptions alike, so the newest version is the
one on the row whose package id equals the declared id, compared without regard to case as NuGet
compares ids. A search listing no such row leaves the tool unassessable rather than converged.

Measured with .NET SDK 10.0.102 on Windows 11, against csharp-ls installed at 0.28.0 with 0.29.0 on
NuGet:

| Reader | Answer | Cost |
| --- | --- | --- |
| `dotnet tool search csharp-ls` | `csharp-ls 0.29.0` among thirteen rows, including `csharp-ls-vs 1.2.5` | 2.2 s to 3.2 s per tool over four runs (2026-11-23); 2.4 s per tool (2026-11-17) |
| `dotnet tool list --global` | `csharp-ls 0.28.0` | 1.1 s for every tool (2026-11-23) |
| `dotnet tool update` with a dry run | no such option in its help | — |

A declared version is passed to the same update as `--version <version> --allow-downgrade`, which
installs, upgrades or downgrades to it; without `--allow-downgrade` a version below the installed one
is refused with exit 1 (measured 2026-11-23 in a scratch tool path).

## Considered options

- **NuGet's flat container index.** One request answered in 0.5 s (2026-11-23), a fifth of the
  search. Rejected because it reads nuget.org directly rather than the feeds dotnet is configured
  with, and needs an HTTP client in the read path where every other package manager is asked through
  its own command line.
- **Reading the search once for all tools.** The search takes one term, so tools sharing no
  substring cannot be answered by one launch.

## Consequences

- **Each tool declaring no version costs one search per change set**, about two and a half seconds,
  so a configuration declaring many such tools plans slowly in proportion. A pinned tool costs no
  search.
- **A prerelease is never read as the newest version**, since the search includes prereleases only
  when passed `--prerelease`.
- **A tool's executable running while it is updated is reported as a failure**, since nothing has
  measured what the update does then.
