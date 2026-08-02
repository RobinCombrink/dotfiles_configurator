use {
    crate::{
        configuration::{CargoPackage, CargoSource, DesiredState, Identity, Package, Resource},
        convergence::assess::SourceReadings,
    },
    anyhow::{Result, bail},
    std::collections::BTreeMap,
};

pub fn resolve(desired_state: &DesiredState, readings: &SourceReadings) -> Result<Vec<Resource>> {
    let mut resources = desired_state.resources.clone();

    for workspace in &desired_state.workspaces {
        match readings.workspace(&workspace.repository) {
            Some(Ok(Some(reading))) => {
                for crate_name in reading.members.keys() {
                    resources.push(Resource::Package(Package::Cargo(CargoPackage {
                        crate_name: crate_name.clone(),
                        source: CargoSource::Workspace {
                            repository: workspace.repository.clone(),
                        },
                    })));
                }
            }
            Some(Ok(None)) | None => {}
            Some(Err(reason)) => bail!(
                "{workspace} could not be read, so which crates it holds is unknown: {reason}. \
                 Nothing was applied."
            ),
        }
    }

    reject_conflicting_claims(&resources)?;
    Ok(resources)
}

fn reject_conflicting_claims(resources: &[Resource]) -> Result<()> {
    let mut claimed: BTreeMap<Identity, &Resource> = BTreeMap::new();

    for resource in resources {
        let Some(identity) = resource.identity() else {
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
