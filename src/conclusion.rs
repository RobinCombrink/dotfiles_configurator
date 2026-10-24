use std::process::ExitCode;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Conclusion {
    Converged,
    Unconverged,
    DidNothing,
    Settled,
}

impl Conclusion {
    pub fn of(converged: bool) -> Self {
        match converged {
            true => Conclusion::Converged,
            false => Conclusion::Unconverged,
        }
    }

    // ADR 0004, ADR 0013
    pub fn status(self) -> u8 {
        match self {
            Conclusion::Converged | Conclusion::Settled => 0,
            Conclusion::Unconverged => 1,
            Conclusion::DidNothing => 2,
        }
    }
}

impl From<Conclusion> for ExitCode {
    fn from(conclusion: Conclusion) -> Self {
        ExitCode::from(conclusion.status())
    }
}

#[cfg(test)]
mod tests;
