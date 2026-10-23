use {
    crate::configuration::{Configuration, names::ConfigurationName},
    machine_manifest_document::SlotsPerBank,
    std::fmt::Display,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SlotsPerBankDeclaredTwice {
    pub first: ConfigurationName,
    pub second: ConfigurationName,
}

impl Display for SlotsPerBankDeclaredTwice {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            formatter,
            "How many slots a bank holds is declared by both {} and {}. A machine's slots per \
             bank are declared by one configuration.",
            self.first, self.second
        )
    }
}

impl std::error::Error for SlotsPerBankDeclaredTwice {}

pub fn resolve_slots_per_bank<'configuration>(
    configurations: impl IntoIterator<
        Item = (
            &'configuration ConfigurationName,
            &'configuration Configuration,
        ),
    >,
) -> Result<Option<SlotsPerBank>, SlotsPerBankDeclaredTwice> {
    let mut declared: Option<(&ConfigurationName, SlotsPerBank)> = None;

    for (name, configuration) in configurations {
        let Some(slots_per_bank) = configuration.slots_per_bank else {
            continue;
        };
        if let Some((first, _)) = declared {
            return Err(SlotsPerBankDeclaredTwice {
                first: first.clone(),
                second: name.clone(),
            });
        }
        declared = Some((name, slots_per_bank));
    }

    Ok(declared.map(|(_, slots_per_bank)| slots_per_bank))
}

#[cfg(test)]
#[path = "slots_per_bank/tests.rs"]
mod tests;
