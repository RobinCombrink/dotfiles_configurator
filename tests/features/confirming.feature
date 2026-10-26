Feature: Confirming a change set before it is enacted

  Applying shows what it is about to do and asks once, so that a wrong configuration is visible
  before any of it reaches the machine and stoppable while it still can be. Later passes converge
  what became ready once earlier changes landed, and are not asked about again.

  Scenario: The change set is shown before the first resource is converged
    Given Alice declares the application "Neovim"
    And Neovim is not installed on Alice's machine
    When Alice applies
    Then Alice was shown "application Neovim" before it was converged

  Scenario: Confirming enacts the change set that was shown
    Given Alice declares the application "Neovim"
    And Neovim is not installed on Alice's machine
    When Alice applies
    Then Neovim is installed on Alice's machine
    And the machine is reported as converged

  Scenario: Declining leaves the machine as it was
    Given Alice declares the application "Neovim"
    And Neovim is not installed on Alice's machine
    And Alice declines the change set
    When Alice applies
    Then nothing on Alice's machine has changed
    And the run is reported as having done nothing

  Scenario: A machine with nothing to enact is asked nothing
    Given Alice declares the application "Neovim"
    And Neovim is installed on Alice's machine
    When Alice applies
    Then the run asked nothing
    And the machine is reported as converged

  Scenario: A configuration waiting to be rewritten is asked about even with no drift
    Given Alice declares the application "Neovim"
    And Neovim is installed on Alice's machine
    And Alice's configuration is waiting to be rewritten a generation forward
    When Alice applies
    Then the run asked once

  Scenario: Confirming rewrites the configuration that was waiting to be rewritten
    Given Alice declares the application "Neovim"
    And Alice's configuration is waiting to be rewritten a generation forward
    When Alice applies
    Then Alice's configuration was rewritten

  Scenario: Declining rewrites no configuration forward
    Given Alice declares the application "Neovim"
    And Alice's configuration is waiting to be rewritten a generation forward
    And Alice declines the change set
    When Alice applies
    Then Alice's configuration was not rewritten

  Scenario: A resource that becomes ready after the first pass is not asked about again
    Given Alice's configurations come from the dotfiles repository
    And Alice declares the symlink "gitconfig/.gitconfig" at ".gitconfig"
    And the dotfiles repository holds "gitconfig/.gitconfig"
    And the dotfiles repository has not been cloned on Alice's machine
    When Alice applies
    Then the link ".gitconfig" points into the dotfiles repository
    And the run asked once

  Scenario: An answer given in advance enacts the change set
    Given Alice declares the application "Neovim"
    And Neovim is not installed on Alice's machine
    And Alice has answered in advance
    When Alice applies
    Then Neovim is installed on Alice's machine
    And the machine is reported as converged

  Scenario: An answer given in advance is still shown the change set
    Given Alice declares the application "Neovim"
    And Neovim is not installed on Alice's machine
    And Alice has answered in advance
    When Alice applies
    Then Alice was shown "application Neovim" before it was converged

  Scenario: An answer given in advance answers the one question the run asks
    Given Alice declares the application "Neovim"
    And Neovim is not installed on Alice's machine
    And Alice has answered in advance
    When Alice applies
    Then the run asked once
    And Neovim is installed on Alice's machine

  Scenario: A run answered in advance with nothing to enact asks nothing
    Given Alice declares the application "Neovim"
    And Neovim is installed on Alice's machine
    And Alice has answered in advance
    When Alice applies
    Then the run asked nothing

  Scenario: A configuration stating a generation this build cannot read puts the question
    Given Alice has a configuration declaring a generation beyond this build
    And a newer configurator than this machine holds has been released
    When Alice applies, reading her configurations before anything else
    Then the run asked once

  Scenario: Declining the newer build leaves the machine as it was
    Given Alice has a configuration declaring a generation beyond this build
    And a newer configurator than this machine holds has been released
    And Alice declines the change set
    When Alice applies, reading her configurations before anything else
    Then the run declined the newer build
    And nothing on Alice's machine has changed

  Scenario: An answer given in advance obtains the newer build without a prompt
    Given Alice has a configuration declaring a generation beyond this build
    And a newer configurator than this machine holds has been released
    And Alice has answered in advance
    When Alice applies, reading her configurations before anything else
    Then the run obtained the newer build
    And the configurator reports the version of its latest release
    And the run asked once

  Scenario: A run that already replaced an earlier build asks nothing and installs nothing
    Given Alice has a configuration declaring a generation beyond this build
    And a newer configurator than this machine holds has been released
    And Alice's run is being carried on by the build that replaced "3.18.0"
    When Alice applies, reading her configurations before anything else
    Then the run failed before reading her configurations
    And the run asked nothing
    And the configurator still reports the version the machine held
