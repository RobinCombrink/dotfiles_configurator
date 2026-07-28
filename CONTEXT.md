# dotfiles_configurator

Brings a machine into the state a declarative configuration describes — programs present,
dotfiles linked, repositories cloned — by reading what is already true and changing only
what is not.

## Language

### Convergence

**Desired state**:
What the configuration declares should be true of a machine.
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
makes repeated runs safe.
_Avoid_: in sync, up to date, provisioned

### Resources

**Resource**:
One declared fact about a machine, whose actual state can be read and whose drift can be
closed. The unit a change set is composed of.
_Avoid_: item, entry, task, step

**Resource kind**:
The category a resource belongs to, which determines how its actual state is read. Symlink,
repository, application, binary from source, registration, notice, and command.
_Avoid_: type, category, variant

**Presence check**:
An author-declared test that establishes whether a resource is already in its desired state,
used where the machine cannot be asked directly. Optional on a command; a command without one
has drift on every run.
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

**Configuration source**:
Where a configuration is read from — a local directory, or a path within a GitHub repository.
_Avoid_: config location, provider, backend

**Dotfile**:
A configuration file or directory owned by the dotfiles repository and linked into place on
the machine, rather than copied.
_Avoid_: config file, rc file, setting

**Dotfiles repository**:
The repository holding the dotfiles themselves, cloned before any dotfile resource can
converge.
_Avoid_: config repo, source repo
