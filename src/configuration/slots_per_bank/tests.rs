use {
    super::*,
    crate::configuration::{BUILD_GENERATION, Context, GitHubAccount},
};

fn configuration(slots_per_bank: Option<SlotsPerBank>) -> Configuration {
    Configuration {
        version: BUILD_GENERATION,
        applies_to: Context::Everywhere,
        github_account: GitHubAccount::from("Alice"),
        estate: None,
        slots_per_bank,
        workspaces: Vec::new(),
        resources: Vec::new(),
        notices: Vec::new(),
    }
}

fn named(position: usize) -> ConfigurationName {
    ConfigurationName::from(format!("{position}.dotconfig.json"))
}

fn resolved(
    configurations: &[Configuration],
) -> Result<Option<SlotsPerBank>, SlotsPerBankDeclaredTwice> {
    let names: Vec<ConfigurationName> = (0..configurations.len()).map(named).collect();
    resolve_slots_per_bank(names.iter().zip(configurations))
}

#[test]
fn configurations_declaring_no_slots_per_bank_resolve_to_none() {
    assert_eq!(
        resolved(&[configuration(None), configuration(None)]),
        Ok(None)
    );
}

#[test]
fn the_one_configuration_declaring_slots_per_bank_decides_them() {
    for slots_per_bank in [SlotsPerBank::Two, SlotsPerBank::Three] {
        assert_eq!(
            resolved(&[configuration(None), configuration(Some(slots_per_bank))]),
            Ok(Some(slots_per_bank))
        );
    }
}

#[test]
fn two_configurations_declaring_slots_per_bank_are_refused_naming_both() {
    assert_eq!(
        resolved(&[
            configuration(Some(SlotsPerBank::Two)),
            configuration(None),
            configuration(Some(SlotsPerBank::Two)),
        ]),
        Err(SlotsPerBankDeclaredTwice {
            first: named(0),
            second: named(2),
        })
    );
}
