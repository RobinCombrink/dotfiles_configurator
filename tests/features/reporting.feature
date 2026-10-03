Feature: Reporting a run

  A run that emits nothing until it exits is indistinguishable from a hung one, so both
  subcommands say what they are doing while they do it, and write every child process's output
  down as it arrives. The log is what a run that turns out to have been interesting is read from
  afterwards, so it is written whether or not anything drifted.

  Scenario: A run writes down what it converged
    Given Alice declares the application "Neovim"
    And Neovim is not installed on Alice's machine
    When Alice applies
    Then the log of Alice's run names "Neovim"

  Scenario: A run that finds no drift is still written down
    Given Alice declares the application "Neovim"
    And Neovim is installed on Alice's machine
    When Alice applies
    Then the log of Alice's run names "Neovim"

  Scenario: Two runs write a log each rather than interleaving into one
    Given Alice declares the application "Neovim"
    And Neovim is not installed on Alice's machine
    When Alice applies twice
    Then 2 runs are logged

  Scenario: Without a terminal, a run reports each change as it starts and as it ends
    Given Alice declares the application "Neovim"
    And Neovim is not installed on Alice's machine
    When Alice applies
    Then Alice's run reports "Neovim" as "installing" and then as "installed"

  Scenario: A run closes on a tally of what it converged, failed and held
    Given a newer configurator than this machine holds has been released
    And Alice's machine is running the configurator and will not let it be replaced
    When Alice applies
    Then Alice's run closes on a tally of 0 converged, 0 failed and 1 held

  Scenario: Only the twenty most recent runs are kept
    Given 30 runs have already been logged
    And Alice declares the application "Neovim"
    When Alice applies
    Then 20 runs are logged
