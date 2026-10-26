use {
    crate::{
        configuration::GitHubAccount,
        configuration_source::load_desired_state,
        confirmation::{Confirm, Confirmation},
        convergence::install_release,
        currency::{RELEASE_OWNER, own_currency, own_release_repository, this_build},
        desired_state::DesiredState,
        github::GitHubAccess,
        machine::{HeldReason, Placement, WriteMachine},
        planned_run::PlannedRun,
        reporting::RunReport,
        version::Version,
    },
    anyhow::{Result, anyhow, bail},
    std::{fmt::Display, path::Path},
};

pub fn still_unreadable_by_the_newest_build(refusal: &impl Display, replaced: &Version) -> String {
    format!(
        "{refusal}
{} replaced {replaced} earlier in this run and cannot read it either, so no release of {} does \
         yet.",
        this_build(),
        own_release_repository()
    )
}

#[derive(Debug)]
pub enum Loaded {
    Read(DesiredState),
    DeclinedTheNewerBuild,
    ObtainedANewerBuild,
}

const OBTAIN_A_NEWER_BUILD: &str = "Obtain the newer build and read again?";

// ADR 0019
pub async fn load_after_updating_if_it_must(
    replaced: Option<&Version>,
    applied: &PlannedRun,
    machine: &impl WriteMachine,
    repositories_root: &Path,
    github: &GitHubAccess,
    report: &RunReport,
    operator: &impl Confirm,
) -> Result<Loaded> {
    let loaded =
        load_desired_state(&applied.sources, applied.machine, repositories_root, github).await;
    let refusal = match loaded {
        Ok(desired_state) => return Ok(Loaded::Read(desired_state)),
        Err(refusal) => refusal,
    };

    if !refusal.is_answered_by_a_newer_build() {
        return Err(refusal.into());
    }

    if let Some(replaced) = replaced {
        bail!(
            "{}",
            still_unreadable_by_the_newest_build(&refusal, replaced)
        );
    }

    report.announce(&format!(
        "{refusal}
A newer build from {} can read it, and installing it replaces the one running now.",
        own_release_repository()
    ));

    if operator.confirmation(OBTAIN_A_NEWER_BUILD) == Confirmation::Declined {
        report.announce("Declined. Nothing on this machine was changed.");
        return Ok(Loaded::DeclinedTheNewerBuild);
    }

    obtain_a_newer_build(machine).await?;
    Ok(Loaded::ObtainedANewerBuild)
}

async fn obtain_a_newer_build(machine: &impl WriteMachine) -> Result<()> {
    let binary = own_currency();
    let released = machine
        .latest_release(&binary.repository, &GitHubAccount::from(RELEASE_OWNER))
        .await?
        .ok_or_else(|| anyhow!("{} has published no release", binary.repository))?;

    match install_release(&binary, &released, machine).await? {
        Placement::Placed => Ok(()),
        Placement::Held(HeldReason::BeingExecuted(path)) => bail!(
            "{} is running and could not be moved aside to install the newer build",
            path.display()
        ),
        Placement::Held(
            reason @ (HeldReason::ElevationDeclined
            | HeldReason::ReportedInUse(_)
            | HeldReason::UpgradedByItsPublisher),
        ) => {
            bail!("The newer build could not be installed: {reason}")
        }
        Placement::Refused(refusal) => bail!("{}", refusal.refusal),
    }
}

#[cfg(test)]
mod tests;
