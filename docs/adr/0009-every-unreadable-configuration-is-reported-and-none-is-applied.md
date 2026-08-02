# Every unreadable configuration is reported, and none is applied

Status: accepted (2026-08-02, session on diagnosing a multi-configuration run)

A run reads several configurations — a machine takes the generic one plus the one for its class,
each from its own repository. When one of them cannot be read, two things follow.

**No configuration is applied.** The desired state is the merge of all of them, so a configuration
that could not be read leaves a desired state nobody wrote: resources missing, and the machine
settings possibly taken from a configuration that was never meant to supply them alone. Converging
towards that is worse than converging towards nothing, because a person reading the change set has
no way to see which declarations are absent. A run that cannot read every configuration it was
given therefore reports and exits non-zero without reading the machine at all.

**Every fault is reported, not the first.** Stopping at the first fault costs a person one run per
broken configuration, and each run pays the full cost of fetching from GitHub again. The faults are
independent — nothing about one configuration's shape changes how another parses — so there is no
cascade to suppress, and reporting them together is strictly more information for the same work.
Each is named by the configuration it came from and by the path within it that failed, so neither
the file nor the field has to be guessed.

## Considered options

- **Apply what could be read, report the rest as failures.** Rejected on the reasoning above: a
  partial desired state is not a smaller version of the intended one, it is a different one, and
  convergence towards it silently withdraws nothing while silently omitting everything the
  unreadable configuration declared.
- **Stop at the first unreadable configuration.** This is what the tool did, not as a decision but
  as the shape `?` gives an early return. Rejected: it turns diagnosing two broken configurations
  into two runs, and for a GitHub source the later ones are not even fetched, so a person cannot
  learn whether the second is also broken without first fixing the first.
- **Name the file only, and leave the fault to be found by reading it.** This was the issue's
  stated minimum. Rejected because the extra reach is one dependency: `serde_path_to_error` wraps
  the same deserialize and yields `resources[1]: unknown variant` where `serde_json` alone yields
  `unknown variant`. A configuration holding thirty resources makes that difference the whole
  diagnosis.

## Consequences

- This governs reading a configuration, not reconciling one against another. Conflicting claims
  between configurations are still reported one at a time, because a conflict is a fact about a
  pair rather than about a document: resolving one can remove or create others, so they lack the
  independence that makes gathering read failures sound.
- A GitHub source fetches every file path it names even when an earlier one is already known to be
  unreadable. That is the cost of reporting them together, and it is paid only on a run that is
  going to fail anyway.
- A single fault is reported as it always was, unwrapped. Only two or more are gathered under a
  count, so the common case gains no ceremony.
- `serde_path_to_error` is a build dependency as well as an ordinary one, because the build script
  generates the schema by including the module that parses configurations.
