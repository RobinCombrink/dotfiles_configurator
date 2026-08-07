# dotfiles_configurator

Brings a machine into the state a declarative configuration describes — programs present,
dotfiles linked, repositories cloned — by reading what is already true and changing only
what is not.

## Language

### Convergence

**Desired state**:
What the configuration determines should be true of a machine. Usually declared outright; a
resource kind may instead resolve it when the change set is produced, from a source the
configuration names. Never resolved while applying.
_Avoid_: target state, wanted state, spec

**Actual state**:
What is observably true of the machine right now, read from the machine itself rather than
from any record of past runs.
_Avoid_: current state, real state, live state

**Drift**:
A difference between the desired state and the actual state of a single resource.
_Avoid_: delta, divergence, mismatch

**Change set**:
The ordered set of changes that would close every drift. Produced by comparing desired
against actual, and inspectable without being enacted.
_Avoid_: plan, diff, execution plan

**Apply**:
To enact a change set. An apply that finds no drift changes nothing.
_Avoid_: execute, run, provision

**Converged**:
Describes a machine with no drift. Applying to a converged machine is a no-op, which is what
makes repeated runs safe. Convergence is additive: it makes declared things true and never
makes undeclared things false, so withdrawing a declaration ends the tool's interest in a
resource rather than undoing it.
_Avoid_: in sync, up to date, provisioned

### Resources

**Resource**:
One declared fact about a machine, whose actual state can be read and whose drift can be
closed. The unit a change set is composed of.
_Avoid_: item, entry, task, step

**Resolved**:
Describes a resource the configuration named through a source rather than individually,
discovered by reading that source when the change set is produced. Once the set is complete, a
resolved resource is indistinguishable from a declared one.
_Avoid_: generated, derived, implicit, expanded

**Resource kind**:
The category a resource belongs to, which determines how its actual state is read. Symlink,
repository, application, package, registration, notice, and command.
_Avoid_: type, category, variant

**Identity**:
The machine fact a resource claims, by which two declarations are recognised as the same
resource. Identical claims collapse to one resource; conflicting claims on one fact are
rejected before any change is enacted, because no machine could satisfy both. A command
claims no fact and so has no identity, which is part of what makes it the last resort.
_Avoid_: key, id, name

**Package**:
A resource whose installation is owned by a package manager, which is consequently also what
can be asked whether it is installed. Which manager owns it is part of what the resource is,
not a setting on it.
_Avoid_: dependency, library, install

**Tool**:
A program a resource kind needs in order to read or converge a resource. A tool's presence is
probed on the machine, never declared, so a tool installed by hand counts exactly as much as
one this tool installed. The same program can be a tool to a resource that needs it and a
package to the resource that installs it — those are different roles, not a contradiction.
_Avoid_: dependency, prerequisite, requirement

**Readiness**:
Whether a resource's tools are present, and therefore whether it can be read or converged yet.
Read from the machine each time rather than inferred from order, so a resource blocked now may
become ready once something else has been applied.
_Avoid_: available, satisfied, unblocked

**Presence check**:
An author-declared test that establishes whether a resource is already in its desired state,
used where the machine cannot be asked directly. Chosen from a fixed set of forms rather than
written as arbitrary shell, so that most checks cannot change the machine they are asking
about. Required on an application; optional on a command, and a command without one has drift
on every run.
_Avoid_: guard, precondition, unless, creates

**Notice**:
A message for a person about something the tool cannot do. Not a resource — it never
converges, is reported by every change set, and is never applied.
_Avoid_: reminder, warning, message

**Command**:
An arbitrary shell invocation, and the resource kind of last resort. Deliberately retained as
the place new capabilities first appear: a capability is expressed as a command until enough
instances accumulate to justify promoting it to a resource kind of its own.
_Avoid_: script, hook, action

### Configuration

**Configuration**:
One file declaring part of a desired state, and the machines it is for. Several are read together
and merged. A configuration is self-describing: it says which machines it applies to, so nothing
outside it decides whether it is read.
_Avoid_: manifest, profile, layer, tier

**Context**:
Which machine something is — `everywhere`, `personal` or `work`. An invocation names the one its
machine is; a configuration declares the one it applies to, and applies when it declares
`everywhere` or the machine the invocation named. One value rather than a set, so naming none and
naming two that describe different machines are unexpressable rather than rejected.
_Avoid_: machine class, environment, mode, tier

**Configuration source**:
Where configurations are read from — a local directory, or a directory in a GitHub repository. Every
`*.dotconfig.json` directly in it is read, subdirectories are not descended into, and it determines
only where: which configurations apply is decided by the context each one declares.
_Avoid_: config location, provider, backend

**Dotfile**:
A configuration file or directory owned by the dotfiles repository and linked into place on
the machine, rather than copied.
_Avoid_: config file, rc file, setting

**Dotfiles repository**:
The repository holding the dotfiles themselves, cloned before any dotfile resource can
converge.
_Avoid_: config repo, source repo
