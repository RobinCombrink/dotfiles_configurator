use {
    crate::{
        configuration::Identity,
        convergence::assess::SourceReadings,
        desired_state::{ChangeSetResource, DesiredState},
    },
    anyhow::{Result, bail},
    std::{collections::BTreeMap, path::Path},
};

pub fn resolve(
    desired_state: &DesiredState,
    readings: &SourceReadings,
    home_directory: &Path,
) -> Result<Vec<ChangeSetResource>> {
    let mut resources: Vec<ChangeSetResource> = desired_state
        .resources
        .iter()
        .cloned()
        .map(ChangeSetResource::Declared)
        .collect();

    for workspace in &desired_state.workspaces {
        let repository = &workspace.declared().repository;
        match readings.workspace(&workspace.clone_directory(repository)) {
            Ok(Some(reading)) => resources.extend(
                workspace
                    .members(reading)
                    .into_iter()
                    .map(ChangeSetResource::Member),
            ),
            Ok(None) => {}
            Err(impediment) => bail!(
                "{} could not be read, so which crates it holds is unknown: {impediment}. Nothing \
                 was applied.",
                workspace.declared()
            ),
        }
    }

    reject_conflicting_claims(&resources, home_directory)?;
    Ok(resources)
}

fn reject_conflicting_claims(resources: &[ChangeSetResource], home_directory: &Path) -> Result<()> {
    let mut claimed: BTreeMap<Identity, &ChangeSetResource> = BTreeMap::new();

    for resource in resources {
        let Some(identity) = resource.identity(home_directory) else {
            continue;
        };

        match claimed.get(&identity) {
            None => {
                claimed.insert(identity, resource);
            }
            Some(existing) if **existing == *resource => {}
            Some(existing) => bail!(
                "Resolving the declared workspaces made conflicting claims on {identity}. No \
                 machine could satisfy both:\n  {existing}\n  {resource}"
            ),
        }
    }

    Ok(())
}
