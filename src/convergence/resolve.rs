use {
    crate::{
        configuration::{CargoPackage, CargoSource, Identity, Package, Resource},
        convergence::assess::SourceReadings,
        desired_state::{DesiredState, ResolvedResource},
    },
    anyhow::{Result, bail},
    std::collections::BTreeMap,
};

pub fn resolve(
    desired_state: &DesiredState,
    readings: &SourceReadings,
) -> Result<Vec<ResolvedResource>> {
    let mut resources = desired_state.resources.clone();

    for workspace in &desired_state.workspaces {
        let repository = &workspace.declared().repository;
        match readings.workspace(&workspace.clone_directory(repository)) {
            Some(Ok(Some(reading))) => {
                for crate_name in reading.members.keys() {
                    resources.push(workspace.alongside(Resource::Package(Package::Cargo(
                        CargoPackage {
                            crate_name: crate_name.clone(),
                            source: CargoSource::Workspace {
                                repository: repository.clone(),
                            },
                        },
                    ))));
                }
            }
            Some(Ok(None)) | None => {}
            Some(Err(reason)) => bail!(
                "{} could not be read, so which crates it holds is unknown: {reason}. Nothing was \
                 applied.",
                workspace.declared()
            ),
        }
    }

    reject_conflicting_claims(&resources)?;
    Ok(resources)
}

fn reject_conflicting_claims(resources: &[ResolvedResource]) -> Result<()> {
    let mut claimed: BTreeMap<Identity, &ResolvedResource> = BTreeMap::new();

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
