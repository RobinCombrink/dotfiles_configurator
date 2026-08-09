use {
    crate::{
        configuration::{DesiredState, Notice, Resource},
        convergence::{
            Blocked, Change, ChangeSet,
            converge::{Convergence, converge},
            plan,
        },
        machine::WriteMachine,
        reporting::RunReport,
    },
    std::{fmt::Display, path::PathBuf},
};

/// One resource whose convergence failed, together with what went wrong.
#[derive(Debug)]
pub struct Failure {
    pub resource: Resource,
    pub error: anyhow::Error,
}

#[derive(Debug)]
pub struct Held {
    pub resource: Resource,
    pub path: PathBuf,
}

/// What an apply did, and what it could not do. Failures are collected rather than raised so that
/// one broken resource does not hide the state of every resource after it.
#[derive(Debug)]
pub struct ApplyOutcome {
    pub converged: Vec<Resource>,
    pub failed: Vec<Failure>,
    pub held: Vec<Held>,
    pub blocked: Vec<Blocked>,
    /// Resources that were converged without error and still read as drifted afterwards — an
    /// installer that exits zero without installing anything looks exactly like this.
    pub unverified: Vec<Change>,
    pub notices: Vec<Notice>,
    pub passes: usize,
}

impl ApplyOutcome {
    /// A machine is converged only when nothing failed, nothing is left unreadable, and every
    /// change that could be read back reads as done. A run that ends otherwise should not imply
    /// the machine is converged.
    pub fn is_converged(&self) -> bool {
        self.failed.is_empty()
            && self.held.is_empty()
            && self.blocked.is_empty()
            && self.unverified.is_empty()
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
        for held in &self.held {
            writeln!(
                formatter,
                "  HELD      {} ({} is being executed)",
                held.resource,
                held.path.display()
            )?;
        }
        for blocked in &self.blocked {
            writeln!(
                formatter,
                "  BLOCKED   {} ({})",
                blocked.resource, blocked.requirement
            )?;
        }
        for change in &self.unverified {
            writeln!(
                formatter,
                "  UNDONE    {} (converged, but still {})",
                change.resource, change.reason
            )?;
        }
        for notice in &self.notices {
            writeln!(formatter, "  notice    {notice}")?;
        }
        write!(
            formatter,
            "\n{} converged, {} failed, {} held, {} still blocked, {} did not take, over {} \
             pass(es)",
            self.converged.len(),
            self.failed.len(),
            self.held.len(),
            self.blocked.len(),
            self.unverified.len(),
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
pub async fn apply(
    desired_state: &DesiredState,
    machine: &impl WriteMachine,
    report: &RunReport,
) -> anyhow::Result<ApplyOutcome> {
    let mut converged: Vec<Resource> = Vec::new();
    let mut failed: Vec<Failure> = Vec::new();
    let mut held: Vec<Held> = Vec::new();
    let mut passes = 0;

    {
        let _doing = report.doing("removing the binaries earlier runs replaced");
        machine.sweep_superseded_images();
    }

    let change_set = loop {
        let change_set: ChangeSet = plan(desired_state, machine, report).await?;
        passes += 1;

        let attempted = attempt(
            &change_set,
            machine,
            report,
            &mut converged,
            &mut failed,
            &mut held,
        )
        .await;
        report.note(&format!("pass {passes} converged {attempted} resource(s)"));

        if attempted == 0 {
            break change_set;
        }
    };

    // The last pass converged nothing, so anything it still reports as drifted was either just
    // converged and did not take, or declares no way to be read back at all.
    let unverified = change_set
        .changes
        .iter()
        .filter(|change| converged.contains(&change.resource) && change.resource.can_be_read_back())
        .cloned()
        .collect();

    Ok(ApplyOutcome {
        converged,
        failed,
        held,
        blocked: change_set.blocked,
        unverified,
        notices: change_set.notices,
        passes,
    })
}

/// Converges every changed resource in the set, collecting failures instead of stopping at the
/// first, and answers how many actually converged.
async fn attempt(
    change_set: &ChangeSet,
    machine: &impl WriteMachine,
    report: &RunReport,
    converged: &mut Vec<Resource>,
    failed: &mut Vec<Failure>,
    held: &mut Vec<Held>,
) -> usize {
    let mut count = 0;
    for change in &change_set.changes {
        // A resource that failed on an earlier pass would otherwise be retried on every pass,
        // and a command without a presence check would never stop being drifted.
        if failed
            .iter()
            .any(|failure| failure.resource == change.resource)
            || held.iter().any(|entry| entry.resource == change.resource)
            || converged.contains(&change.resource)
        {
            continue;
        }

        let outcome = {
            let _doing = report.doing(format!("converging {}", change.resource));
            converge(&change.resource, machine, &change_set.readings).await
        };

        match outcome {
            Ok(Convergence::Converged) => {
                report.note(&format!("converged {}", change.resource));
                converged.push(change.resource.clone());
                count += 1;
            }
            Ok(Convergence::Held(path)) => {
                report.note(&format!(
                    "HELD {}: {} is being executed",
                    change.resource,
                    path.display()
                ));
                held.push(Held {
                    resource: change.resource.clone(),
                    path,
                });
            }
            Err(error) => {
                report.note(&format!("FAILED {}: {error:#}", change.resource));
                failed.push(Failure {
                    resource: change.resource.clone(),
                    error,
                });
            }
        }
    }
    count
}
