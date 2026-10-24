use {
    crate::{
        configuration::CrateName,
        convergence::{Change, SourceReadings},
        desired_state::ChangeSetResource,
        machine::{WorkspaceBuild, WriteMachine},
        reporting::{Entry, EntryOutcome, RunReport},
    },
    std::{
        collections::{BTreeMap, BTreeSet},
        path::PathBuf,
    },
};

pub fn workspace_builds<'readings, 'change>(
    changes: impl IntoIterator<Item = &'change Change>,
    readings: &'readings SourceReadings,
) -> Vec<WorkspaceBuild<'readings>> {
    let mut changed: BTreeMap<PathBuf, BTreeSet<CrateName>> = BTreeMap::new();
    for change in changes {
        let ChangeSetResource::Member(member) = &change.resource else {
            continue;
        };

        changed
            .entry(member.clone_directory())
            .or_default()
            .insert(member.crate_name().clone());
    }

    changed
        .into_iter()
        .filter_map(|(clone_directory, members)| {
            let reading = readings.resolved_workspace(&clone_directory).ok()?;
            WorkspaceBuild::of(clone_directory, reading, members)
        })
        .collect()
}

pub async fn build(builds: &[WorkspaceBuild<'_>], machine: &impl WriteMachine, report: &RunReport) {
    if builds.is_empty() {
        return;
    }

    machine.reap_builds_of_other_revisions(
        &builds
            .iter()
            .map(|build| build.revision().clone())
            .collect(),
    );
    for build in builds {
        let entry = Entry::workspace_build(build);
        let built = report
            .converging(&entry, machine.build_workspace_members(build))
            .await;
        let Err(error) = built else {
            report.entry_finished(&entry, EntryOutcome::Converged);
            continue;
        };

        let reason = format!(
            "the build of {build} did not finish, so each install builds what it still needs: \
             {error:#}"
        );
        report.note(&reason);
        report.entry_finished(&entry, EntryOutcome::Failed { reason });
    }
}
