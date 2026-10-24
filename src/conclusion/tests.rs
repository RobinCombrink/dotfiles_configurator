use super::*;

#[test]
fn a_converged_machine_exits_zero_and_an_unconverged_one_exits_non_zero() {
    assert_eq!(Conclusion::of(true).status(), 0);
    assert_eq!(Conclusion::of(false).status(), 1);
}

#[test]
fn a_run_that_changed_nothing_exits_as_neither_converged_nor_failed() {
    assert_eq!(Conclusion::DidNothing.status(), 2);
}
