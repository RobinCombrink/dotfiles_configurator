use {
    crate::{
        configuration::{DesiredState, Notice, Resource},
        convergence::{Blocked, ChangeSet, converge::converge, plan},
        machine::WriteMachine,
    },
    log::info,
    std::fmt::Display,
};

/// One resource whose convergence failed, together with what went wrong.
#[derive(Debug)]
pub struct Failure {
    pub resource: Resource,
    pub error: anyhow::Error,
}

/// What an apply did, and what it could not do. Failures are collected rather than raised so that
/// one broken resource does not hide the state of every resource after it.
#[derive(Debug)]
pub struct ApplyOutcome {
    pub converged: Vec<Resource>,
    pub failed: Vec<Failure>,
    pub blocked: Vec<Blocked>,
    pub notices: Vec<Notice>,
    pub passes: usize,
}

impl ApplyOutcome {
    /// A machine is converged only when nothing failed and nothing is left unreadable. A run that
    /// ends with resources still unassessable should not imply otherwise.
    pub fn is_converged(&self) -> bool {
        self.failed.is_empty() && self.blocked.is_empty()
    }
}

impl Display for ApplyOutcome {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        for resource in &self.converged {
            writeln!(formatter, "  converged {resource}")?;
        }
        for failure in &self.failed {
            writeln!(
                formatter,
                "  FAILED    {}: {:#}",
                failure.resource, failure.error
            )?;
        }
        for blocked in &self.blocked {
            writeln!(
                formatter,
                "  BLOCKED   {} ({})",
                blocked.resource, blocked.requirement
            )?;
        }
        for notice in &self.notices {
            writeln!(formatter, "  notice    {notice}")?;
        }
        write!(
            formatter,
            "\n{} converged, {} failed, {} still blocked, over {} pass(es)",
            self.converged.len(),
            self.failed.len(),
            self.blocked.len(),
            self.passes
        )
    }
}

/// Enacts a change set, and keeps enacting until a pass changes nothing.
///
/// Apply repeats because readiness is read rather than ordered: converge what is ready, read
/// again, converge what has since become ready. Termination is structural rather than a limit —
/// a pass that converges nothing ends the run, and every productive pass strictly shrinks the set
/// of unconverged resources. See ADR 0004.
pub async fn apply(desired_state: &DesiredState, machine: &impl WriteMachine) -> ApplyOutcome {
    let mut converged: Vec<Resource> = Vec::new();
    let mut failed: Vec<Failure> = Vec::new();
    let mut passes = 0;

    let change_set = loop {
        let change_set: ChangeSet = plan(desired_state, machine);
        passes += 1;

        let attempted = attempt(&change_set, machine, &mut converged, &mut failed).await;
        info!("Pass {passes} converged {attempted} resource(s)");

        if attempted == 0 {
            break change_set;
        }
    };

    ApplyOutcome {
        converged,
        failed,
        blocked: change_set.blocked,
        notices: change_set.notices,
        passes,
    }
}

/// Converges every changed resource in the set, collecting failures instead of stopping at the
/// first, and answers how many actually converged.
async fn attempt(
    change_set: &ChangeSet,
    machine: &impl WriteMachine,
    converged: &mut Vec<Resource>,
    failed: &mut Vec<Failure>,
) -> usize {
    let mut count = 0;
    for change in &change_set.changes {
        // A resource that failed on an earlier pass would otherwise be retried on every pass,
        // and a command without a presence check would never stop being drifted.
        if failed
            .iter()
            .any(|failure| failure.resource == change.resource)
            || converged.contains(&change.resource)
        {
            continue;
        }

        match converge(&change.resource, machine).await {
            Ok(()) => {
                converged.push(change.resource.clone());
                count += 1;
            }
            Err(error) => failed.push(Failure {
                resource: change.resource.clone(),
                error,
            }),
        }
    }
    count
}
