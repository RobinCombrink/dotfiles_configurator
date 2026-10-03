use {
    crate::{
        configuration::{CargoPackage, CargoSource, CrateName, Package, Resource},
        convergence::{Change, SourceReadings},
        machine::{WorkspaceBuild, WriteMachine},
        reporting::RunReport,
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
        let Resource::Package(Package::Cargo(CargoPackage {
            crate_name,
            source: CargoSource::Workspace { repository },
        })) = change.resource.declared()
        else {
            continue;
        };

        changed
            .entry(change.resource.clone_directory(repository))
            .or_default()
            .insert(crate_name.clone());
    }

    changed
        .into_iter()
        .filter_map(|(clone_directory, members)| {
            let reading = readings.resolved_workspace(&clone_directory).ok()?;
            WorkspaceBuild::of(clone_directory, reading, members)
        })
        .collect()
}

pub fn build(builds: &[WorkspaceBuild<'_>], machine: &impl WriteMachine, report: &RunReport) {
    for build in builds {
        let _doing = report.doing(format!("building {build}"));
        if let Err(error) = machine.build_workspace_members(build) {
            report.note(&format!(
                "the build of {build} did not finish, so each install builds what it still \
                 needs: {error:#}"
            ));
        }
    }
}
