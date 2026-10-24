use {
    crate::{
        currency::{own_release_repository, this_build},
        version::Version,
    },
    std::fmt::Display,
};

pub fn still_unreadable_by_the_newest_build(refusal: &impl Display, replaced: &Version) -> String {
    format!(
        "{refusal}
{} replaced {replaced} earlier in this run and cannot read it either, so no release of {} does \
         yet.",
        this_build(),
        own_release_repository()
    )
}

#[cfg(test)]
mod tests;
