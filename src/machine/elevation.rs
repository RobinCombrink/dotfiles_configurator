use {
    serde::{Deserialize, Serialize},
    sha2::{Digest, Sha256},
    std::{
        fmt::{self, Display},
        io::{self, Read},
        path::PathBuf,
    },
};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ElevatedWork {
    Link {
        link_path: PathBuf,
        target_path: PathBuf,
    },
    Installer {
        installer_path: PathBuf,
        digest: ContentDigest,
    },
    #[cfg(target_family = "windows")]
    MachineSearchPathFront { directory: PathBuf },
}

impl Display for ElevatedWork {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ElevatedWork::Link {
                link_path,
                target_path,
            } => write!(
                formatter,
                "link {} to {}",
                link_path.display(),
                target_path.display()
            ),
            ElevatedWork::Installer { installer_path, .. } => {
                write!(formatter, "run {}", installer_path.display())
            }
            #[cfg(target_family = "windows")]
            ElevatedWork::MachineSearchPathFront { directory } => write!(
                formatter,
                "put {} first on the Machine PATH",
                directory.display()
            ),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct ContentDigest([u8; 32]);

impl ContentDigest {
    pub fn of(mut contents: impl Read) -> io::Result<Self> {
        let mut hasher = Sha256::new();
        let mut buffer = vec![0; 64 * 1024];
        loop {
            match contents.read(&mut buffer) {
                Ok(0) => return Ok(Self(hasher.finalize().into())),
                Ok(read) => hasher.update(&buffer[..read]),
                Err(error) if error.kind() == io::ErrorKind::Interrupted => {}
                Err(error) => return Err(error),
            }
        }
    }
}

impl Display for ContentDigest {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("SHA-256 ")?;
        self.0
            .iter()
            .try_for_each(|byte| write!(formatter, "{byte:02x}"))
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PrivilegeRefusal {
    pub work: ElevatedWork,
    pub refusal: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ElevatedBatch<Outcome> {
    pub entries: Vec<Batched<Outcome>>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Batched<Outcome> {
    pub work: ElevatedWork,
    pub outcome: Outcome,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ElevatedOutcome {
    Converged,
    Failed { reason: String },
}

impl Display for ElevatedOutcome {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ElevatedOutcome::Converged => formatter.write_str("converged"),
            ElevatedOutcome::Failed { reason } => write!(formatter, "failed: {reason}"),
        }
    }
}

impl ElevatedBatch<()> {
    pub fn of(works: impl IntoIterator<Item = ElevatedWork>) -> Self {
        Self {
            entries: works
                .into_iter()
                .map(|work| Batched { work, outcome: () })
                .collect(),
        }
    }
}

impl ElevatedBatch<ElevatedOutcome> {
    pub fn outcome_of(&self, work: &ElevatedWork) -> Option<&ElevatedOutcome> {
        self.entries
            .iter()
            .find(|batched| batched.work == *work)
            .map(|batched| &batched.outcome)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Elevation {
    Performed(ElevatedBatch<ElevatedOutcome>),
    Declined,
}

#[cfg(test)]
mod tests;
