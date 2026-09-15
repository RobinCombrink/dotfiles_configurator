use std::{
    fmt::Display,
    io::{IsTerminal, Write},
};

/// What a person says about the change set they have been shown. Anything but a plain yes leaves
/// the machine alone, which is what the `[y/N]` in the question promises.
///
/// ```
/// # use dotfiles_configurator::confirmation::Confirmation;
/// assert_eq!(Confirmation::from("y"), Confirmation::Proceed);
/// assert_eq!(Confirmation::from("YES\n"), Confirmation::Proceed);
/// assert_eq!(Confirmation::from(""), Confirmation::Declined);
/// assert_eq!(Confirmation::from("no"), Confirmation::Declined);
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Confirmation {
    Proceed,
    Declined,
}

impl From<&str> for Confirmation {
    fn from(answer: &str) -> Self {
        match answer.trim().to_ascii_lowercase().as_str() {
            "y" | "yes" => Confirmation::Proceed,
            _ => Confirmation::Declined,
        }
    }
}

pub trait Confirm {
    fn confirmation(&self) -> Confirmation;
}

/// Who answers for the change set a run has printed: `--yes` in a person's place, or the person
/// themselves at a terminal.
///
/// ```
/// # use dotfiles_configurator::confirmation::{Confirm, Confirmation, Operator};
/// let answered_in_advance = Operator::of_this_run(true).unwrap();
/// assert_eq!(answered_in_advance.confirmation(), Confirmation::Proceed);
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Operator {
    AnsweredInAdvance,
    AtATerminal,
}

/// A run that could put its question to nobody and was answered by nobody in advance.
///
/// ```
/// # use dotfiles_configurator::confirmation::NoOneToAsk;
/// assert!(NoOneToAsk.to_string().contains("--yes"));
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NoOneToAsk;

impl Display for NoOneToAsk {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(
            "Refusing to change this machine unseen: there is no terminal to show the change set \
             on and none to read an answer from. Pass --yes to apply without being asked.",
        )
    }
}

impl std::error::Error for NoOneToAsk {}

const QUESTION: &str = "Proceed? [y/N] ";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Asking {
    Possible,
    Impossible,
}

impl Asking {
    // ADR 0013
    fn of_this_process() -> Self {
        match std::io::stdin().is_terminal() && std::io::stderr().is_terminal() {
            true => Asking::Possible,
            false => Asking::Impossible,
        }
    }
}

impl Operator {
    pub fn of_this_run(answered_in_advance: bool) -> Result<Self, NoOneToAsk> {
        Self::of(answered_in_advance, Asking::of_this_process())
    }

    fn of(answered_in_advance: bool, asking: Asking) -> Result<Self, NoOneToAsk> {
        match (answered_in_advance, asking) {
            (true, _) => Ok(Operator::AnsweredInAdvance),
            (false, Asking::Possible) => Ok(Operator::AtATerminal),
            (false, Asking::Impossible) => Err(NoOneToAsk),
        }
    }
}

impl Confirm for Operator {
    fn confirmation(&self) -> Confirmation {
        match self {
            Operator::AnsweredInAdvance => Confirmation::Proceed,
            Operator::AtATerminal => ask_at_the_terminal(),
        }
    }
}

fn ask_at_the_terminal() -> Confirmation {
    let mut question_goes_to = std::io::stderr();
    let _ = write!(question_goes_to, "{QUESTION}");
    let _ = question_goes_to.flush();

    let mut answer = String::new();
    match std::io::stdin().read_line(&mut answer) {
        Ok(0) | Err(_) => Confirmation::Declined,
        Ok(_) => Confirmation::from(answer.as_str()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_run_answered_in_advance_is_not_asked_even_where_it_could_have_been() {
        assert_eq!(
            Operator::of(true, Asking::Possible),
            Ok(Operator::AnsweredInAdvance)
        );
    }

    #[test]
    fn a_run_answered_in_advance_proceeds_where_nothing_could_have_been_asked() {
        assert_eq!(
            Operator::of(true, Asking::Impossible),
            Ok(Operator::AnsweredInAdvance)
        );
    }

    #[test]
    fn a_run_that_can_ask_puts_the_question_to_the_person_at_the_terminal() {
        assert_eq!(
            Operator::of(false, Asking::Possible),
            Ok(Operator::AtATerminal)
        );
    }

    #[test]
    fn a_run_that_can_neither_ask_nor_was_answered_in_advance_is_refused() {
        assert_eq!(Operator::of(false, Asking::Impossible), Err(NoOneToAsk));
    }

    #[test]
    fn an_answer_given_in_advance_is_taken_without_reading_the_terminal() {
        assert_eq!(
            Operator::AnsweredInAdvance.confirmation(),
            Confirmation::Proceed
        );
    }

    #[test]
    fn the_question_says_that_an_unanswered_one_leaves_the_machine_alone() {
        assert!(QUESTION.contains("[y/N]"), "{QUESTION}");
    }

    #[test]
    fn an_answer_of_something_other_than_yes_declines() {
        assert_eq!(Confirmation::from("maybe"), Confirmation::Declined);
    }

    #[test]
    fn an_answer_surrounded_by_whitespace_is_read_as_the_word_it_holds() {
        assert_eq!(Confirmation::from("  y \n"), Confirmation::Proceed);
    }
}
